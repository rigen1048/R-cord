use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};

pub struct Recorder {
    stop_tx: Option<Sender<()>>,
    done_rx: Option<std::sync::mpsc::Receiver<Result<PathBuf, String>>>,
}

pub type SharedRecorder = Arc<Mutex<Recorder>>;

pub fn new_shared() -> SharedRecorder {
    Arc::new(Mutex::new(Recorder {
        stop_tx: None,
        done_rx: None,
    }))
}

#[derive(Serialize)]
pub struct DeviceInfo {
    pub name: String,
    pub is_default: bool,
}

pub fn input_devices() -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|d| d.name().ok());
    let mut out = vec![];
    if let Ok(devices) = host.input_devices() {
        for d in devices {
            if let Ok(name) = d.name() {
                out.push(DeviceInfo {
                    is_default: Some(name.clone()) == default_name,
                    name,
                });
            }
        }
    }
    out
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

fn unique_wav_path(folder: &Path, base: &str) -> PathBuf {
    let base = sanitize(base);
    let base = if base.is_empty() { "take".to_string() } else { base };
    let mut p = folder.join(format!("{base}.wav"));
    let mut n = 1;
    while p.exists() {
        p = folder.join(format!("{base}-{n}.wav"));
        n += 1;
    }
    p
}

impl Recorder {
    pub fn is_recording(&self) -> bool {
        self.stop_tx.is_some()
    }

    /// Start recording into `folder` using device `device_name` (None = default).
    /// cpal Streams are !Send, so the stream is created and owned entirely
    /// inside the worker thread; startup errors come back on the ack channel.
    pub fn start(
        &mut self,
        folder: PathBuf,
        device_name: Option<String>,
    ) -> Result<(), String> {
        if self.stop_tx.is_some() {
            return Err("already recording".into());
        }
        let auto_name = format!("take-{}", chrono::Local::now().format("%Y%m%d-%H%M%S"));
        let path = unique_wav_path(&folder, &auto_name);

        let (stop_tx, stop_rx) = channel::<()>();
        let (done_tx, done_rx) = channel::<Result<PathBuf, String>>();
        let (ack_tx, ack_rx) = channel::<Result<(), String>>();

        std::thread::spawn(move || {
            let host = cpal::default_host();
            let run = || -> Result<(cpal::Stream, Arc<Mutex<hound::WavWriter<std::io::BufWriter<std::fs::File>>>>, PathBuf), String> {
                let device = match &device_name {
                    Some(name) => host
                        .input_devices()
                        .map_err(|e| e.to_string())?
                        .find(|d| d.name().map(|n| &n == name).unwrap_or(false))
                        .ok_or_else(|| format!("input device '{name}' not found"))?,
                    None => host
                        .default_input_device()
                        .ok_or("no default input device")?,
                };
                let supported = device
                    .default_input_config()
                    .map_err(|e| format!("no default input config: {e}"))?;
                let sample_format = supported.sample_format();
                let config: cpal::StreamConfig = supported.clone().into();
                let spec = hound::WavSpec {
                    channels: config.channels,
                    sample_rate: config.sample_rate.0,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                };
                let writer = hound::WavWriter::create(&path, spec)
                    .map_err(|e| format!("cannot create {}: {e}", path.display()))?;
                let writer = Arc::new(Mutex::new(writer));
                let w = writer.clone();
                let err = |e| eprintln!("stream error: {e}");

                macro_rules! build {
                    ($ty:ty, $conv:expr) => {
                        device.build_input_stream(
                            &config,
                            move |data: &[$ty], _: &cpal::InputCallbackInfo| {
                                let mut wr = w.lock().unwrap();
                                let f: fn($ty) -> i16 = $conv;
                                for &s in data {
                                    let _ = wr.write_sample(f(s));
                                }
                            },
                            err,
                            None,
                        )
                    };
                }

                let stream = match sample_format {
                    cpal::SampleFormat::I16 => build!(i16, |s| s),
                    cpal::SampleFormat::U16 => {
                        build!(u16, |s: u16| (s as i32 - 32768) as i16)
                    }
                    cpal::SampleFormat::F32 => {
                        build!(f32, |s: f32| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
                    }
                    f => return Err(format!("unsupported sample format: {f}")),
                }
                .map_err(|e| format!("failed to build input stream: {e}"))?;
                stream
                    .play()
                    .map_err(|e| format!("failed to start stream: {e}"))?;
                Ok((stream, writer, path))
            };

            match run() {
                Ok((stream, writer, path)) => {
                    let _ = ack_tx.send(Ok(()));
                    let _ = stop_rx.recv();
                    drop(stream);
                    let res = writer
                        .lock()
                        .unwrap()
                        .flush()
                        .map_err(|e| e.to_string())
                        .map(|_| path);
                    let _ = done_tx.send(res);
                }
                Err(e) => {
                    let _ = ack_tx.send(Err(e));
                }
            }
        });

        // Wait for the worker to report stream startup (or failure).
        ack_rx
            .recv()
            .map_err(|_| "recorder thread died".to_string())??;

        self.stop_tx = Some(stop_tx);
        self.done_rx = Some(done_rx);
        Ok(())
    }

    /// Stop recording, finalize the WAV, return its path.
    pub fn stop(&mut self) -> Result<PathBuf, String> {
        let tx = self.stop_tx.take().ok_or("not recording")?;
        let rx = self.done_rx.take().ok_or("not recording")?;
        let _ = tx.send(());
        rx.recv()
            .map_err(|_| "recorder thread died".to_string())?
    }

    pub fn abort(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
            self.done_rx.take();
        }
    }
}

pub fn wav_duration_secs(path: &Path) -> Option<f64> {
    let r = hound::WavReader::open(path).ok()?;
    let spec = r.spec();
    let frames = r.duration() as f64 / spec.channels as f64;
    Some(frames / spec.sample_rate as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rcord-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn sanitize_strips_unsafe_chars() {
        assert_eq!(sanitize("my take #1 / best!"), "my take _1 _ best_");
        assert_eq!(sanitize("  spaced  "), "spaced");
    }

    #[test]
    fn unique_path_avoids_collisions() {
        let dir = tmpdir("uniq");
        let p1 = unique_wav_path(&dir, "take");
        std::fs::write(&p1, b"x").unwrap();
        let p2 = unique_wav_path(&dir, "take");
        assert_ne!(p1, p2);
        assert!(p2.to_string_lossy().ends_with("take-1.wav"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wav_roundtrip_and_duration() {
        let dir = tmpdir("wav");
        let path = dir.join("t.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        for _ in 0..8000 {
            w.write_sample(0i16).unwrap();
        }
        w.flush().unwrap();
        let d = wav_duration_secs(&path).unwrap();
        assert!((d - 1.0).abs() < 0.01, "expected ~1s, got {d}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
