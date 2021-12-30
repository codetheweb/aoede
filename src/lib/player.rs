use librespot::connect::spirc::Spirc;
use librespot::core::{
    authentication::Credentials,
    cache::Cache,
    config::{ConnectConfig, DeviceType, SessionConfig},
    session::Session,
};
use librespot::playback::{
    audio_backend,
    audio_backend::SinkResult,
    config::Bitrate,
    config::{PlayerConfig, VolumeCtrl},
    convert::Converter,
    decoder::AudioPacket,
    mixer::softmixer::SoftMixer,
    mixer::{Mixer, MixerConfig},
    player::{Player, PlayerEventChannel},
};

use serenity::prelude::TypeMapKey;

use std::clone::Clone;
use std::sync::{
    mpsc::{sync_channel, Receiver, SyncSender},
    Arc, Mutex,
};
use std::{io, mem};

use byteorder::{ByteOrder, LittleEndian};
use rubato::{FftFixedInOut, Resampler};
use songbird::input::reader::MediaSource;

pub struct SpotifyPlayer {
    player_config: PlayerConfig,
    pub emitted_sink: EmittedSink,
    pub session: Session,
    pub spirc: Option<Box<Spirc>>,
    pub event_channel: Option<Arc<tokio::sync::Mutex<PlayerEventChannel>>>,
}

pub struct EmittedSink {
    sender: Arc<SyncSender<[f32; 2]>>,
    pub receiver: Arc<Mutex<Receiver<[f32; 2]>>>,
    input_buffer: Arc<Mutex<(Vec<f32>, Vec<f32>)>>,
    resampler: Arc<Mutex<FftFixedInOut<f32>>>,
    resampler_input_frames_needed: usize,
}

impl EmittedSink {
    fn new() -> EmittedSink {
        let (sender, receiver) = sync_channel::<[f32; 2]>((1024f32 * 1.2).ceil() as usize);

        let resampler = FftFixedInOut::<f32>::new(
            librespot::playback::SAMPLE_RATE as usize,
            songbird::constants::SAMPLE_RATE_RAW,
            1024,
            2,
        );

        let resampler_input_frames_needed = resampler.nbr_frames_needed();

        EmittedSink {
            sender: Arc::new(sender),
            receiver: Arc::new(Mutex::new(receiver)),
            input_buffer: Arc::new(Mutex::new((
                Vec::with_capacity(resampler_input_frames_needed),
                Vec::with_capacity(resampler_input_frames_needed),
            ))),
            resampler: Arc::new(Mutex::new(resampler)),
            resampler_input_frames_needed,
        }
    }
}

impl audio_backend::Sink for EmittedSink {
    fn start(&mut self) -> SinkResult<()> {
        Ok(())
    }

    fn stop(&mut self) -> SinkResult<()> {
        Ok(())
    }

    fn write(&mut self, packet: &AudioPacket, _converter: &mut Converter) -> SinkResult<()> {
        let frames_needed = self.resampler_input_frames_needed;
        let mut input_buffer = self.input_buffer.lock().unwrap();

        for c in packet.samples().unwrap().chunks_exact(2) {
            input_buffer.0.push(c[0] as f32);
            input_buffer.1.push(c[1] as f32);
            if input_buffer.0.len() == frames_needed {
                let mut resampler = self.resampler.lock().unwrap();
                let resampled = resampler
                    .process(&[
                        &input_buffer.0[0..frames_needed],
                        &input_buffer.1[0..frames_needed],
                    ])
                    .unwrap();

                input_buffer.0.clear();
                input_buffer.1.clear();

                let sender = self.sender.clone();

                for i in 0..resampled[0].len() {
                    sender.send([resampled[0][i], resampled[1][i]]).unwrap()
                }
            }
        }

        Ok(())
    }
}

impl io::Read for EmittedSink {
    fn read(&mut self, buff: &mut [u8]) -> io::Result<usize> {
        let receiver = self.receiver.lock().unwrap();

        let chunk_size = mem::size_of::<f32>() * 2;
        let mut bytes_written = 0;
        while bytes_written + (chunk_size - 1) < buff.len() {
            if bytes_written == 0 {
                // We can not return 0 bytes because songbird then things that the track has ended,
                // therefore block until at least one stereo data set can be returned.

                let chunk = receiver.recv().unwrap();
                LittleEndian::write_f32_into(
                    &chunk,
                    &mut buff[bytes_written..(bytes_written + chunk_size)],
                );
            } else if let Ok(data) = receiver.try_recv() {
                LittleEndian::write_f32_into(
                    &data,
                    &mut buff[bytes_written..(bytes_written + chunk_size)],
                );
            } else {
                break;
            }
            bytes_written += chunk_size;
        }

        Ok(bytes_written)
    }
}

impl io::Seek for EmittedSink {
    fn seek(&mut self, _pos: io::SeekFrom) -> io::Result<u64> {
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
            resampler_input_frames_needed: self.resampler_input_frames_needed,
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
            .await
            .expect("Error creating session");

        let player_config = PlayerConfig {
            bitrate: quality,
            ..Default::default()
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
            initial_volume: None,
            has_volume_ctrl: true,
            autoplay: true,
        };

        let mixer = Box::new(SoftMixer::open(MixerConfig {
            volume_ctrl: VolumeCtrl::Linear,
            ..MixerConfig::default()
        }));

        let cloned_sink = self.emitted_sink.clone();

        let (player, player_events) = Player::new(
            self.player_config.clone(),
            self.session.clone(),
            mixer.get_audio_filter(),
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
