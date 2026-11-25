//! `canPlayType()` (HTML "can play type",
//! <https://html.spec.whatwg.org/multipage/media.html#dom-navigator-canplaytype>):
//! what this build answers for a MIME type with an optional `codecs`
//! parameter.
//!
//! The answers are Chromium 148's (probe `media-can-play-type.json`, 149
//! types, `video` and `audio` elements agree) limited to what this build
//! decodes (ADR 0047 section 1): without the feature `ffmpeg` every answer
//! is `""`. The departures from Chromium, on purpose:
//!
//! - HLS types (`application/vnd.apple.mpegurl` and the like) are `""`
//!   (ground rules: until swb has native HLS, sites must use hls.js);
//! - AV1 (`av01.*`) is `""`: the build has no AV1 decoder (a container
//!   with only AV1 is `""`, a list with an AV1 codec is `""`).
//!
//! Parsing follows the measured rules: the type is case-insensitive and
//! white space around it and the codecs is ignored; a `codecs` parameter
//! that is missing, empty or unterminated gives `maybe` for a known
//! container; the codec strings are case-sensitive in their prefix; one
//! unknown codec makes the whole answer `""`; single quotes are not quotes;
//! other parameters are ignored.

/// The answer of `canPlayType()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanPlay {
    /// `""`: the type is not supported.
    No,
    /// `"maybe"`: the container is supported, the codecs are not known.
    Maybe,
    /// `"probably"`: the container and every codec are supported.
    Probably,
}

impl CanPlay {
    /// The string that `canPlayType()` returns.
    pub fn as_str(self) -> &'static str {
        match self {
            CanPlay::No => "",
            CanPlay::Maybe => "maybe",
            CanPlay::Probably => "probably",
        }
    }
}

/// What a codec string is, for a container.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Codec {
    Known,
    /// The family is right but the parameters are not (`avc1.F4001F`):
    /// `maybe`.
    Unsure,
    Unknown,
}

/// The codec families of a container.
#[derive(Clone, Copy)]
enum Container {
    Mp4,
    Webm,
    Ogg,
    Matroska,
    /// MP3, AAC and FLAC files, WAV: one codec, the codec strings that the
    /// type allows, and the answer for the type without codecs.
    Audio(&'static [&'static str], CanPlay),
}

/// The answer of `canPlayType(mime_type)` in this build.
pub fn can_play_type(mime_type: &str) -> CanPlay {
    if !crate::ENABLED {
        return CanPlay::No;
    }
    // A NUL character gives "" (measured).
    if mime_type.contains('\0') {
        return CanPlay::No;
    }
    let (essence, params) = match mime_type.split_once(';') {
        Some((essence, params)) => (essence, Some(params)),
        None => (mime_type, None),
    };
    let essence = essence.trim().to_ascii_lowercase();
    let Some(container) = container_of(&essence) else {
        return CanPlay::No;
    };
    let codecs = params.and_then(codecs_parameter);
    let Some(codecs) = codecs else {
        return match container {
            Container::Audio(_, bare) => bare,
            _ => CanPlay::Maybe,
        };
    };
    let mut answer = CanPlay::Probably;
    let mut any = false;
    for codec in codecs.split(',').map(str::trim).filter(|c| !c.is_empty()) {
        any = true;
        match check_codec(container, codec) {
            Codec::Known => {}
            Codec::Unsure => answer = CanPlay::Maybe,
            Codec::Unknown => return CanPlay::No,
        }
    }
    if any { answer } else { CanPlay::Maybe }
}

/// The container of a MIME essence, or `None` for a type that swb does not
/// play (and for the HLS types).
fn container_of(essence: &str) -> Option<Container> {
    Some(match essence {
        "video/mp4" | "audio/mp4" | "audio/x-m4a" | "video/x-m4v" | "video/3gpp" => Container::Mp4,
        "video/webm" | "audio/webm" => Container::Webm,
        "video/ogg" | "audio/ogg" | "application/ogg" => Container::Ogg,
        "video/x-matroska" | "audio/x-matroska" => Container::Matroska,
        "audio/mpeg" | "audio/mp3" | "audio/x-mp3" => Container::Audio(&["mp3"], CanPlay::Probably),
        // PCM: the codec string `1` (the format tag of PCM) is `probably`;
        // the bare type is `maybe`.
        "audio/wav" | "audio/x-wav" => Container::Audio(&["1"], CanPlay::Maybe),
        "audio/aac" | "audio/flac" => Container::Audio(&[], CanPlay::Probably),
        _ => return None,
    })
}

/// The value of the `codecs` parameter in `params` (the text after the
/// first `;`): `None` when it is missing, empty or its quote is not closed.
fn codecs_parameter(params: &str) -> Option<&str> {
    for param in params.split(';') {
        let Some((name, value)) = param.split_once('=') else {
            continue;
        };
        if !name.trim().eq_ignore_ascii_case("codecs") {
            continue;
        }
        let value = value.trim();
        let value = match value.strip_prefix('"') {
            Some(rest) => rest.strip_suffix('"')?,
            None => value,
        };
        let value = value.trim();
        return (!value.is_empty()).then_some(value);
    }
    None
}

fn check_codec(container: Container, codec: &str) -> Codec {
    match container {
        Container::Mp4 => check_mp4(codec),
        Container::Webm => check_webm(codec),
        Container::Ogg => match codec {
            "vorbis" | "opus" | "flac" | "vp8" => Codec::Known,
            _ => Codec::Unknown,
        },
        Container::Matroska => match check_mp4(codec) {
            Codec::Unknown => check_webm(codec),
            known => known,
        },
        Container::Audio(allowed, _) => {
            if allowed.contains(&codec) {
                Codec::Known
            } else {
                Codec::Unknown
            }
        }
    }
}

fn check_mp4(codec: &str) -> Codec {
    match codec {
        "opus" | "flac" | "mp4a.40.2" | "mp4a.40.5" | "mp4a.40.29" | "mp4a.69" | "mp4a.6B" => {
            Codec::Known
        }
        // `avc1` alone: the family without a profile.
        "avc1" | "avc3" => Codec::Unsure,
        _ => {
            if let Some(rest) = codec
                .strip_prefix("avc1.")
                .or_else(|| codec.strip_prefix("avc3."))
            {
                avc(rest)
            } else if let Some(rest) = codec.strip_prefix("vp09.") {
                vp9(rest)
            } else {
                Codec::Unknown
            }
        }
    }
}

fn check_webm(codec: &str) -> Codec {
    match codec {
        "vp8" | "vp8.0" | "vp9" | "opus" | "vorbis" => Codec::Known,
        _ => match codec.strip_prefix("vp09.") {
            Some(rest) => vp9(rest),
            None => Codec::Unknown,
        },
    }
}

/// `avc1.PPCCLL` (six hex digits: profile, constraints, level). The decoder
/// plays Baseline (0x42), Main (0x4D) and High (0x64) with any level; the
/// other profiles are `maybe` (`avc1.F4001F` is, measured).
fn avc(rest: &str) -> Codec {
    if rest.len() != 6 || !rest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Codec::Unknown;
    }
    match rest.get(..2).map(str::to_ascii_uppercase).as_deref() {
        Some("42" | "4D" | "64") => Codec::Known,
        _ => Codec::Unsure,
    }
}

/// `vp09.PP.LL.DD[.CC.cp.tc.mc.FF]`: numeric fields (profile 0 to 3, level,
/// bit depth 8, 10 or 12).
fn vp9(rest: &str) -> Codec {
    let fields: Vec<&str> = rest.split('.').collect();
    if fields.len() < 3
        || !fields
            .iter()
            .all(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()))
    {
        return Codec::Unknown;
    }
    let profile_ok = fields
        .first()
        .is_some_and(|p| matches!(*p, "00" | "01" | "02" | "03"));
    let depth_ok = fields
        .get(2)
        .is_some_and(|d| matches!(*d, "08" | "10" | "12"));
    if profile_ok && depth_ok {
        Codec::Known
    } else {
        Codec::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(t: &str) -> &'static str {
        can_play_type(t).as_str()
    }

    #[test]
    fn answers_follow_the_measured_rows_with_media() {
        if !crate::ENABLED {
            assert_eq!(answer("video/mp4"), "");
            return;
        }
        let rows: &[(&str, &str)] = &[
            ("video/mp4", "maybe"),
            ("video/mp4; codecs=\"avc1.42E01E, mp4a.40.2\"", "probably"),
            ("video/mp4; codecs=\"avc1.F4001F\"", "maybe"),
            ("video/mp4; codecs=\"avc1\"", "maybe"),
            ("video/mp4; codecs=\"hev1.1.6.L93.B0\"", ""),
            ("video/mp4; codecs=\"vp9\"", ""),
            ("video/mp4; codecs=\"av01.0.05M.08\"", ""),
            ("video/mp4; codecs=\"avc1.42E01E, vorbis\"", ""),
            ("audio/m4a", ""),
            ("audio/aac", "probably"),
            ("audio/aacp", ""),
            ("video/webm; codecs=\"vp8.0\"", "probably"),
            ("video/webm; codecs=\"VP8, Vorbis\"", ""),
            ("video/webm; codecs=\"avc1.42E01E\"", ""),
            ("audio/ogg; codecs=speex", ""),
            ("video/ogg; codecs=\"theora\"", ""),
            ("audio/mpeg", "probably"),
            ("audio/mpeg3", ""),
            ("audio/wav", "maybe"),
            ("audio/wav; codecs=1", "probably"),
            ("audio/wav; codecs=\"pcm\"", ""),
            ("audio/flac", "probably"),
            ("audio/flac; codecs=flac", ""),
            ("application/vnd.apple.mpegurl", ""),
            ("video/mp4;", "maybe"),
            ("video/mp4; codecs=", "maybe"),
            ("video/mp4; codecs=\"\"", "maybe"),
            ("video/mp4; codecs=\"", "maybe"),
            ("video/mp4; codecs='avc1.42E01E'", ""),
            ("video/mp4 ; codecs=\"avc1.42E01E\"", "probably"),
            (" video/mp4", "maybe"),
            ("VIDEO/MP4", "maybe"),
            ("Video/Mp4; Codecs=\"AVC1.42E01E\"", ""),
            ("video/mp4; codecs=\"avc1.42e01e\"", "probably"),
            ("video/mp4; codecs=\"avc1.42E01E, unknown\"", ""),
            ("video/mp4; profile=foo", "maybe"),
            ("video/mp4; codecs=\"avc1.42E01E\"; foo=bar", "probably"),
            ("video/mp4\0", ""),
            ("", ""),
            ("video/", ""),
            ("video/x-matroska; codecs=\"avc1.42E01E\"", "probably"),
        ];
        for (t, want) in rows {
            assert_eq!(answer(t), *want, "{t:?}");
        }
    }

    #[test]
    fn nothing_plays_without_media() {
        if crate::ENABLED {
            return;
        }
        assert_eq!(answer("video/mp4; codecs=\"avc1.42E01E\""), "");
        assert_eq!(answer("audio/mpeg"), "");
    }
}
