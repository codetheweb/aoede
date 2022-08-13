use std::env;
use std::process::exit;

use lib::config::Config;
use songbird::{input, SerenityInit};

mod lib {
    pub mod config;
    pub mod player;
}
use figment::error::Kind::MissingField;
use lib::player::{SpotifyPlayer, SpotifyPlayerKey};
use librespot::core::mercury::MercuryError;
use librespot::playback::config::Bitrate;
use librespot::playback::player::PlayerEvent;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

use serenity::Client;

use serenity::prelude::TypeMapKey;

use serenity::{
    async_trait,
    client::{Context, EventHandler},
    framework::StandardFramework,
    model::{gateway, gateway::Ready, id, user, voice::VoiceState},
};

struct Handler;

pub struct ConfigKey;
impl TypeMapKey for ConfigKey {
    type Value = Config;
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("Ready!");
        println!("Invite me with https://discord.com/api/oauth2/authorize?client_id={}&permissions=36700160&scope=bot", ready.user.id);

        ctx.invisible().await;
    }

    async fn cache_ready(&self, ctx: Context, guilds: Vec<id::GuildId>) {
        let guild_id = match guilds.first() {
            Some(guild_id) => *guild_id,
            None => {
                panic!("Not currently in any guilds.");
            }
        };

        let data = ctx.data.read().await;

        let player = data.get::<SpotifyPlayerKey>().unwrap().clone();
        let config = data.get::<ConfigKey>().unwrap().clone();

        // Handle case when user is in VC when bot starts
        let guild = ctx
            .cache
            .guild(guild_id)
            .expect("Could not find guild in cache.");

        let channel_id = guild
            .voice_states
            .get(&config.discord_user_id.into())
            .and_then(|voice_state| voice_state.channel_id);
        drop(guild);

        if channel_id.is_some() {
            // Enable casting
            player.lock().await.enable_connect().await;
        }

        let c = ctx.clone();

        // Handle Spotify events
        tokio::spawn(async move {
            loop {
                let channel = player.lock().await.event_channel.clone().unwrap();
                let mut receiver = channel.lock().await;

                let event = match receiver.recv().await {
                    Some(e) => e,
                    None => {
                        // Busy waiting bad but quick and easy
                        sleep(Duration::from_millis(256)).await;
                        continue;
                    }
                };

                match event {
                    PlayerEvent::Stopped { .. } => {
                        c.set_presence(None, user::OnlineStatus::Online).await;

                        let manager = songbird::get(&c)
                            .await
                            .expect("Songbird Voice client placed in at initialization.")
                            .clone();

                        let _ = manager.remove(guild_id).await;
                    }

                    PlayerEvent::Started { .. } => {
                        let manager = songbird::get(&c)
                            .await
                            .expect("Songbird Voice client placed in at initialization.")
                            .clone();

                        let guild = c
                            .cache
                            .guild(guild_id)
                            .expect("Could not find guild in cache.");

                        let channel_id = match guild
                            .voice_states
                            .get(&config.discord_user_id.into())
                            .and_then(|voice_state| voice_state.channel_id)
                        {
                            Some(channel_id) => channel_id,
                            None => {
                                println!("Could not find user in VC.");
                                continue;
                            }
                        };

                        let _handler = manager.join(guild_id, channel_id).await;

                        if let Some(handler_lock) = manager.get(guild_id) {
                            let mut handler = handler_lock.lock().await;

                            let mut decoder = input::codec::OpusDecoderState::new().unwrap();
                            decoder.allow_passthrough = false;

                            let source = input::Input::new(
                                true,
                                input::reader::Reader::Extension(Box::new(
                                    player.lock().await.emitted_sink.clone(),
                                )),
                                input::codec::Codec::FloatPcm,
                                input::Container::Raw,
                                None,
                            );

                            handler.set_bitrate(songbird::driver::Bitrate::Auto);

                            handler.play_only_source(source);
                        } else {
                            println!("Could not fetch guild by ID.");
                        }
                    }

                    PlayerEvent::Paused { .. } => {
                        c.set_presence(None, user::OnlineStatus::Online).await;
                    }

                    PlayerEvent::Playing { track_id, .. } => {
                        let track: Result<librespot::metadata::Track, MercuryError> =
                            librespot::metadata::Metadata::get(
                                &player.lock().await.session,
                                track_id,
                            )
                            .await;

                        if let Ok(track) = track {
                            let artist: Result<librespot::metadata::Artist, MercuryError> =
                                librespot::metadata::Metadata::get(
                                    &player.lock().await.session,
                                    *track.artists.first().unwrap(),
                                )
                                .await;

                            if let Ok(artist) = artist {
                                let listening_to = format!("{}: {}", artist.name, track.name);

                                c.set_presence(
                                    Some(gateway::Activity::listening(listening_to)),
                                    user::OnlineStatus::Online,
                                )
                                .await;
                            }
                        }
                    }

                    _ => {}
                }
            }
        });
    }

    async fn voice_state_update(&self, ctx: Context, old: Option<VoiceState>, new: VoiceState) {
        let data = ctx.data.read().await;

        let config = data.get::<ConfigKey>().unwrap();

        if new.user_id.to_string() != config.discord_user_id.to_string() {
            return;
        }

        let player = data.get::<SpotifyPlayerKey>().unwrap();

        let guild = ctx
            .cache
            .guild(ctx.cache.guilds().first().unwrap())
            .unwrap();

        // If user just connected
        if old.clone().is_none() {
            // Enable casting
            ctx.set_presence(None, user::OnlineStatus::Online).await;
            player.lock().await.enable_connect().await;
            return;
        }

        // If user disconnected
        if old.clone().unwrap().channel_id.is_some() && new.channel_id.is_none() {
            // Disable casting
            ctx.invisible().await;
            player.lock().await.disable_connect().await;

            // Disconnect
            let manager = songbird::get(&ctx)
                .await
                .expect("Songbird Voice client placed in at initialization.")
                .clone();

            let _handler = manager.remove(guild.id).await;

            return;
        }

        // If user moved channels
        if old.unwrap().channel_id.unwrap() != new.channel_id.unwrap() {
            let bot_id = ctx.cache.current_user_id();

            let bot_channel = guild
                .voice_states
                .get(&bot_id)
                .and_then(|voice_state| voice_state.channel_id);

            if Option::is_some(&bot_channel) {
                let manager = songbird::get(&ctx)
                    .await
                    .expect("Songbird Voice client placed in at initialization.")
                    .clone();

                if let Some(guild_id) = ctx.cache.guilds().first() {
                    let _handler = manager.join(*guild_id, new.channel_id.unwrap()).await;
                }
            }

            return;
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let framework = StandardFramework::new();

    let config = match Config::new() {
        Ok(config) => config,
        Err(error) => {
            println!("Couldn't read config");
            if let MissingField(f) = error.kind {
                println!("Missing field: '{}'", f.to_uppercase());
            } else {
                println!("Error: {:?}", error);
                exit(2)
            }
            exit(1)
        }
    };

    let mut cache_dir = None;

    if let Ok(c) = env::var("CACHE_DIR") {
        cache_dir = Some(c);
    }

    let player = Arc::new(Mutex::new(
        SpotifyPlayer::new(
            config.spotify_username.clone(),
            config.spotify_password.clone(),
            Bitrate::Bitrate320,
            cache_dir,
        )
        .await,
    ));

    let mut client = Client::builder(
        &config.discord_token,
        gateway::GatewayIntents::non_privileged(),
    )
    .event_handler(Handler)
    .framework(framework)
    .type_map_insert::<SpotifyPlayerKey>(player)
    .type_map_insert::<ConfigKey>(config)
    .register_songbird()
    .await
    .expect("Err creating client");

    let _ = client
        .start()
        .await
        .map_err(|why| println!("Client ended: {:?}", why));
}
