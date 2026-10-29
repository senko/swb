//! `URL` and `URLSearchParams` (M8 feature 8a1, ADR 0028 section 13): the
//! cases of the Chromium probe `tools/probes/web-url.json` (the interface
//! objects, parsing with and without a base, the attributes and their
//! setters on special, `file:`, opaque-path and other URLs, `searchParams`
//! linked both ways, the constructors of `URLSearchParams`, its methods and
//! iteration), each with and without the GC stress mode.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

#[path = "url/cases.rs"]
mod cases;

mod common;

use common::{Departure, probe_failures};

/// The values where swb departs from Chromium on purpose: (case, query
/// index, swb's value, why).
const SWB_VALUES: &[Departure] = &[
    (
        "parse-errors",
        3,
        r#""TypeError: Failed to construct 'URL': Invalid URL""#,
        "Chromium accepts a space in a host and percent-encodes it; the standard and the `url` crate refuse it",
    ),
    (
        "parse-other",
        4,
        r#"["file:///a","file://","file:","","","","","","/a","",""]"#,
        "Chromium keeps `localhost` as the host of a `file:` URL; the standard clears it",
    ),
    (
        "parse-other",
        31,
        r#"["chrome://settings/","null","chrome:","","","settings","settings","","/","",""]"#,
        "swb has no `chrome:` scheme, so the origin is opaque",
    ),
    (
        "setters-http-1",
        5,
        r##"["http://user:pw@a.com:8080/p/a?old=1#oldf","http://a.com:8080","http:","user","pw","a.com:8080","a.com","8080","/p/a","?old=1","#oldf"]"##,
        "Chromium accepts a space in a host and percent-encodes it; the standard and the `url` crate refuse it",
    ),
    (
        "setters-http-1",
        18,
        r##"["http://user:pw@a.com:8080/p/a?old=1#oldf","http://a.com:8080","http:","user","pw","a.com:8080","a.com","8080","/p/a","?old=1","#oldf"]"##,
        "Chromium accepts a space in a host and percent-encodes it; the standard and the `url` crate refuse it",
    ),
    (
        "setters-https-0",
        10,
        r#"["https://a.com/x","https://a.com","https:","","","a.com","a.com","","/x","",""]"#,
        "Chromium converts a URL between special and non-special schemes, and from or to an opaque path; the standard refuses (and so does the `url` crate)",
    ),
    (
        "setters-https-1",
        5,
        r#"["https://a.com/x","https://a.com","https:","","","a.com","a.com","","/x","",""]"#,
        "Chromium accepts a space in a host and percent-encodes it; the standard and the `url` crate refuse it",
    ),
    (
        "setters-https-1",
        18,
        r#"["https://a.com/x","https://a.com","https:","","","a.com","a.com","","/x","",""]"#,
        "Chromium accepts a space in a host and percent-encodes it; the standard and the `url` crate refuse it",
    ),
    (
        "setters-file-1",
        5,
        r##"["file:///dir/f.txt?q#h","file://","file:","","","","","","/dir/f.txt","?q","#h"]"##,
        "Chromium accepts a space in a host and percent-encodes it; the standard and the `url` crate refuse it",
    ),
    (
        "setters-file-1",
        18,
        r##"["file:///dir/f.txt?q#h","file://","file:","","","","","","/dir/f.txt","?q","#h"]"##,
        "Chromium accepts a space in a host and percent-encodes it; the standard and the `url` crate refuse it",
    ),
    (
        "setters-file-2",
        10,
        r##"["file:///a//b?q#h","file://","file:","","","","","","/a//b","?q","#h"]"##,
        "the `url` crate drops the empty first segments of a `file:` path; Chromium keeps them",
    ),
    (
        "setters-file-2",
        12,
        r##"["file:///x?q#h","file://","file:","","","","","","/x","?q","#h"]"##,
        "the `url` crate drops the empty first segments of a `file:` path; Chromium keeps them",
    ),
    (
        "setters-mailto-0",
        4,
        r##"["mailto:a@b.c?s=1#f","null","mailto:","","","","","","a@b.c","?s=1","#f"]"##,
        "Chromium converts a URL between special and non-special schemes, and from or to an opaque path; the standard refuses (and so does the `url` crate)",
    ),
    (
        "setters-mailto-0",
        5,
        r##"["mailto:a@b.c?s=1#f","null","mailto:","","","","","","a@b.c","?s=1","#f"]"##,
        "Chromium converts a URL between special and non-special schemes, and from or to an opaque path; the standard refuses (and so does the `url` crate)",
    ),
    (
        "setters-mailto-0",
        6,
        r##"["mailto:a@b.c?s=1#f","null","mailto:","","","","","","a@b.c","?s=1","#f"]"##,
        "Chromium converts a URL between special and non-special schemes, and from or to an opaque path; the standard refuses (and so does the `url` crate)",
    ),
    (
        "setters-mailto-0",
        7,
        r##"["mailto:a@b.c?s=1#f","null","mailto:","","","","","","a@b.c","?s=1","#f"]"##,
        "Chromium converts a URL between special and non-special schemes, and from or to an opaque path; the standard refuses (and so does the `url` crate)",
    ),
    (
        "setters-mailto-0",
        8,
        r##"["mailto:a@b.c?s=1#f","null","mailto:","","","","","","a@b.c","?s=1","#f"]"##,
        "Chromium converts a URL between special and non-special schemes, and from or to an opaque path; the standard refuses (and so does the `url` crate)",
    ),
    (
        "setters-mailto-0",
        9,
        r##"["mailto:a@b.c?s=1#f","null","mailto:","","","","","","a@b.c","?s=1","#f"]"##,
        "Chromium converts a URL between special and non-special schemes, and from or to an opaque path; the standard refuses (and so does the `url` crate)",
    ),
    (
        "setters-mailto-0",
        10,
        r##"["mailto:a@b.c?s=1#f","null","mailto:","","","","","","a@b.c","?s=1","#f"]"##,
        "Chromium converts a URL between special and non-special schemes, and from or to an opaque path; the standard refuses (and so does the `url` crate)",
    ),
    (
        "setters-mailto-0",
        16,
        r##"["mailto:a@b.c?s=1#f","null","mailto:","","","","","","a@b.c","?s=1","#f"]"##,
        "Chromium converts a URL between special and non-special schemes, and from or to an opaque path; the standard refuses (and so does the `url` crate)",
    ),
    (
        "setters-foo-0",
        4,
        r##"["foo://host:9/p?q#f","null","foo:","","","host:9","host","9","/p","?q","#f"]"##,
        "Chromium converts a URL between special and non-special schemes, and from or to an opaque path; the standard refuses (and so does the `url` crate)",
    ),
    (
        "setters-foo-0",
        5,
        r##"["foo://host:9/p?q#f","null","foo:","","","host:9","host","9","/p","?q","#f"]"##,
        "Chromium converts a URL between special and non-special schemes, and from or to an opaque path; the standard refuses (and so does the `url` crate)",
    ),
    (
        "setters-foo-0",
        6,
        r##"["foo://host:9/p?q#f","null","foo:","","","host:9","host","9","/p","?q","#f"]"##,
        "Chromium converts a URL between special and non-special schemes, and from or to an opaque path; the standard refuses (and so does the `url` crate)",
    ),
    (
        "setters-foo-0",
        7,
        r##"["foo://host:9/p?q#f","null","foo:","","","host:9","host","9","/p","?q","#f"]"##,
        "Chromium converts a URL between special and non-special schemes, and from or to an opaque path; the standard refuses (and so does the `url` crate)",
    ),
    (
        "setters-foo-0",
        8,
        r##"["foo://host:9/p?q#f","null","foo:","","","host:9","host","9","/p","?q","#f"]"##,
        "Chromium converts a URL between special and non-special schemes, and from or to an opaque path; the standard refuses (and so does the `url` crate)",
    ),
    (
        "setters-foo-0",
        9,
        r##"["foo://host:9/p?q#f","null","foo:","","","host:9","host","9","/p","?q","#f"]"##,
        "Chromium converts a URL between special and non-special schemes, and from or to an opaque path; the standard refuses (and so does the `url` crate)",
    ),
    (
        "setters-foo-0",
        10,
        r##"["foo://host:9/p?q#f","null","foo:","","","host:9","host","9","/p","?q","#f"]"##,
        "Chromium converts a URL between special and non-special schemes, and from or to an opaque path; the standard refuses (and so does the `url` crate)",
    ),
    (
        "setters-foo-0",
        16,
        r##"["foo://host:9/p?q#f","null","foo:","","","host:9","host","9","/p","?q","#f"]"##,
        "Chromium converts a URL between special and non-special schemes, and from or to an opaque path; the standard refuses (and so does the `url` crate)",
    ),
    (
        "setters-data-0",
        4,
        r##"["data:text/plain,hello#frag","null","data:","","","","","","text/plain,hello","","#frag"]"##,
        "Chromium converts a URL between special and non-special schemes, and from or to an opaque path; the standard refuses (and so does the `url` crate)",
    ),
    (
        "setters-data-0",
        5,
        r##"["data:text/plain,hello#frag","null","data:","","","","","","text/plain,hello","","#frag"]"##,
        "Chromium converts a URL between special and non-special schemes, and from or to an opaque path; the standard refuses (and so does the `url` crate)",
    ),
    (
        "setters-data-0",
        6,
        r##"["data:text/plain,hello#frag","null","data:","","","","","","text/plain,hello","","#frag"]"##,
        "Chromium converts a URL between special and non-special schemes, and from or to an opaque path; the standard refuses (and so does the `url` crate)",
    ),
    (
        "setters-data-0",
        7,
        r##"["data:text/plain,hello#frag","null","data:","","","","","","text/plain,hello","","#frag"]"##,
        "Chromium converts a URL between special and non-special schemes, and from or to an opaque path; the standard refuses (and so does the `url` crate)",
    ),
    (
        "setters-data-0",
        8,
        r##"["data:text/plain,hello#frag","null","data:","","","","","","text/plain,hello","","#frag"]"##,
        "Chromium converts a URL between special and non-special schemes, and from or to an opaque path; the standard refuses (and so does the `url` crate)",
    ),
    (
        "setters-data-0",
        9,
        r##"["data:text/plain,hello#frag","null","data:","","","","","","text/plain,hello","","#frag"]"##,
        "Chromium converts a URL between special and non-special schemes, and from or to an opaque path; the standard refuses (and so does the `url` crate)",
    ),
    (
        "setters-data-0",
        10,
        r##"["data:text/plain,hello#frag","null","data:","","","","","","text/plain,hello","","#frag"]"##,
        "Chromium converts a URL between special and non-special schemes, and from or to an opaque path; the standard refuses (and so does the `url` crate)",
    ),
    (
        "setters-data-0",
        16,
        r##"["data:text/plain,hello#frag","null","data:","","","","","","text/plain,hello","","#frag"]"##,
        "Chromium converts a URL between special and non-special schemes, and from or to an opaque path; the standard refuses (and so does the `url` crate)",
    ),
];

/// Runs every probe case with and without the GC stress mode.
#[test]
fn probe_cases() {
    let failures = probe_failures(cases::CASES, SWB_VALUES);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
