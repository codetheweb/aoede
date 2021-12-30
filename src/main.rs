use std::env;

use songbird::input;
use songbird::SerenityInit;

mod lib {
    pub mod player;
    // pub mod forward_mpsc;
}
use lib::player::{SpotifyPlayer, SpotifyPlayerKey};
use librespot::core::mercury::MercuryError;
use librespot::playback::config::Bitrate;
use librespot::playback::player::PlayerEvent;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

use serenity::client::Context;

use serenity::prelude::TypeMapKey;

use serenity::{
    async_trait,
    client::{Client, EventHandler},
    framework::StandardFramework,
    model::{gateway, gateway::Ready, id, user, voice::VoiceState},
};
use songbird::tracks::TrackHandle;

struct Handler;

pub struct UserIdKey;
impl TypeMapKey for UserIdKey {
    type Value = id::UserId;
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
        let user_id = *data
            .get::<UserIdKey>()
            .expect("User ID placed in at initialisation.");

        // Handle case when user is in VC when bot starts
        let guild = ctx
            .cache
            .guild(guild_id)
            .await
            .expect("Could not find guild in cache.");

        let channel_id = guild
            .voice_states
            .get(&user_id)
            .and_then(|voice_state| voice_state.channel_id);
        drop(guild);

        if channel_id.is_some() {
            // Enable casting
            player.lock().await.enable_connect().await;
        }

        let c = ctx.clone();

        // Handle Spotify events
        tokio::spawn(async move {
            let mut track_handle: Option<TrackHandle> = None;
            let mut current_volume = 0f32;
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
                            .expect("Songbird Voice client placed in at initialisation.")
                            .clone();

                        let _ = manager.remove(guild_id).await;

                        track_handle = None;
                    }

                    PlayerEvent::Started { .. } => {
                        let manager = songbird::get(&c)
                            .await
                            .expect("Songbird Voice client placed in at initialisation.")
                            .clone();

                        let guild = c
                            .cache
                            .guild(guild_id)
                            .await
                            .expect("Could not find guild in cache.");

                        let channel_id = match guild
                            .voice_states
                            .get(&user_id)
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

                            track_handle = Some(handler.play_only_source(source));
                            if track_handle.is_some() {
                                track_handle.as_ref().unwrap().set_volume(current_volume).expect("Track handler should be valid");
                            }
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

                    PlayerEvent::VolumeSet {volume} => {
                        current_volume = volume as f32 / u16::MAX as f32;
                        if track_handle.is_some() {
                            track_handle.as_ref().unwrap().set_volume(current_volume).expect("Track handler should be valid");
                        }
                    }

                    _ => {}
                }
            }
        });
    }

    async fn voice_state_update(
        &self,
        ctx: Context,
        _: Option<id::GuildId>,
        old: Option<VoiceState>,
        new: VoiceState,
    ) {
        let data = ctx.data.read().await;

        let user_id = data.get::<UserIdKey>();

        if new.user_id.to_string() != user_id.unwrap().to_string() {
            return;
        }

        let player = data.get::<SpotifyPlayerKey>().unwrap();

        let guild = ctx
            .cache
            .guild(ctx.cache.guilds().await.first().unwrap())
            .await
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
                .expect("Songbird Voice client placed in at initialisation.")
                .clone();

            let _handler = manager.remove(guild.id).await;

            return;
        }

        // If user moved channels
        if old.unwrap().channel_id.unwrap() != new.channel_id.unwrap() {
            let bot_id = ctx.cache.current_user_id().await;

            let bot_channel = guild
                .voice_states
                .get(&bot_id)
                .and_then(|voice_state| voice_state.channel_id);

            if Option::is_some(&bot_channel) {
                let manager = songbird::get(&ctx)
                    .await
                    .expect("Songbird Voice client placed in at initialisation.")
                    .clone();

                if let Some(guild_id) = ctx.cache.guilds().await.first() {
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

    // Configure the client with your Discord bot token in the environment.
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

    let framework = StandardFramework::new();
    let username =
        env::var("SPOTIFY_USERNAME").expect("Expected a Spotify username in the environment");
    let password =
        env::var("SPOTIFY_PASSWORD").expect("Expected a Spotify password in the environment");
    let user_id =
        env::var("DISCORD_USER_ID").expect("Expected a Discord user ID in the environment");

    let mut cache_dir = None;

    if let Ok(c) = env::var("CACHE_DIR") {
        cache_dir = Some(c);
    }

    let player = Arc::new(Mutex::new(
        SpotifyPlayer::new(username, password, Bitrate::Bitrate320, cache_dir).await,
    ));

    let mut client = Client::builder(&token)
        .event_handler(Handler)
        .framework(framework)
        .type_map_insert::<SpotifyPlayerKey>(player)
        .type_map_insert::<UserIdKey>(id::UserId::from(user_id.parse::<u64>().unwrap()))
        .register_songbird()
        .await
        .expect("Err creating client");

    let _ = client
        .start()
        .await
        .map_err(|why| println!("Client ended: {:?}", why));
}
