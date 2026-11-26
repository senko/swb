//! The PipeWire device (feature `pipewire`; ADR 0047 section 8): one
//! stream of `swb-media-ffi`, whose real-time callback calls the mixer.

use std::sync::Arc;

use swb_media_ffi::pipewire::{AudioStream, StreamState};

use super::control::LATENCY_FRAMES;
use super::{Backend, Device, DeviceState, Sink};

/// The backend that opens a PipeWire stream. It connects to the server that
/// the environment names (`PIPEWIRE_RUNTIME_DIR`, `XDG_RUNTIME_DIR`).
pub(super) struct PipeWire;

struct PipeWireDevice {
    stream: AudioStream,
}

impl Device for PipeWireDevice {
    fn set_active(&mut self, active: bool) {
        self.stream.set_active(active);
    }
}

fn device_state(state: StreamState) -> DeviceState {
    match state {
        StreamState::Unconnected => DeviceState::Unconnected,
        StreamState::Connecting => DeviceState::Connecting,
        StreamState::Paused => DeviceState::Paused,
        StreamState::Streaming => DeviceState::Streaming,
        StreamState::Error => DeviceState::Error,
    }
}

impl Backend for PipeWire {
    fn connect(&self, sink: Arc<dyn Sink>) -> Result<Box<dyn Device>, String> {
        let fill_sink = Arc::clone(&sink);
        let stream = AudioStream::open(
            LATENCY_FRAMES,
            Box::new(move |out, delay| fill_sink.fill(out, delay)),
            Box::new(move |state| sink.state(device_state(state))),
        )
        .map_err(|e| e.to_string())?;
        Ok(Box::new(PipeWireDevice { stream }))
    }
}
