//! `NetSource`: the byte source over the loader's media resource (ADR 0047
//! section 5, M14 feature 4). Only with the feature `net`.

use std::io;

use crate::ByteSource;

/// A media resource of the loader: `Range` requests over HTTP(S), a local
/// file, or bytes in memory (`swb_net::MediaResource`). A server without
/// `Range` gives a source that cannot seek and has no size; cancelling the
/// resource (`swb_net::MediaHandle::cancel`, from any thread) makes a
/// blocked read fail at once.
pub struct NetSource {
    resource: swb_net::MediaResource,
}

impl NetSource {
    /// Wraps `resource`. Keep its handle (`MediaResource::handle`) before
    /// moving the source to the demux thread.
    pub fn new(resource: swb_net::MediaResource) -> NetSource {
        NetSource { resource }
    }
}

impl ByteSource for NetSource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.resource.read(buf)
    }

    fn seek(&mut self, pos: u64) -> io::Result<u64> {
        self.resource.seek(pos)
    }

    fn size(&mut self) -> Option<u64> {
        self.resource.size()
    }
}
