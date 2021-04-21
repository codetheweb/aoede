//! Requires the "client", "standard_framework", and "voice" features be enabled in your
//! Cargo.toml, like so:
//!
//! ```toml
//! [dependencies.serenity]
//! git = "https://github.com/serenity-rs/serenity.git"
//! features = ["client", standard_framework", "voice"]
//! ```
use std::env;

// This trait adds the `register_songbird` and `register_songbird_with` methods
// to the client builder below, making it easy to install this voice client.
// The voice client can be retrieved in any command using `songbird::get(ctx).await`.
use songbird::input;
use songbird::SerenityInit;

mod lib {
    pub mod player;
}
use lib::player::{SpotifyPlayer, SpotifyPlayerKey};
use librespot::core::config::{DeviceType, VolumeCtrl};
use librespot::core::mercury::MercuryError;
use librespot::playback::config::Bitrate;
use librespot::playback::player::PlayerEvent;
use std::sync::Arc;
use tokio::sync::Mutex;

use tokio::time::sleep;

use std::time::Duration;

// Import the `Context` to handle commands.
use serenity::client::Context;

use serenity::prelude::TypeMapKey;

use serenity::{
    async_trait,
    client::{Client, EventHandler},
    framework::{
        standard::{
            macros::{command, group},
            Args, CommandResult,
        },
        StandardFramework,
    },
    model::{channel::Message, gateway, gateway::Ready, id, user, voice::VoiceState},
    Result as SerenityResult,
};

struct Handler;

pub struct UserIdKey;
impl TypeMapKey for UserIdKey {
    type Value = id::UserId;
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, _ready: Ready) {
        // TODO: handle case when user is in VC when bot starts
        println!("Ready!");
        let data = ctx.data.read().await;

        let player = data.get::<SpotifyPlayerKey>().unwrap().clone();
        let user_id = *data
            .get::<UserIdKey>()
            .expect("User ID placed in at initialisation.");

        let c = ctx.clone();

        // Spawn event channel handler for Spotify
        tokio::spawn(async move {
            loop {
                let channel = player.lock().await.event_channel.clone().unwrap();
                let mut receiver = channel.lock().await;

                let event = match receiver.recv().await {
                    Some(e) => e,
                    None => {
                        // Busy waiting bad but works fine
                        sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                };

                let guild = match c.cache.guilds().await.first() {
                    Some(guild_id) => match c.cache.guild(guild_id).await {
                        Some(guild) => guild,
                        None => continue,
                    },
                    None => {
                        println!("Not currently in any guilds.");
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

                        let _ = manager.leave(guild.id).await;
                    }

                    PlayerEvent::Started { .. } => {
                        let manager = songbird::get(&c)
                            .await
                            .expect("Songbird Voice client placed in at initialisation.")
                            .clone();

                        let channel_id = guild
                            .voice_states
                            .get(&user_id)
                            .and_then(|voice_state| voice_state.channel_id);

                        let _handler = manager.join(guild.id, channel_id.unwrap()).await;

                        if let Some(handler_lock) = manager.get(guild.id) {
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

                            handler.set_bitrate(songbird::Bitrate::Auto);

                            handler.play_source(source);
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

    async fn voice_state_update(
        &self,
        ctx: Context,
        _: Option<id::GuildId>,
        old: Option<VoiceState>,
        new: VoiceState,
    ) {
        let data = ctx.data.read().await;

        // disconnect = old channel id, no new channel id
        // connect = old none, new channel id
        // move = check current connected channel

        let user_id = data.get::<UserIdKey>();

        if new.user_id.to_string() != user_id.unwrap().to_string() {
            return;
        }

        let player = data.get::<SpotifyPlayerKey>().unwrap();

        // If user just connected
        if old.clone().is_none() {
            // Enable casting
            player
                .lock()
                .await
                .enable_connect(
                    "Aoede".to_string(),
                    DeviceType::GameConsole,
                    1u16,
                    VolumeCtrl::default(),
                )
                .await;
            return;
        }

        // If user disconnected
        if old.clone().unwrap().channel_id.is_some() && new.channel_id.is_none() {
            // Disable casting
            player.lock().await.disable_connect();
            return;
        }

        // If user moved channels
        if old.unwrap().channel_id.unwrap() != new.channel_id.unwrap() {
            println!("movding...");
            // TODO: move with user
            return;
        }

        // let mut disconnected = false;

        // if old.is_some() && old.unwrap().channel_id.is_some() && new.channel_id.is_none() {
        //     disconnected = true;
        // }

        // let  player = data.get::<SpotifyPlayerKey>().unwrap();
        // let is_spirc_defined = player.lock().await.spirc.is_some();

        // let should_change = (disconnected && is_spirc_defined) || (!disconnected && !is_spirc_defined);

        // if !should_change {
        //     return;
        // }

        // let manager = songbird::get(&ctx).await.unwrap();

        // if disconnected {
        //     let _handler = manager.leave(new.guild_id.unwrap()).await;
        //     // player.disable_connect();
        // } else {
        //     let _handler = manager.join(new.guild_id.unwrap(), new.channel_id.unwrap()).await;

        //     if let Some(handler_lock) = manager.get(new.guild_id.unwrap()) {
        //         let mut handler = handler_lock.lock().await;

        //         player.lock().await.enable_connect("Aoede".to_string(), DeviceType::AudioDongle, 1u16, VolumeCtrl::default());

        //         let mut decoder = input::codec::OpusDecoderState::new().unwrap();
        //     decoder.allow_passthrough = false;

        //     let source = input::Input::new(
        //         true,
        //         input::reader::Reader::Extension(Box::new(player.lock().await.emitted_sink.clone())),
        //         input::codec::Codec::FloatPcm,
        //         input::Container::Raw,
        //         None
        //     );

        //     handler.set_bitrate(songbird::Bitrate::Auto);

        //     handler.play_source(source);
        // } else {
        //     print!("could not lock");
        // }
        // }
    }
}

#[group]
#[commands(deafen, join, leave, mute, play, ping, undeafen, unmute)]
struct General;

#[tokio::main]
async fn main() {
    // TODO: handle volume
    // TODO: handle cache directory

    tracing_subscriber::fmt::init();

    // Configure the client with your Discord bot token in the environment.
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

    let framework = StandardFramework::new()
        .configure(|c| c.prefix("~"))
        .group(&GENERAL_GROUP);

    let username =
        env::var("SPOTIFY_USERNAME").expect("Expected a Spotify username in the environment");
    let password =
        env::var("SPOTIFY_PASSWORD").expect("Expected a Spotify password in the environment");
    let user_id =
        env::var("DISCORD_USER_ID").expect("Expected a Discord user ID in the environment");

    let player = Arc::new(Mutex::new(
        SpotifyPlayer::new(username, password, Bitrate::Bitrate320, "asdf".to_string()).await,
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

#[command]
#[only_in(guilds)]
async fn deafen(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    let handler_lock = match manager.get(guild_id) {
        Some(handler) => handler,
        None => {
            check_msg(msg.reply(ctx, "Not in a voice channel").await);

            return Ok(());
        }
    };

    let mut handler = handler_lock.lock().await;

    if handler.is_deaf() {
        check_msg(msg.channel_id.say(&ctx.http, "Already deafened").await);
    } else {
        if let Err(e) = handler.deafen(true).await {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, format!("Failed: {:?}", e))
                    .await,
            );
        }

        check_msg(msg.channel_id.say(&ctx.http, "Deafened").await);
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn join(ctx: &Context, msg: &Message) -> CommandResult {
    let data = ctx.data.read().await;

    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let channel_id = guild
        .voice_states
        .get(&msg.author.id)
        .and_then(|voice_state| voice_state.channel_id);

    let connect_to = match channel_id {
        Some(channel) => channel,
        None => {
            check_msg(msg.reply(ctx, "Not in a voice channel").await);

            return Ok(());
        }
    };

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    let _handler = manager.join(guild_id, connect_to).await;

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;

        let player = data.get::<SpotifyPlayerKey>();

        player
            .unwrap()
            .lock()
            .await
            .enable_connect(
                "Aoede".to_string(),
                DeviceType::AudioDongle,
                1u16,
                VolumeCtrl::default(),
            )
            .await;

        let mut decoder = input::codec::OpusDecoderState::new().unwrap();
        decoder.allow_passthrough = false;

        let source = input::Input::new(
            true,
            input::reader::Reader::Extension(Box::new(
                player.unwrap().lock().await.emitted_sink.clone(),
            )),
            input::codec::Codec::FloatPcm,
            input::Container::Raw,
            None,
        );

        // let file = std::fs::File::open("out.wav").unwrap();

        // handler.play_source(input::Input::new(
        //     true,
        //     input::reader::Reader::File(std::io::BufReader::new(file)),
        //     input::codec::Codec::FloatPcm,
        //     input::Container::Raw,
        //     None
        // ));

        // std::io::copy(&mut player.unwrap().lock().unwrap().emitted_sink.clone(), &mut file).unwrap();

        // let source = input::Input::float_pcm(true, input::reader::Reader::Extension(Box::new(source)));

        // sleep(Duration::from_millis(5000)).await;
        //     println!("Starting to play...");
        handler.set_bitrate(songbird::Bitrate::Auto);

        handler.play_source(source);

        check_msg(msg.channel_id.say(&ctx.http, "Playing song").await);
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to play in")
                .await,
        );
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn leave(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();
    let has_handler = manager.get(guild_id).is_some();

    if has_handler {
        if let Err(e) = manager.remove(guild_id).await {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, format!("Failed: {:?}", e))
                    .await,
            );
        }

        check_msg(msg.channel_id.say(&ctx.http, "Left voice channel").await);
    } else {
        check_msg(msg.reply(ctx, "Not in a voice channel").await);
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn mute(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    let handler_lock = match manager.get(guild_id) {
        Some(handler) => handler,
        None => {
            check_msg(msg.reply(ctx, "Not in a voice channel").await);

            return Ok(());
        }
    };

    let mut handler = handler_lock.lock().await;

    if handler.is_mute() {
        check_msg(msg.channel_id.say(&ctx.http, "Already muted").await);
    } else {
        if let Err(e) = handler.mute(true).await {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, format!("Failed: {:?}", e))
                    .await,
            );
        }

        check_msg(msg.channel_id.say(&ctx.http, "Now muted").await);
    }

    Ok(())
}

#[command]
async fn ping(context: &Context, msg: &Message) -> CommandResult {
    check_msg(msg.channel_id.say(&context.http, "Pong!").await);

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn play(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let url = match args.single::<String>() {
        Ok(url) => url,
        Err(_) => {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, "Must provide a URL to a video or audio")
                    .await,
            );

            return Ok(());
        }
    };

    if !url.starts_with("http") {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Must provide a valid URL")
                .await,
        );

        return Ok(());
    }

    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;

        let source = match songbird::ytdl(&url).await {
            Ok(source) => source,
            Err(why) => {
                println!("Err starting source: {:?}", why);

                check_msg(msg.channel_id.say(&ctx.http, "Error sourcing ffmpeg").await);

                return Ok(());
            }
        };

        handler.play_source(source);

        check_msg(msg.channel_id.say(&ctx.http, "Playing song").await);
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to play in")
                .await,
        );
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn undeafen(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;
        if let Err(e) = handler.deafen(false).await {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, format!("Failed: {:?}", e))
                    .await,
            );
        }

        check_msg(msg.channel_id.say(&ctx.http, "Undeafened").await);
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to undeafen in")
                .await,
        );
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn unmute(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).await.unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;
        if let Err(e) = handler.mute(false).await {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, format!("Failed: {:?}", e))
                    .await,
            );
        }

        check_msg(msg.channel_id.say(&ctx.http, "Unmuted").await);
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to unmute in")
                .await,
        );
    }

    Ok(())
}

/// Checks that a message successfully sent; if not, then logs why to stdout.
fn check_msg(result: SerenityResult<Message>) {
    if let Err(why) = result {
        println!("Error sending message: {:?}", why);
    }
}
