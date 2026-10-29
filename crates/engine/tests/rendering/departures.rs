// Where swb departs from Chromium on purpose in the cases of
// `cases.rs`: (case, query index, swb's value, why). The reasons are in
// ADR 0028, "Feature 7b2 notes".

/// The departures.
pub(crate) const SWB_VALUES: &[(&str, usize, &str, &str)] = &[
    (
        r"hidden",
        12,
        r#""cv""#,
        r"swb has no `content-visibility`, so the content of a `content-visibility: hidden` element is rendered text",
    ),
    (
        r"replaced",
        8,
        r#""axb""#,
        r"swb renders MathML as inline text; Chromium makes `math` a block and maps the letter of `mi` to a mathematical italic letter",
    ),
    (
        r"setter",
        15,
        r#""ReferenceError: MutationObserver is not defined""#,
        r"swb has no `MutationObserver` yet",
    ),
    (
        r"parse",
        35,
        r#"["(min-width: calc(100px + 1em))", false]"#,
        r"swb's media queries keep a `calc()` as written and cannot resolve `em` in it when they parse it (the query is false); Chromium simplifies it to `calc(1em + 100px)`",
    ),
    (
        r"parse",
        101,
        r#"["(inverted-colors: none)", true]"#,
        r"swb knows `inverted-colors` (Media Queries 5); Chromium 148 does not, so its query is false",
    ),
    (
        r"bbox",
        16,
        r"[0, -10, 124, 75]",
        r"swb has no box for SVG `text`, so the box of the `<svg>` leaves it out",
    ),
    (
        r"bbox",
        17,
        r"[0, 0, 0, 0]",
        r"swb has no box for SVG `text`",
    ),
    (
        r"length",
        1,
        r"54.852",
        r"Chromium measures curves with its own approximation, swb with 64 chords per curve (the rounded corners)",
    ),
    (
        r"length",
        2,
        r"62.839",
        r"Chromium measures curves with its own approximation (a circle of radius 10 is 62.427 long), swb with 64 chords per curve",
    ),
    (
        r"length",
        3,
        r"48.448",
        r"Chromium measures curves with its own approximation, swb with 64 chords per curve",
    ),
    (
        r"length",
        9,
        r"42.955",
        r"Chromium measures curves with its own approximation, swb with 64 chords per curve",
    ),
    (
        r"length",
        12,
        r"0",
        r"swb takes the length from the drawn content; Chromium computes it from the attributes also for an element that is not drawn (`display: none`)",
    ),
    (
        r"length",
        15,
        r"0",
        r"swb takes the length from the drawn content; Chromium computes it from the attributes also for an element outside the document",
    ),
];
