use librespot::audio::AudioPacket;
use librespot::connect::spirc::Spirc;
use librespot::core::{
    authentication::Credentials,
    cache::Cache,
    config::{ConnectConfig, DeviceType, SessionConfig, VolumeCtrl},
    session::Session,
};
use librespot::playback::{
    audio_backend,
    config::Bitrate,
    config::PlayerConfig,
    config::{NormalisationMethod, NormalisationType},
    mixer::{AudioFilter, Mixer, MixerConfig},
    player::{Player, PlayerEventChannel},
};

use serenity::prelude::TypeMapKey;

use std::clone::Clone;
use std::io;
use std::sync::{
    mpsc::{sync_channel, Receiver, SyncSender},
    Arc, Mutex,
};

use byteorder::{ByteOrder, LittleEndian};

pub struct SpotifyPlayer {
    player_config: PlayerConfig,
    pub emitted_sink: EmittedSink,
    pub session: Session,
    pub spirc: Option<Box<Spirc>>,
    pub event_channel: Option<Arc<tokio::sync::Mutex<PlayerEventChannel>>>,
}

pub struct EmittedSink {
    sender: Arc<SyncSender<u8>>,
    pub receiver: Arc<Mutex<Receiver<u8>>>,
}

impl EmittedSink {
    fn new() -> EmittedSink {
        let (sender, receiver) = sync_channel::<u8>(64);

        EmittedSink {
            sender: Arc::new(sender),
            receiver: Arc::new(Mutex::new(receiver)),
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

    fn set_volume(&self, _volume: u16) {}

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
        let resampled = samplerate::convert(
            44100,
            48000,
            2,
            samplerate::ConverterType::Linear,
            packet.samples(),
        )
        .unwrap();

        let sender = self.sender.clone();

        for i in resampled {
            let mut new = [0, 0, 0, 0];

            LittleEndian::write_f32_into(&[i], &mut new);

            for j in new.iter() {
                sender.send(*j).unwrap();
            }
        }

        Ok(())
    }
}

impl io::Read for EmittedSink {
    fn read(&mut self, buff: &mut [u8]) -> Result<usize, io::Error> {
        let receiver = self.receiver.lock().unwrap();

        #[allow(clippy::needless_range_loop)]
        for i in 0..buff.len() {
            buff[i] = receiver.recv().unwrap();
        }

        Ok(buff.len())
    }
}

impl Clone for EmittedSink {
    fn clone(&self) -> EmittedSink {
        EmittedSink {
            receiver: self.receiver.clone(),
            sender: self.sender.clone(),
        }
    }
}

pub struct SpotifyPlayerKey;
impl TypeMapKey for SpotifyPlayerKey {
    type Value = Arc<tokio::sync::Mutex<SpotifyPlayer>>;
}

impl SpotifyPlayer {
    pub async fn new(
        username: String,
        password: String,
        quality: Bitrate,
        cache_dir: Option<String>,
    ) -> SpotifyPlayer {
        let credentials = Credentials::with_password(username, password);

        let session_config = SessionConfig::default();

        let mut cache: Option<Cache> = None;

        if let Ok(c) = Cache::new(cache_dir.clone(), cache_dir) {
            cache = Some(c);
        }

        let session = Session::connect(session_config, credentials, cache)
            .await
            .expect("Error creating session");

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
            passthrough: false,
        };

        let emitted_sink = EmittedSink::new();

        let cloned_sink = emitted_sink.clone();

        let (_player, rx) = Player::new(player_config.clone(), session.clone(), None, move || {
            Box::new(cloned_sink)
        });

        SpotifyPlayer {
            player_config,
            emitted_sink,
            session,
            spirc: None,
            event_channel: Some(Arc::new(tokio::sync::Mutex::new(rx))),
        }
    }

    pub async fn enable_connect(&mut self) {
        let config = ConnectConfig {
            name: "Aoede".to_string(),
            device_type: DeviceType::AudioDongle,
            volume: std::u16::MAX / 2,
            autoplay: true,
            volume_ctrl: VolumeCtrl::default(),
        };

        let mixer = Box::new(ImpliedMixer {});

        let cloned_sink = self.emitted_sink.clone();

        let (player, player_events) = Player::new(
            self.player_config.clone(),
            self.session.clone(),
            None,
            move || Box::new(cloned_sink),
        );

        let cloned_session = self.session.clone();

        let (spirc, task) = Spirc::new(config, cloned_session, player, mixer);

        let handle = tokio::runtime::Handle::current();
        handle.spawn(async {
            task.await;
        });

        self.spirc = Some(Box::new(spirc));

        let mut channel_lock = self.event_channel.as_ref().unwrap().lock().await;

        *channel_lock = player_events;
    }

    pub fn disable_connect(&mut self) {
        if let Some(spirc) = self.spirc.as_ref() {
            spirc.shutdown();
        }
    }
}
