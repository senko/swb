//! Tests of the declaration blocks against the values that Chromium 148
//! gave (`tools/probes/web-cssom-style.json`).

#[path = "chromium_rows.rs"]
mod chromium_rows;

use super::*;

/// `setProperty(property, value)` on an empty block: the value read back
/// and the style attribute (`None` if the block did not change).
fn set(property: &str, value: &str) -> (String, Option<String>) {
    let mut block = StyleDeclarations::default();
    let changed = block.set_property(property, value, "");
    let attribute = changed.then(|| block.css_text());
    (block.property_value(property), attribute)
}

/// The rows where swb departs from Chromium, with the reason.
const DEPARTURES: &[(&str, &str, &str)] = &[
    (
        "color",
        "lab(50% 20 30)",
        "lab() keeps its units as written",
    ),
    ("width", "fit-content(10px)", "swb's parser accepts it"),
    ("display", "math", "swb's parser does not take `math`"),
    ("position", "-webkit-sticky", "swb's parser accepts it"),
    ("text-align", "match-parent", "swb's parser accepts it"),
    ("tab-size", "10px", "swb's parser takes numbers only"),
    (
        "transform",
        "matrix3d(1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1)",
        "no 3D transforms in swb",
    ),
    ("transform", "perspective(100px)", "no 3D transforms in swb"),
    ("transform", "rotateX(10deg)", "no 3D transforms in swb"),
    ("clip-path", "inset(10px)", "swb parses `url()` only"),
    ("clip-path", "circle(50%)", "swb parses `url()` only"),
    (
        "clip-path",
        "polygon(0 0, 10px 0, 0 10px)",
        "swb parses `url()` only",
    ),
    (
        "clip-path",
        "inset(1px 2px round 3px)",
        "swb parses `url()` only",
    ),
    ("fill", "context-fill", "swb's parser does not take it"),
    ("grid-template-columns", "subgrid", "no subgrid in swb"),
    ("--x", "  a  b  ", "whitespace runs collapse"),
    (
        "color",
        "var(--c) !important",
        "Chromium takes the priority from the value",
    ),
    ("grid", "auto-flow / 1fr 1fr", "`grid` is kept as written"),
    (
        "mask",
        "url(a.svg) no-repeat center / contain",
        "`mask` is kept as written",
    ),
    ("font-width", "condensed", "swb's alias of `font-stretch`"),
];

#[test]
fn longhands_and_shorthands_as_chromium_writes_them() {
    let mut bad = Vec::new();
    for &(property, value, chromium, attribute) in chromium_rows::ROWS {
        let departs = DEPARTURES
            .iter()
            .any(|&(p, v, _)| p == property && v == value);
        let (got, got_attr) = set(property, value);
        let same = got == chromium && got_attr.as_deref() == attribute;
        if same == departs {
            bad.push(format!(
                "{property}: {value:?} (departure: {departs})\n   chromium [{chromium}] {attribute:?}\n   swb      [{got}] {got_attr:?}"
            ));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// A block from `cssText`.
fn block(css: &str) -> StyleDeclarations {
    StyleDeclarations::parse(css, false)
}

#[test]
fn priorities() {
    let mut b = block("");
    b.set_property("color", "red", "important");
    assert_eq!(b.property_priority("color"), "important");
    assert_eq!(b.css_text(), "color: red !important;");
    // An unknown priority is ignored; `IMPORTANT` is one.
    assert!(!b.set_property("width", "1px", "foo"));
    b.set_property("color", "blue", "IMPORTANT");
    assert_eq!(b.css_text(), "color: blue !important;");
    // Without a priority the declaration is no longer important.
    b.set_property("color", "green", "");
    assert_eq!(b.css_text(), "color: green;");
    let mut b = block("");
    b.set_property("margin", "1px", "important");
    assert_eq!(b.css_text(), "margin: 1px !important;");
    assert_eq!(b.property_priority("margin"), "important");
    assert_eq!(b.property_priority("margin-top"), "important");
    assert_eq!(b.len(), 4);
    b.set_property("margin-top", "2px", "");
    assert_eq!(
        b.css_text(),
        "margin-top: 2px; margin-right: 1px !important; margin-bottom: 1px !important; margin-left: 1px !important;"
    );
    assert_eq!(b.property_value("margin"), "");
    assert_eq!(b.property_priority("margin"), "");
}

#[test]
fn shorthands_from_longhands() {
    let mut b = block("");
    for side in ["top", "right", "bottom"] {
        b.set_property(&format!("margin-{side}"), "1px", "");
    }
    assert_eq!(
        b.css_text(),
        "margin-top: 1px; margin-right: 1px; margin-bottom: 1px;"
    );
    b.set_property("margin-left", "1px", "");
    assert_eq!(b.css_text(), "margin: 1px;");
    assert_eq!(b.property_value("margin"), "1px");
    let cases = [
        (
            "margin-top: 1px; color: red; margin-right: 2px; margin-bottom: 1px; margin-left: 2px",
            "margin: 1px 2px; color: red;",
        ),
        ("margin: 1px; margin-top: 2px", "margin: 2px 1px 1px;"),
        (
            "margin-top: 1px; margin-right: 1px; margin-bottom: 1px; margin-left: 1px; color: red; border-width: 1px; border-style: solid; border-color: red",
            "margin: 1px; color: red; border-width: 1px; border-style: solid; border-color: red;",
        ),
        (
            "border-top: 1px solid red; border-right: 1px solid red; border-bottom: 1px solid red; border-left: 1px solid red",
            "border-width: 1px; border-style: solid; border-color: red;",
        ),
        (
            "border: 1px solid red; border-top-color: blue",
            "border-width: 1px; border-style: solid; border-color: blue red red; border-image: initial;",
        ),
        ("background-color: red", "background-color: red;"),
        (
            "overflow-x: hidden; overflow-y: hidden",
            "overflow: hidden;",
        ),
        (
            "flex-grow: 1; flex-shrink: 1; flex-basis: 0%",
            "flex: 1 1 0%;",
        ),
        (
            "font-size: 12px; font-family: a",
            "font-size: 12px; font-family: a;",
        ),
        (
            "inset: 1px; gap: 1px 2px; place-items: center",
            "inset: 1px; gap: 1px 2px; place-items: center;",
        ),
        (
            "border-radius: 1px; border-spacing: 1px 2px",
            "border-radius: 1px; border-spacing: 1px 2px;",
        ),
        (
            "list-style: square; outline: red; columns: 3",
            "list-style: square; outline: red; columns: 3;",
        ),
        (
            "flex-flow: column; overflow: hidden; contain-intrinsic-size: 10px",
            "flex-flow: column; overflow: hidden; contain-intrinsic-size: 10px;",
        ),
        (
            "grid-area: a; background-position: center",
            "grid-area: a; background-position: center center;",
        ),
        (
            "margin: 1px; margin-inline-start: 5px",
            "margin: 1px; margin-inline-start: 5px;",
        ),
        (
            "inline-size: 5px; width: 6px",
            "inline-size: 5px; width: 6px;",
        ),
        (
            "border: none",
            "border-width: medium; border-style: none; border-color: currentcolor; border-image: initial;",
        ),
        ("font-variant: small-caps", "font-variant: small-caps;"),
        (
            "font-variant-caps: small-caps",
            "font-variant-caps: small-caps;",
        ),
    ];
    for (css, chromium) in cases {
        assert_eq!(block(css).css_text(), chromium, "{css}");
    }
}

#[test]
fn shorthand_values_and_changes() {
    let b = block("border: 1px solid red; border-top-color: blue");
    assert_eq!(b.property_value("border"), "");
    assert_eq!(b.property_value("border-color"), "blue red red");
    assert_eq!(b.property_value("border-top"), "1px solid blue");
    assert_eq!(b.property_value("border-width"), "1px");
    let b = block(
        "border-top: 1px solid red; border-right: 1px solid red; border-bottom: 1px solid red; border-left: 1px solid red",
    );
    assert_eq!(b.property_value("border"), "");
    assert_eq!(b.property_value("border-top"), "1px solid red");
    let mut b = block("background: red");
    b.set_property("background-image", "url(a.png)", "");
    assert_eq!(b.css_text(), "background: url(\"a.png\") red;");
    let mut b = block("font: 12px a");
    b.set_property("font-weight", "bold", "");
    assert_eq!(b.css_text(), "font: bold 12px a;");
    let mut b = block("border: 1px solid red");
    b.set_property("border-top-width", "2px", "");
    assert_eq!(
        b.css_text(),
        "border-width: 2px 1px 1px; border-style: solid; border-color: red; border-image: initial;"
    );
    let b = block("text-decoration: underline");
    assert_eq!(b.property_value("text-decoration-color"), "initial");
    // Chromium gives `auto` to the style, not the color.
    let mut b = block("outline: auto");
    assert_eq!(b.property_value("outline-style"), "auto");
    assert_eq!(b.property_value("outline-color"), "initial");
    assert_eq!(b.remove_property("outline-style").0, "auto");
    assert_eq!(
        b.css_text(),
        "outline-color: initial; outline-width: initial;"
    );
    let b = block("outline: auto red");
    assert_eq!(b.css_text(), "outline: red auto;");
    let b = block("outline: red auto");
    assert_eq!(b.property_value("outline-style"), "auto");
}

#[test]
fn css_text_parsing() {
    let b = block(
        "color: RED; foo: bar; width: 10px !important; color: blue; /* c */ height: 1px; margin: 1px 2px; ;;; bad; padding-top: 0",
    );
    assert_eq!(
        b.css_text(),
        "color: blue; height: 1px; margin: 1px 2px; padding-top: 0px; width: 10px !important;"
    );
    assert_eq!(b.len(), 8);
    let cases = [
        (
            "margin: 1px !important; margin-top: 2px",
            "margin: 1px !important;",
        ),
        (
            "margin-top: 1px !important; margin-right: 1px; margin-bottom: 1px; margin-left: 1px",
            "margin-right: 1px; margin-bottom: 1px; margin-left: 1px; margin-top: 1px !important;",
        ),
        (
            "color: red !IMPORTANT; width: 1px ! important; height: 2px!important",
            "color: red !important; width: 1px !important; height: 2px !important;",
        ),
        ("  ;color:red", "color: red;"),
        ("color:red;}width:1px", "color: red;"),
        (
            "color:red;{width:1px};height:1px",
            "color: red; height: 1px;",
        ),
        ("--a: 1px; --a: 2px; --b:x", "--a: 2px; --b: x;"),
        ("xxxxxxxxxx", ""),
        ("COLOR:Blue;  margin:0", "color: blue; margin: 0px;"),
    ];
    for (css, chromium) in cases {
        assert_eq!(block(css).css_text(), chromium, "{css}");
    }
    let b = block("--a: 1px; --a: 2px; --b:x");
    assert_eq!((b.len(), b.item(0).as_deref()), (2, Some("--a")));
}

#[test]
fn set_remove_and_items() {
    let mut b = block("margin: 1px 2px");
    assert_eq!(b.remove_property("margin"), (String::new(), true));
    assert_eq!((b.css_text().as_str(), b.len()), ("", 0));
    let mut b = block("margin: 1px 2px");
    assert_eq!(b.remove_property("margin-top"), ("1px".to_owned(), true));
    assert_eq!(
        b.css_text(),
        "margin-right: 2px; margin-bottom: 1px; margin-left: 2px;"
    );
    assert_eq!(b.property_value("margin"), "");
    assert_eq!(b.remove_property("foo"), (String::new(), false));
    let mut b = block("");
    b.set_property("color", "red", "");
    b.set_property("width", "1px", "");
    b.set_property("color", "blue", "");
    assert_eq!(b.css_text(), "color: blue; width: 1px;");
    // An invalid value changes nothing; an empty one removes.
    assert!(!b.set_property("width", "bogus", ""));
    assert!(b.set_property("color", "", ""));
    assert_eq!(b.css_text(), "width: 1px;");
    let b = block("margin: 1px");
    let items: Vec<Option<String>> = (0..5).map(|i| b.item(i)).collect();
    assert_eq!(
        items,
        [
            Some("margin-top".to_owned()),
            Some("margin-right".to_owned()),
            Some("margin-bottom".to_owned()),
            Some("margin-left".to_owned()),
            None
        ]
    );
    let mut b = block("");
    b.set_property("COLOR", "red", "");
    b.set_property("--X", "1", "");
    assert_eq!(b.css_text(), "color: red; --X: 1;");
    assert_eq!(b.property_value("COLOR"), "red");
    assert_eq!(b.property_value("--x"), "");
    assert_eq!(b.property_value("--X"), "1");
    let mut b = block("margin: 1px 2px; padding: 3px; color: red");
    b.remove_property("padding-left");
    assert_eq!(
        b.css_text(),
        "margin: 1px 2px; padding-top: 3px; padding-right: 3px; padding-bottom: 3px; color: red;"
    );
}

#[test]
fn quirks_mode_lengths() {
    let mut b = StyleDeclarations::parse("", true);
    for (property, value) in [
        ("width", "100"),
        ("height", "100"),
        ("margin-top", "5"),
        ("line-height", "2"),
        ("padding", "1 2"),
        ("margin-inline-start", "3"),
    ] {
        b.set_property(property, value, "");
    }
    assert_eq!(
        b.css_text(),
        "width: 100px; height: 100px; margin-top: 5px; line-height: 2; padding: 1px 2px;"
    );
    let b = StyleDeclarations::parse("width: 10; top: 5; border: 1 solid red", true);
    assert_eq!(b.css_text(), "width: 10px; top: 5px;");
    // Departure: Chromium takes hashless colors in quirks mode
    // (`rgb(255, 0, 0)`); swb's parser does not.
    let b = StyleDeclarations::parse("color: ff0000; background-color: f00", true);
    assert_eq!(b.css_text(), "");
}

#[test]
fn supports_and_escape() {
    let property = [
        ("display", "flex", true),
        ("display", "bogus", false),
        ("DISPLAY", "flex", true),
        ("--x", "anything", true),
        ("color", "red !important", false),
        ("color", "", false),
        ("color", "var(--x)", true),
        ("display", " flex ", true),
        ("  display", "flex", false),
        ("width", "10", false),
        ("color", "red; width: 1px", false),
        ("float", "left", true),
        ("cssFloat", "left", false),
        ("-webkit-transform", "none", true),
    ];
    for (p, v, chromium) in property {
        assert_eq!(supports_property(p, v), chromium, "{p}: {v}");
    }
    let condition = [
        ("display: flex", true),
        ("(display: flex)", true),
        ("display:flex and color:red", false),
        ("(display:flex) and (color:red)", true),
        ("--x: 1", true),
        ("not (display: bogus)", true),
        ("", false),
        ("(color: red) or (bogus: 1)", true),
        ("font-tech(color-COLRv1)", false),
        ("color: red;", false),
        ("(color: red)  ", true),
        ("display:flex !important", true),
        ("(display:flex !important)", true),
        ("display: flex!important", true),
        ("(color: red !important) and (display: flex)", true),
        ("display: flex !IMPORTANT", true),
        ("display: flex ! important", true),
        ("(display: flex !important !important)", false),
    ];
    for (text, chromium) in condition {
        assert_eq!(supports_condition(text), chromium, "{text}");
    }
    let escapes = [
        ("a", "a"),
        ("1a", "\\31 a"),
        ("-1a", "-\\31 a"),
        ("-", "\\-"),
        ("--", "--"),
        ("a b", "a\\ b"),
        ("a\u{0}b", "a\u{FFFD}b"),
        ("\u{1}", "\\1 "),
        ("\u{7f}", "\\7f "),
        ("#id", "\\#id"),
        ("0", "\\30 "),
        ("a\\b", "a\\\\b"),
        ("\t", "\\9 "),
    ];
    for (ident, chromium) in escapes {
        assert_eq!(escape_identifier(ident), chromium, "{ident:?}");
    }
}
