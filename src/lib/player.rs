use futures_util::{future, FutureExt, StreamExt};

use futures::{ Future, Stream };
use std::{ thread, pin::Pin };
use std::sync::{ Mutex, Arc };
use std::sync::mpsc;
use futures::channel::oneshot;

use librespot::core::authentication::Credentials;
use librespot::core::config::{DeviceType, SessionConfig, ConnectConfig, VolumeCtrl};
use librespot::core::session::Session;

use librespot::core::spotify_id::SpotifyId;
use librespot::core::keymaster;
use librespot::core::keymaster::Token;
use librespot::audio::AudioPacket;
use librespot::playback::config::PlayerConfig;
use librespot::playback::config::Bitrate;
use librespot::playback::config::{NormalisationType, NormalisationMethod};
use librespot::playback::audio_backend;
use librespot::playback::player::Player;
use librespot::playback::mixer::{Mixer, AudioFilter, MixerConfig};
use librespot::core::cache::Cache;
use librespot::connect::spirc::{Spirc};
use librespot::connect::discovery::discovery;
use std::path::PathBuf;
use std::clone::Clone;
use std::io;
use std::vec::Vec;
use std::convert::TryInto;
use std::cmp;
use serenity::prelude::TypeMapKey;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, Sender, channel};

use futures::executor::block_on;
use tokio::time::{Instant, Duration, sleep};

use byteorder::LittleEndian;
use byteorder::ByteOrder;

pub struct SpotifyPlayer {
    // player: Player,
    player_config: PlayerConfig,
    pub emitted_sink: EmittedSink,
    session: Session,
    pub spirc: Option<Box<Spirc>>,
    remote: oneshot::Receiver<Spirc>
}

pub struct EmittedSink {
    sender: Arc<Mutex<SyncSender<u8>>>,
    pub receiver: Arc<Mutex<Receiver<u8>>>
}

impl EmittedSink {
    fn new() -> EmittedSink {
        let (sender, receiver) = sync_channel::<u8>(16);

        EmittedSink {
            sender: Arc::new(Mutex::new(sender)),
            receiver: Arc::new(Mutex::new(receiver))
        }
    }
}

struct ImpliedMixer {}

impl Mixer for ImpliedMixer {
    fn open(_config: Option<MixerConfig>) -> ImpliedMixer {
        ImpliedMixer {}
    }

    fn start(&self) {}

    fn stop(&self) {}

    fn volume(&self) -> u16 {
        50
    }

    fn set_volume(&self, volume: u16) {

    }

    fn get_audio_filter(&self) -> Option<Box<dyn AudioFilter + Send>> {
        None
    }
}

impl audio_backend::Sink for EmittedSink {
    fn start(&mut self) -> std::result::Result<(), std::io::Error> {
        Ok(())
    }

    fn stop(&mut self) -> std::result::Result<(), std::io::Error> {
        Ok(())
    }

    fn write(&mut self, packet: &AudioPacket) -> std::result::Result<(), std::io::Error> {
            let resampled = samplerate::convert(44100, 48000, 2, samplerate::ConverterType::Linear, packet.samples()).unwrap();

            let sender = self.sender.lock().unwrap();

            for i in resampled {
                let mut new = [0, 0, 0, 0];

                LittleEndian::write_f32_into(&[i], &mut new);

                for j in 0..new.len() {
                    sender.send(new[j]).unwrap();
                }
            }
        // }

        // let sender = self.sender.lock().unwrap();

        // for b in packet.oggdata() {
        //     sender.send(*b).unwrap();
        // }

        Ok(())
    }
}

impl io::Read for EmittedSink {
    fn read(&mut self, buff: &mut [u8]) -> Result<usize, io::Error> {
        let receiver = self.receiver.lock().unwrap();

        for i in 0..buff.len() {
            buff[i] = receiver.recv().unwrap();
        }

        return Ok(buff.len());

        // for i in {
        //     buff[i] = self.receiver.lock().unwrap().recv().unwrap();
        // }

        // return Ok(4);
    }
}

impl Clone for EmittedSink {
    fn clone(&self) -> EmittedSink {
        EmittedSink {
            receiver: self.receiver.clone(),
            sender: self.sender.clone()
        }
    }
}

// impl Clone for EmittedSink {
//     fn clone(&self) -> EmittedSink {
//         EmittedSink {
//             data: self.data.clone()
//         }
//     }
// }


pub struct SpotifyPlayerKey;
impl TypeMapKey for SpotifyPlayerKey {
    type Value = Arc<Mutex<SpotifyPlayer>>;
}

impl Drop for SpotifyPlayer {
    fn drop(&mut self) {
        println!("dropping player");
    }
}

impl SpotifyPlayer {
    pub async fn new(username: String, password: String, quality: Bitrate, cache_dir: String) -> SpotifyPlayer {
        let credentials = Credentials::with_password(username, password);

        let session_config = SessionConfig::default();

        let session = Session::connect(
            session_config,
            credentials,
            None).await.expect("Error creating session");

        let player_config = PlayerConfig {
            bitrate: quality,
            normalisation: false,
            normalisation_type: NormalisationType::default(),
            normalisation_method: NormalisationMethod::default(),
            normalisation_pregain: 0.0,
            normalisation_threshold: -1.0,
            normalisation_attack: 0.005,
            normalisation_release: 0.1,
            normalisation_knee: 1.0,
            gapless: true,
            passthrough: false
        };

        let emitted_sink = EmittedSink::new();

        // let cloned_sink = emitted_sink.clone();

        // let (player, rx) = Player::new(player_config.clone(), session.clone(), None, move || Box::new(cloned_sink));

        let (remote_tx, remote_rx) = oneshot::channel::<Spirc>();

        SpotifyPlayer {
            // player: player,
            player_config,
            emitted_sink,
            session: session,
            spirc: None,
            remote: remote_rx
        }
    }

    pub fn enable_connect(&mut self, device_name: String, device_type: DeviceType, initial_volume: u16, volume_ctrl: VolumeCtrl) {
        let config = ConnectConfig {
            name: device_name,
            device_type,
            volume: initial_volume,
            autoplay: true,
            volume_ctrl
        };

        let mixer = Box::new(ImpliedMixer {});

        let cloned_sink = self.emitted_sink.clone();

        let (player, _) = Player::new(self.player_config.clone(), self.session.clone(), None, move ||Box::new(cloned_sink));

        let cloned_config = config.clone();
        let cloned_session = self.session.clone();

        let (spirc, task) = Spirc::new(cloned_config, cloned_session, player, mixer);

        // thread::spawn(|| task);
        let handle = tokio::runtime::Handle::current();
        handle.spawn(async {
            task.await;
        });

        self.spirc = Some(Box::new(spirc));

        // discovery(&self.handle, config.clone(), self.session.device_id().clone().to_string(), 0).expect("started discovery process");
    }

}
