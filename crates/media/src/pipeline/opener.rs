//! How a pipeline gets its bytes: a function that opens a new byte source
//! at position 0 (called on the demux thread, at the first open and at
//! each reopen after a suspend), and a cancel function that makes a read
//! that blocks in the source return at once (called by the owner when the
//! pipeline closes or suspends; ADR 0047 section 5, "Cancellation").

use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use crate::{ByteSource, FileSource, MemorySource};

type OpenFn = Box<dyn FnMut() -> io::Result<Box<dyn ByteSource>> + Send>;
type CancelFn = Arc<dyn Fn() + Send + Sync>;

/// Opens the byte source of a pipeline, again after each suspend.
pub struct SourceOpener {
    open: OpenFn,
    cancel: Option<CancelFn>,
}

impl SourceOpener {
    /// An opener from an `open` function and an optional `cancel` function.
    /// `open` may block (it runs on the demux thread); after `cancel`, the
    /// next `open` must undo the cancel (a suspended pipeline reopens).
    pub fn new(
        open: impl FnMut() -> io::Result<Box<dyn ByteSource>> + Send + 'static,
        cancel: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> SourceOpener {
        SourceOpener {
            open: Box::new(open),
            cancel,
        }
    }

    /// The file at `path` (never blocks; no cancel).
    pub fn file(path: impl Into<PathBuf>) -> SourceOpener {
        let path = path.into();
        SourceOpener::new(
            move || Ok(Box::new(FileSource::open(&path)?) as Box<dyn ByteSource>),
            None,
        )
    }

    /// Bytes in memory; `seekable` false gives a stream (no size, no seek).
    pub fn memory(data: impl Into<Arc<[u8]>>, seekable: bool) -> SourceOpener {
        let data: Arc<[u8]> = data.into();
        SourceOpener::new(
            move || {
                let source = if seekable {
                    MemorySource::new(Arc::clone(&data))
                } else {
                    MemorySource::stream(Arc::clone(&data))
                };
                Ok(Box::new(source) as Box<dyn ByteSource>)
            },
            None,
        )
    }

    /// The loader's media resource (`swb_net::MediaHandle`): each open
    /// resumes the resource and reads it from position 0 with a new reader
    /// over the same cache; cancel cancels the resource (its cache stays).
    #[cfg(feature = "net")]
    pub fn net(handle: swb_net::MediaHandle) -> SourceOpener {
        let cancel_handle = handle.clone();
        SourceOpener::new(
            move || {
                handle.resume();
                Ok(Box::new(crate::NetSource::new(handle.reader())) as Box<dyn ByteSource>)
            },
            Some(Arc::new(move || cancel_handle.cancel())),
        )
    }

    pub(crate) fn open(&mut self) -> io::Result<Box<dyn ByteSource>> {
        (self.open)()
    }

    pub(crate) fn canceller(&self) -> Option<CancelFn> {
        self.cancel.clone()
    }
}
