//! A fake audio device for tests (no PipeWire, no sound): the test drives it
//! by hand. It pulls N frames when told ([`FakeDevice::pull`]: one cycle of
//! the device's real-time callback, with a delay that the test gives) and
//! reports the stream's states ([`FakeDevice::report`]). By default it
//! reports `Streaming` as soon as it is opened and started, like a working
//! server; a test turns that off to drive the states itself.
//!
//! Not for the product: `swb` never builds an output over it. It is public
//! so that the tests of other crates (the engine) can use it.

use std::sync::{Arc, Mutex, PoisonError};

use super::{Backend, Device, DeviceState, Sink};

struct Shared {
    sink: Option<Arc<dyn Sink>>,
    active: bool,
    open: bool,
    connects: u64,
    closes: u64,
    refuse: Option<String>,
    auto_stream: bool,
}

/// The fake backend and the test's handle on its device. Clones share the
/// state: give one to the [`super::Output`] and keep one.
#[derive(Clone)]
pub struct FakeDevice {
    shared: Arc<Mutex<Shared>>,
}

impl Default for FakeDevice {
    fn default() -> FakeDevice {
        FakeDevice::new()
    }
}

impl FakeDevice {
    /// A fake that streams as soon as it is opened.
    pub fn new() -> FakeDevice {
        FakeDevice {
            shared: Arc::new(Mutex::new(Shared {
                sink: None,
                active: false,
                open: false,
                connects: 0,
                closes: 0,
                refuse: None,
                auto_stream: true,
            })),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The next connects fail with `why` (no server); `None` lets them work.
    pub fn refuse(&self, why: Option<&str>) {
        self.lock().refuse = why.map(str::to_owned);
    }

    /// Whether the device reports `Streaming` by itself when it is opened
    /// or started (and `Paused` when it is stopped).
    pub fn auto_stream(&self, on: bool) {
        self.lock().auto_stream = on;
    }

    /// Reports a state of the stream to the output.
    pub fn report(&self, state: DeviceState) {
        let sink = self.lock().sink.clone();
        if let Some(sink) = sink {
            sink.state(state);
        }
    }

    /// One cycle of the real-time callback: `frames` frames with the delay
    /// `delay_frames`. Returns the buffer (interleaved stereo), or `None`
    /// when the device is closed or stopped (no callback runs then).
    pub fn pull(&self, frames: usize, delay_frames: i64) -> Option<Vec<f32>> {
        let sink = {
            let shared = self.lock();
            if !shared.active {
                return None;
            }
            shared.sink.clone()?
        };
        let mut out = vec![0.0f32; frames * 2];
        sink.fill(&mut out, delay_frames);
        Some(out)
    }

    /// Whether a device is open.
    pub fn is_open(&self) -> bool {
        self.lock().open
    }

    /// Whether the device runs its cycles.
    pub fn is_active(&self) -> bool {
        let shared = self.lock();
        shared.open && shared.active
    }

    /// Devices opened so far.
    pub fn connects(&self) -> u64 {
        self.lock().connects
    }

    /// Devices closed so far.
    pub fn closes(&self) -> u64 {
        self.lock().closes
    }
}

impl Backend for FakeDevice {
    fn connect(&self, sink: Arc<dyn Sink>) -> Result<Box<dyn Device>, String> {
        let auto = {
            let mut shared = self.lock();
            if let Some(why) = &shared.refuse {
                return Err(why.clone());
            }
            shared.sink = Some(Arc::clone(&sink));
            shared.open = true;
            shared.active = true;
            shared.connects += 1;
            shared.auto_stream
        };
        if auto {
            sink.state(DeviceState::Streaming);
        } else {
            sink.state(DeviceState::Connecting);
        }
        Ok(Box::new(FakeHandle {
            device: self.clone(),
        }))
    }
}

/// The device that the output holds.
struct FakeHandle {
    device: FakeDevice,
}

impl Device for FakeHandle {
    fn set_active(&mut self, active: bool) {
        let (sink, auto) = {
            let mut shared = self.device.lock();
            shared.active = active;
            (shared.sink.clone(), shared.auto_stream)
        };
        if let (Some(sink), true) = (sink, auto) {
            sink.state(if active {
                DeviceState::Streaming
            } else {
                DeviceState::Paused
            });
        }
    }
}

impl Drop for FakeHandle {
    fn drop(&mut self) {
        let mut shared = self.device.lock();
        shared.open = false;
        shared.active = false;
        shared.closes += 1;
        // The sink holds the output's state: do not keep it alive.
        shared.sink = None;
    }
}
