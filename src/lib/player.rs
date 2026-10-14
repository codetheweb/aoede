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
    mixer: Box<SoftMixer>,
    pub bot_autoplay: bool,
    pub device_name: String,
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
        // By setting the sync_channel bound to at least the output frame size of one resampling
        // step (1120 for a chunk size of 1024 and our frequency settings) the number of
        // synchronizations needed between EmittedSink::write and EmittedSink::read can be reduced.
        let (sender, receiver) = sync_channel::<[f32; 2]>(1120);

        let resampler = FftFixedInOut::<f32>::new(
            librespot::playback::SAMPLE_RATE as usize,
            songbird::constants::SAMPLE_RATE_RAW,
            1024,
            2,
        )
        .unwrap();

        let resampler_input_frames_needed = resampler.input_frames_max();

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

    fn write(&mut self, packet: AudioPacket, _converter: &mut Converter) -> SinkResult<()> {
        let frames_needed = self.resampler_input_frames_needed;
        let mut input_buffer = self.input_buffer.lock().unwrap();

        let mut resampler = self.resampler.lock().unwrap();

        let mut resampled_buffer = resampler.output_buffer_allocate();

        for c in packet.samples().unwrap().chunks_exact(2) {
            input_buffer.0.push(c[0] as f32);
            input_buffer.1.push(c[1] as f32);
            if input_buffer.0.len() == frames_needed {
                resampler
                    .process_into_buffer(
                        &[
                            &input_buffer.0[0..frames_needed],
                            &input_buffer.1[0..frames_needed],
                        ],
                        &mut resampled_buffer,
                        None,
                    )
                    .unwrap();

                input_buffer.0.clear();
                input_buffer.1.clear();

                let sender = self.sender.clone();

                for i in 0..resampled_buffer[0].len() {
                    sender
                        .send([resampled_buffer[0][i], resampled_buffer[1][i]])
                        .unwrap()
                }
            }
        }

        Ok(())
    }
}

impl io::Read for EmittedSink {
    fn read(&mut self, buff: &mut [u8]) -> io::Result<usize> {
        let sample_size = mem::size_of::<f32>() * 2;

        if buff.len() < sample_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "EmittedSink does not support read buffer too small to guarantee \
                holding one audio sample (8 bytes)",
            ));
        }

        let receiver = self.receiver.lock().unwrap();

        let mut bytes_written = 0;
        while bytes_written + (sample_size - 1) < buff.len() {
            if bytes_written == 0 {
                // We can not return 0 bytes because songbird then thinks that the track has ended,
                // therefore block until at least one stereo data set can be returned.

                let sample = receiver.recv().unwrap();
                LittleEndian::write_f32_into(
                    &sample,
                    &mut buff[bytes_written..(bytes_written + sample_size)],
                );
            } else if let Ok(data) = receiver.try_recv() {
                LittleEndian::write_f32_into(
                    &data,
                    &mut buff[bytes_written..(bytes_written + sample_size)],
                );
            } else {
                break;
            }
            bytes_written += sample_size;
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

    fn byte_len(&self) -> Option<u64> {
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
        bot_autoplay: bool,
        device_name: String,
    ) -> SpotifyPlayer {
        let session_config = SessionConfig::default();

        // 4 GB
        let mut cache_limit: u64 = 10;
        cache_limit = cache_limit.pow(9);
        cache_limit *= 4;

        let cache = Cache::new(
            cache_dir.clone(),
            cache_dir.clone(),
            cache_dir,
            Some(cache_limit),
        )
        .ok();

        // Try to load credentials from cache first
        let credentials = if let Some(ref cache) = cache {
            match cache.credentials() {
                Some(cached_creds) => {
                    println!("Using cached credentials");
                    cached_creds
                }
                None => {
                    println!("No cached credentials found, trying username/password");
                    Credentials::with_password(username, password)
                }
            }
        } else {
            println!("No cache available, using username/password");
            Credentials::with_password(username, password)
        };

        let (session, _) = Session::connect(session_config, credentials, cache, false)
            .await
            .expect("Error creating session");

        let player_config = PlayerConfig {
            bitrate: quality,
            ..Default::default()
        };

        let emitted_sink = EmittedSink::new();

        let cloned_sink = emitted_sink.clone();

        let mixer = Box::new(SoftMixer::open(MixerConfig {
            volume_ctrl: VolumeCtrl::Linear,
            ..MixerConfig::default()
        }));

        let (_player, rx) = Player::new(
            player_config.clone(),
            session.clone(),
            mixer.get_soft_volume(),
            move || Box::new(cloned_sink),
        );

        SpotifyPlayer {
            player_config,
            emitted_sink,
            session,
            spirc: None,
            event_channel: Some(Arc::new(tokio::sync::Mutex::new(rx))),
            mixer,
            bot_autoplay,
            device_name,
        }
    }

    pub async fn enable_connect(&mut self) {
        let config = ConnectConfig {
            name: self.device_name.clone(),
            device_type: DeviceType::AudioDongle,
            initial_volume: None,
            has_volume_ctrl: true,
            autoplay: self.bot_autoplay,
        };

        let cloned_sink = self.emitted_sink.clone();

        let (player, player_events) = Player::new(
            self.player_config.clone(),
            self.session.clone(),
            self.mixer.get_soft_volume(),
            move || Box::new(cloned_sink),
        );

        let cloned_session = self.session.clone();

        let (spirc, task) = Spirc::new(config, cloned_session, player, self.mixer.clone());

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
