//! HTTP byte ranges: the `Range` request header and the `Content-Range`
//! response header (RFC 9110 sections 14.1 to 14.4). The replay of fixtures
//! and the media byte source use them.
//!
//! <https://httpwg.org/specs/rfc9110.html#range.requests>

/// One range of a `Range` header with the unit `bytes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RangeSpec {
    /// `bytes=N-`: from `N` to the end.
    From(u64),
    /// `bytes=N-M`: from `N` to `M`, inclusive.
    Span(u64, u64),
    /// `bytes=-N`: the last `N` bytes.
    Suffix(u64),
}

/// What a server answers to a range over a body of known length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Resolved {
    /// The range is not usable: the server sends the whole body with 200
    /// (a header that is not a single `bytes` range, or `last < first`).
    Whole,
    /// 206 with the bytes `start..=end`.
    Partial { start: u64, end: u64 },
    /// 416: the range starts after the last byte (or the body is empty).
    Unsatisfiable,
}

/// Parses the value of a `Range` header. Returns `None` for another unit,
/// several ranges or a malformed value (a server ignores such a header).
pub(crate) fn parse_range(value: &str) -> Option<RangeSpec> {
    let spec = value.trim().strip_prefix("bytes=")?.trim();
    if spec.contains(',') {
        return None;
    }
    let (first, last) = spec.split_once('-')?;
    let number = |s: &str| {
        let s = s.trim();
        (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse::<u64>().ok())
    };
    match (number(first), number(last)) {
        (Some(first), None) if last.trim().is_empty() => first.map(RangeSpec::From),
        (Some(first), Some(last)) => Some(RangeSpec::Span(first?, last?)),
        (None, Some(last)) if first.trim().is_empty() => last.map(RangeSpec::Suffix),
        _ => None,
    }
}

/// Resolves `spec` against a body of `len` bytes.
pub(crate) fn resolve(spec: RangeSpec, len: u64) -> Resolved {
    let last = len.saturating_sub(1);
    match spec {
        RangeSpec::From(start) if start < len => Resolved::Partial { start, end: last },
        RangeSpec::Span(start, end) if end >= start && start < len => Resolved::Partial {
            start,
            end: end.min(last),
        },
        RangeSpec::Span(start, end) if end < start => Resolved::Whole,
        RangeSpec::Suffix(n) if n > 0 && len > 0 => Resolved::Partial {
            start: len.saturating_sub(n),
            end: last,
        },
        _ => Resolved::Unsatisfiable,
    }
}

/// The value of a `Content-Range` header of a 206 response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ContentRange {
    /// The first byte of the body.
    pub(crate) start: u64,
    /// The last byte of the body (inclusive).
    pub(crate) end: u64,
    /// The length of the whole resource; `None` for `*`.
    pub(crate) total: Option<u64>,
}

/// Parses `bytes START-END/TOTAL` (or `/*`).
pub(crate) fn parse_content_range(value: &str) -> Option<ContentRange> {
    let rest = value.trim().strip_prefix("bytes")?.trim_start();
    let (span, total) = rest.split_once('/')?;
    let (start, end) = span.split_once('-')?;
    let start: u64 = start.trim().parse().ok()?;
    let end: u64 = end.trim().parse().ok()?;
    let total = match total.trim() {
        "*" => None,
        number => Some(number.parse::<u64>().ok()?),
    };
    (end >= start && total.is_none_or(|t| end < t)).then_some(ContentRange { start, end, total })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_ranges_only() {
        assert_eq!(parse_range("bytes=0-"), Some(RangeSpec::From(0)));
        assert_eq!(parse_range("bytes=5-10"), Some(RangeSpec::Span(5, 10)));
        assert_eq!(parse_range("bytes=-5"), Some(RangeSpec::Suffix(5)));
        for bad in [
            "bytes=0-1,5-6",
            "items=0-",
            "bytes=",
            "bytes=-",
            "bytes=a-b",
            "0-5",
            "bytes=99999999999999999999-",
        ] {
            assert_eq!(parse_range(bad), None, "{bad}");
        }
    }

    #[test]
    fn resolves_against_the_length() {
        let partial = |start, end| Resolved::Partial { start, end };
        assert_eq!(resolve(RangeSpec::From(0), 10), partial(0, 9));
        assert_eq!(resolve(RangeSpec::From(9), 10), partial(9, 9));
        assert_eq!(resolve(RangeSpec::From(10), 10), Resolved::Unsatisfiable);
        assert_eq!(resolve(RangeSpec::From(0), 0), Resolved::Unsatisfiable);
        assert_eq!(resolve(RangeSpec::Span(2, 100), 10), partial(2, 9));
        assert_eq!(resolve(RangeSpec::Span(5, 2), 10), Resolved::Whole);
        assert_eq!(resolve(RangeSpec::Suffix(3), 10), partial(7, 9));
        assert_eq!(resolve(RangeSpec::Suffix(30), 10), partial(0, 9));
        assert_eq!(resolve(RangeSpec::Suffix(0), 10), Resolved::Unsatisfiable);
    }

    #[test]
    fn parses_content_ranges() {
        assert_eq!(
            parse_content_range("bytes 0-9/10"),
            Some(ContentRange {
                start: 0,
                end: 9,
                total: Some(10)
            })
        );
        assert_eq!(parse_content_range("bytes 5-9/*").unwrap().total, None);
        assert_eq!(parse_content_range("bytes 5-10/10"), None);
        assert_eq!(parse_content_range("bytes 9-5/10"), None);
        assert_eq!(parse_content_range("bytes */10"), None);
    }
}
