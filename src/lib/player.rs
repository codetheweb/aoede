use librespot::playback::decoder::AudioPacket;
use librespot::connect::spirc::Spirc;
use librespot::core::{
    authentication::Credentials,
    cache::Cache,
    config::{ConnectConfig, DeviceType, SessionConfig},
    session::Session,
};
use librespot::playback::{
    audio_backend,
    config::Bitrate,
    config::PlayerConfig,
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
use librespot::playback::audio_backend::SinkError;
use librespot::playback::convert::Converter;
use songbird::input::reader::MediaSource;
use std::io::SeekFrom;
use rubato::{Resampler, FftFixedInOut};

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
    input_buffer: Arc<Mutex<(Vec<f32>, Vec<f32>)>>,
    resampler: Arc<Mutex<FftFixedInOut<f32>>>,
}

impl EmittedSink {
    fn new() -> EmittedSink {
        let (sender, receiver) = sync_channel::<u8>(64);

        EmittedSink {
            sender: Arc::new(sender),
            receiver: Arc::new(Mutex::new(receiver)),
            input_buffer: Arc::new(Mutex::new((Vec::new(), Vec::new()))),
            resampler: Arc::new(Mutex::new(FftFixedInOut::<f32>::new(
                44100,
                songbird::constants::SAMPLE_RATE_RAW,
                2048,
                2,
            ))),
        }
    }
}

struct ImpliedMixer {}

impl Mixer for ImpliedMixer {
    fn open(_config: MixerConfig) -> ImpliedMixer {
        ImpliedMixer {}
    }

    fn set_volume(&self, _volume: u16) {}

    fn volume(&self) -> u16 {
        50
    }

    fn get_audio_filter(&self) -> Option<Box<dyn AudioFilter + Send>> {
        None
    }
}

impl audio_backend::Sink for EmittedSink {
    fn start(&mut self) -> std::result::Result<(), SinkError> {
        Ok(())
    }

    fn stop(&mut self) -> std::result::Result<(), SinkError> {
        Ok(())
    }

    fn write(&mut self, packet: &AudioPacket, _converter: &mut Converter) -> std::result::Result<(), SinkError> {
        let mut input_buffer = self.input_buffer.lock().unwrap();
        for c in packet.samples().unwrap().chunks_exact(2) {
            input_buffer.0.push(c[0] as f32);
            input_buffer.1.push(c[1] as f32);
        }

        let mut resampler = self.resampler.lock().unwrap();

        while input_buffer.0.len() >= resampler.nbr_frames_needed() {
            let frames = resampler.nbr_frames_needed();

            let resampled = resampler.process(
                &[&input_buffer.0[0..frames], &input_buffer.1[0..frames]]
            ).unwrap();

            input_buffer.0.drain(0..frames);
            input_buffer.1.drain(0..frames);

            let sender = self.sender.clone();

            for i in 0..resampled[0].len() {
                let mut data = [0u8; 8];
                LittleEndian::write_f32_into(&[resampled[0][i], resampled[1][i]], &mut data);

                for b in data {
                    sender.send(b).unwrap();
                }
            }
        }

        Ok(())
    }
}

impl std::io::Read for EmittedSink {
    fn read(&mut self, buff: &mut [u8]) -> Result<usize, io::Error> {
        let receiver = self.receiver.lock().unwrap();

        #[allow(clippy::needless_range_loop)]
        for i in 0..buff.len() {
            buff[i] = receiver.recv().unwrap();
        }

        Ok(buff.len())
    }
}

impl std::io::Seek for EmittedSink {
    fn seek(&mut self, _pos: SeekFrom) -> std::io::Result<u64> {
        unreachable!()
    }
}

impl MediaSource for EmittedSink {
    fn is_seekable(&self) -> bool {
        false
    }

    fn len(&self) -> Option<u64> {
        None
    }
}

impl Clone for EmittedSink {
    fn clone(&self) -> EmittedSink {
        EmittedSink {
            receiver: self.receiver.clone(),
            sender: self.sender.clone(),
            input_buffer: self.input_buffer.clone(),
            resampler: self.resampler.clone(),
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

        // 4 GB
        let mut cache_limit: u64 = 10;
        cache_limit = cache_limit.pow(9);
        cache_limit *= 4;

        let cache = Cache::new(cache_dir.clone(), cache_dir, Some(cache_limit)).ok();

        let session = Session::connect(session_config, credentials, cache)
            .await.expect("Error creating session");

        let mut player_config = PlayerConfig::default();
        player_config.bitrate = quality;

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
            initial_volume: Some(std::u16::MAX / 2),
            has_volume_ctrl: false,
            autoplay: true,
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

    pub async fn disable_connect(&mut self) {
        if let Some(spirc) = self.spirc.as_ref() {
            spirc.shutdown();

            self.event_channel.as_ref().unwrap().lock().await.close();
        }
    }
}
