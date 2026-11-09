use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Minimal rodio playback: one take at a time.
pub struct Player {
    sink: Option<rodio::Sink>,
    // Keep stream alive while playing.
    _stream: Option<rodio::OutputStream>,
}

pub type SharedPlayer = Arc<Mutex<Player>>;

pub fn new_shared() -> SharedPlayer {
    Arc::new(Mutex::new(Player {
        sink: None,
        _stream: None,
    }))
}

impl Player {
    pub fn play(&mut self, path: PathBuf) -> Result<(), String> {
        self.stop();
        let stream = rodio::OutputStreamBuilder::open_default_stream()
            .map_err(|e| format!("no output device: {e}"))?;
        let sink = rodio::Sink::connect_new(stream.mixer());
        let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
        let source = rodio::Decoder::new(std::io::BufReader::new(file))
            .map_err(|e| format!("cannot decode: {e}"))?;
        sink.append(source);
        self.sink = Some(sink);
        self._stream = Some(stream);
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(sink) = self.sink.take() {
            sink.stop();
        }
        self._stream = None;
    }
}
