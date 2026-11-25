//! `MediaFile`: a media file decoded in order on one thread, with seeking
//! to an exact frame (ADR 0047 section 7, "Seeking").

use std::collections::VecDeque;

use tiny_skia::Pixmap;

use crate::decode::{AudioChunk, AudioDecoder, Demux, Output, Packet, RawVideoFrame, VideoDecoder};
use crate::{ByteSource, MediaError, MediaInfo};

/// How much earlier than its target a seek of audio alone starts decoding
/// (the samples before the target are dropped).
const AUDIO_PREROLL_US: i64 = 200_000;

/// Seeks `demux` for an exact seek to `target_us` (stream time) with the
/// tracks that are on: to the video's key frame at or before the target,
/// or, for audio alone, to the audio packet at or before the target minus
/// the preroll (audio decoders need the packets before the target: MP3's
/// bit reservoir, Opus's pre-skip; every audio packet is a key frame).
pub(crate) fn seek_demux(
    demux: &mut Demux,
    video_stream: Option<usize>,
    audio_stream: Option<usize>,
    target_us: i64,
) -> Result<(), MediaError> {
    let (stream, demux_target) = match (video_stream, audio_stream) {
        (Some(v), _) => (v, target_us),
        (None, Some(a)) => (a, target_us.saturating_sub(AUDIO_PREROLL_US)),
        (None, None) => return Err(MediaError::Seek("no track is on".to_owned())),
    };
    demux.seek(stream, demux_target)
}

/// The video frames after an exact seek: the frames before the target are
/// dropped except the last one at or before it, which comes out when the
/// first frame after the target (or the end) shows that it is the last.
#[derive(Default)]
pub(crate) struct SeekFilter {
    target: Option<i64>,
    held: Option<RawVideoFrame>,
}

impl SeekFilter {
    /// Starts filtering for a seek to `target_us` (stream time).
    pub(crate) fn start(&mut self, target_us: i64) {
        self.target = Some(target_us);
        self.held = None;
    }

    /// Stops filtering and drops the held frame.
    pub(crate) fn reset(&mut self) {
        self.target = None;
        self.held = None;
    }

    /// Takes a decoded frame; returns the frames that come out, in order:
    /// the held frame (the one at the target), then `frame`.
    pub(crate) fn offer(
        &mut self,
        frame: RawVideoFrame,
    ) -> (Option<RawVideoFrame>, Option<RawVideoFrame>) {
        let Some(target) = self.target else {
            return (None, Some(frame));
        };
        if frame.pts_us().is_none_or(|pts| pts <= target) {
            self.held = Some(frame);
            return (None, None);
        }
        self.target = None;
        (self.held.take(), Some(frame))
    }

    /// The end of the stream: the held frame, if any.
    pub(crate) fn finish(&mut self) -> Option<RawVideoFrame> {
        self.target = None;
        self.held.take()
    }
}

/// A video frame converted to RGBA.
pub struct VideoFrame {
    /// The presentation time in microseconds, when known.
    pub pts_us: Option<i64>,
    /// The duration in microseconds, when known.
    pub duration_us: Option<i64>,
    /// The pixels, opaque, at the frame's size.
    pub image: Pixmap,
}

/// What `MediaFile::decode_next` decodes.
pub enum Decoded {
    /// A video frame.
    Video(VideoFrame),
    /// A chunk of audio samples.
    Audio(AudioChunk),
}

/// A media file that decodes its selected tracks in order: the frames and
/// sample chunks come in the order of the packets in the container. After
/// `seek`, the first video frame is the last one at or before the target,
/// and the first samples are those at the target.
pub struct MediaFile {
    demux: Demux,
    video: Option<VideoDecoder>,
    audio: Option<AudioDecoder>,
    video_on: bool,
    audio_on: bool,
    /// A packet that its decoder refused because it was full.
    pending: Option<Packet>,
    out: VecDeque<Decoded>,
    /// After a seek: the frame at the target.
    filter: SeekFilter,
    eof: bool,
    video_ended: bool,
    audio_ended: bool,
}

impl MediaFile {
    /// Opens the media file that `source` holds (see `Demux::open`) and
    /// the decoders of its tracks.
    pub fn open(source: Box<dyn ByteSource>) -> Result<MediaFile, MediaError> {
        let mut demux = Demux::open(source)?;
        let video = demux.video_decoder()?;
        let audio = demux.audio_decoder()?;
        Ok(MediaFile {
            demux,
            video_on: video.is_some(),
            audio_on: audio.is_some(),
            video,
            audio,
            pending: None,
            out: VecDeque::new(),
            filter: SeekFilter::default(),
            eof: false,
            video_ended: false,
            audio_ended: false,
        })
    }

    /// The metadata.
    pub fn info(&self) -> &MediaInfo {
        self.demux.info()
    }

    /// The error of the demuxer that ended the stream early, if one did.
    pub fn error_at_end(&self) -> Option<&str> {
        self.demux.error_at_end()
    }

    /// Decodes only the tracks that are on (a track that is off costs
    /// nothing: the demuxer discards its packets). Takes effect for the
    /// packets read after the call; call it before decoding or before a
    /// seek.
    pub fn set_tracks(&mut self, video: bool, audio: bool) -> Result<(), MediaError> {
        self.video_on = video && self.video.is_some();
        self.audio_on = audio && self.audio.is_some();
        self.demux.select(self.video_on, self.audio_on)
    }

    fn done(&self) -> bool {
        self.eof && (!self.video_on || self.video_ended) && (!self.audio_on || self.audio_ended)
    }

    /// The next video frame or audio chunk, or `None` at the end.
    pub fn decode_next(&mut self) -> Result<Option<Decoded>, MediaError> {
        let mut refused = false;
        loop {
            if let Some(d) = self.out.pop_front() {
                return Ok(Some(d));
            }
            if self.done() {
                return Ok(None);
            }
            if self.receive_video()? || self.receive_audio()? {
                refused = false;
                continue;
            }
            // Both decoders need input.
            if self.eof {
                // A drained decoder that still asks for input: end it.
                self.video_ended = true;
                self.audio_ended = true;
                self.release_held()?;
                continue;
            }
            let packet = if let Some(p) = self.pending.take() {
                p
            } else if let Some(p) = self.demux.read()? {
                p
            } else {
                self.finish()?;
                continue;
            };
            if self.feed(packet)? {
                refused = false;
            } else if refused {
                return Err(MediaError::Decode(
                    "the decoder neither takes packets nor gives frames".to_owned(),
                ));
            } else {
                refused = true;
            }
        }
    }

    /// The next video frame, skipping audio chunks; `None` at the end.
    pub fn next_video_frame(&mut self) -> Result<Option<VideoFrame>, MediaError> {
        while let Some(d) = self.decode_next()? {
            if let Decoded::Video(f) = d {
                return Ok(Some(f));
            }
        }
        Ok(None)
    }

    /// Seeks to `target_us` (stream time): the demuxer goes to the key frame
    /// at or before it, the decoders decode forward, video frames before
    /// the target are dropped except the last one at or before it, and the
    /// audio samples before it are dropped.
    pub fn seek(&mut self, target_us: i64) -> Result<(), MediaError> {
        let video = self.video.as_ref().filter(|_| self.video_on);
        let audio = self.audio.as_ref().filter(|_| self.audio_on);
        seek_demux(
            &mut self.demux,
            video.map(VideoDecoder::stream),
            audio.map(AudioDecoder::stream),
            target_us,
        )?;
        self.out.clear();
        self.pending = None;
        self.filter.reset();
        self.eof = false;
        self.video_ended = false;
        self.audio_ended = false;
        if let Some(v) = &mut self.video {
            v.flush();
            self.filter.start(target_us);
        }
        if let Some(a) = &mut self.audio {
            a.flush();
            a.start_at(target_us);
        }
        Ok(())
    }

    /// Takes a frame of the video decoder; `true` if it gave one or ended.
    fn receive_video(&mut self) -> Result<bool, MediaError> {
        if !self.video_on || self.video_ended {
            return Ok(false);
        }
        let Some(v) = &mut self.video else {
            return Ok(false);
        };
        match v.receive()? {
            Output::Ready(frame) => {
                self.on_video_frame(frame)?;
                Ok(true)
            }
            Output::Ended => {
                self.video_ended = true;
                self.release_held()?;
                Ok(true)
            }
            Output::NeedsInput => Ok(false),
        }
    }

    /// Takes a chunk of the audio decoder; `true` if it gave one or ended.
    fn receive_audio(&mut self) -> Result<bool, MediaError> {
        if !self.audio_on || self.audio_ended {
            return Ok(false);
        }
        let Some(a) = &mut self.audio else {
            return Ok(false);
        };
        match a.receive()? {
            Output::Ready(chunk) => {
                self.out.push_back(Decoded::Audio(chunk));
                Ok(true)
            }
            Output::Ended => {
                self.audio_ended = true;
                Ok(true)
            }
            Output::NeedsInput => Ok(false),
        }
    }

    fn on_video_frame(&mut self, frame: RawVideoFrame) -> Result<(), MediaError> {
        let (held, frame) = self.filter.offer(frame);
        for f in [held, frame].into_iter().flatten() {
            let converted = self.convert(&f)?;
            self.out.push_back(Decoded::Video(converted));
        }
        Ok(())
    }

    /// Emits the frame held for a seek target.
    fn release_held(&mut self) -> Result<(), MediaError> {
        if let Some(frame) = self.filter.finish() {
            let converted = self.convert(&frame)?;
            self.out.push_back(Decoded::Video(converted));
        }
        Ok(())
    }

    fn convert(&mut self, frame: &RawVideoFrame) -> Result<VideoFrame, MediaError> {
        let Some(v) = &mut self.video else {
            return Err(MediaError::Decode("no video decoder".to_owned()));
        };
        let (w, h) = frame.size();
        let mut image = Pixmap::new(w, h)
            .ok_or_else(|| MediaError::Decode(format!("no image of {w} x {h} pixels")))?;
        v.convert(frame, &mut image)?;
        Ok(VideoFrame {
            pts_us: frame.pts_us(),
            duration_us: frame.duration_us(),
            image,
        })
    }

    /// Sends a packet to its decoder; `false` if the decoder was full.
    fn feed(&mut self, packet: Packet) -> Result<bool, MediaError> {
        let stream = packet.stream();
        let accepted = match (&mut self.video, &mut self.audio) {
            (Some(v), _) if self.video_on && v.stream() == stream => v.send(&packet)?,
            (_, Some(a)) if self.audio_on && a.stream() == stream => a.send(&packet)?,
            _ => true,
        };
        if !accepted {
            self.pending = Some(packet);
        }
        Ok(accepted)
    }

    /// The end of the container: drain the decoders.
    fn finish(&mut self) -> Result<(), MediaError> {
        self.eof = true;
        if let Some(v) = &mut self.video
            && self.video_on
        {
            v.finish()?;
        }
        if let Some(a) = &mut self.audio
            && self.audio_on
        {
            a.finish()?;
        }
        Ok(())
    }
}
