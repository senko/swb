//! Byte sources: where a media file's bytes come from (ADR 0047 section 5).
//!
//! The demuxer reads a source with blocking calls on its thread. This
//! module has the file and the in-memory source; the network source
//! (`Range` requests through the loader) comes with M14 feature 4.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

/// The bytes of a media resource, read by the demuxer's thread.
pub trait ByteSource: Send {
    /// Reads up to `buf.len()` bytes at the current position; `Ok(0)` at
    /// the end. May block; fails when the source fails or is cancelled.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>;

    /// Moves to the absolute byte position `pos` and returns it. Fails for
    /// a source that cannot seek (a server without `Range`).
    fn seek(&mut self, pos: u64) -> io::Result<u64>;

    /// The size in bytes, when it is known and the source can seek. `None`
    /// makes the demuxer read the source as a stream (no seeking).
    fn size(&mut self) -> Option<u64>;
}

/// A local file (seekable).
pub struct FileSource {
    file: File,
    size: u64,
}

impl FileSource {
    /// Opens the file at `path`.
    pub fn open(path: impl AsRef<Path>) -> io::Result<FileSource> {
        let file = File::open(path)?;
        let size = file.metadata()?.len();
        Ok(FileSource { file, size })
    }
}

impl ByteSource for FileSource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.file.read(buf)
    }

    fn seek(&mut self, pos: u64) -> io::Result<u64> {
        self.file.seek(SeekFrom::Start(pos))
    }

    fn size(&mut self) -> Option<u64> {
        Some(self.size)
    }
}

/// Bytes in memory (`data:` and `blob:` URLs, tests).
pub struct MemorySource {
    data: Arc<[u8]>,
    pos: usize,
    seekable: bool,
}

impl MemorySource {
    /// A seekable source over `data`.
    pub fn new(data: impl Into<Arc<[u8]>>) -> MemorySource {
        MemorySource {
            data: data.into(),
            pos: 0,
            seekable: true,
        }
    }

    /// A source over `data` that cannot seek and does not tell its size, as
    /// a response from a server without `Range`.
    pub fn stream(data: impl Into<Arc<[u8]>>) -> MemorySource {
        MemorySource {
            seekable: false,
            ..MemorySource::new(data)
        }
    }
}

impl ByteSource for MemorySource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let rest = self.data.get(self.pos..).unwrap_or_default();
        let n = rest.len().min(buf.len());
        buf[..n].copy_from_slice(&rest[..n]);
        self.pos += n;
        Ok(n)
    }

    fn seek(&mut self, pos: u64) -> io::Result<u64> {
        if !self.seekable {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "the source cannot seek",
            ));
        }
        // A position past the end reads nothing, as for a file.
        self.pos = usize::try_from(pos).unwrap_or(usize::MAX);
        Ok(pos)
    }

    fn size(&mut self) -> Option<u64> {
        self.seekable.then(|| self.data.len() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_source_reads_and_seeks() {
        let mut s = MemorySource::new(b"abcdef".to_vec());
        let mut buf = [0u8; 4];
        assert_eq!(s.read(&mut buf).ok(), Some(4));
        assert_eq!(&buf, b"abcd");
        assert_eq!(s.read(&mut buf).ok(), Some(2));
        assert_eq!(s.read(&mut buf).ok(), Some(0));
        assert_eq!(s.seek(1).ok(), Some(1));
        assert_eq!(s.read(&mut buf).ok(), Some(4));
        assert_eq!(&buf, b"bcde");
        assert_eq!(s.seek(100).ok(), Some(100));
        assert_eq!(s.read(&mut buf).ok(), Some(0));
        assert_eq!(s.size(), Some(6));
    }

    #[test]
    fn stream_source_does_not_seek() {
        let mut s = MemorySource::stream(b"abc".to_vec());
        assert_eq!(s.size(), None);
        assert!(s.seek(0).is_err());
    }
}
