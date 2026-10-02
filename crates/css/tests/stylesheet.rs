//! Smoke tests: parse a realistic stylesheet and check the result.

use swb_css::{
    ComponentValue, CssRule, Declaration, MediaEnvironment, MediaType, Parser, StyleRule,
    Stylesheet, parse_stylesheet, serialize_component_values,
};

const SAMPLE: &str = r#"@charset "UTF-8";
@import url("print.css") print;
@import 'theme.css' layer(theme) supports(display: grid) screen and (min-width: 40em);

/* Custom properties and the root element. */
:root {
  --color-base: #202122;
  --color-link: #36c;
  --Spacing-Unit: calc(0.5rem + 2px);
  --empty:;
  --json: { "a": [1, 2] };
}

html, body {
  margin: 0;
  padding: 0;
  font-family: -apple-system, "Segoe UI", Roboto, sans-serif;
  color: var(--color-base, #000);
}

a:not([href]) { color: inherit; text-decoration: none }
a:link, a:visited { color: var(--color-link) }
a:hover, a:focus-visible { text-decoration: underline !important; }

.mw-body .mw-heading > h2::after {
  content: "\2014  " attr(data-x);
  display: block;
}

ul li:nth-child(2n+1 of .item) { background: rgba(0, 0, 0, .05) }
ol > li + li, dl dt ~ dd { margin-top: .25em }
input[type="checkbox" i]:checked + label::before { content: '\2713' }
.box:has(> img) { padding: 0 }
.vector-menu:is(.a, .b) :where(.c) { float: left; }
table.wikitable > tr > th[scope=col] { text-align: center; }

/* Vendor-specific selectors are dropped, as in other browsers. */
::-webkit-scrollbar { width: 8px }
input::-moz-placeholder, input::placeholder { color: gray }

@media screen and (min-width: 720px) {
  .sidebar { width: calc(100% - 2 * var(--Spacing-Unit)); }
  @media (prefers-color-scheme: dark) {
    :root { --color-base: #eaecf0; }
  }
}

@media print {
  .noprint { display: none !important }
}

@media (400px <= width < 1000px), not all and (monochrome) {
  .responsive { columns: 2 }
}

@supports (display: grid) and (not (display: inline-grid)) {
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(10em, 1fr)); }
}

@supports selector(:has(a)) {
  .has-support { color: green }
}

@font-face {
  font-family: "Example Sans";
  src: url(fonts/example.woff2) format("woff2"), url('fonts/example.woff') format("woff");
  font-display: swap;
  unicode-range: U+0000-00FF, U+0131, U+0152-0153;
}

@keyframes spin {
  from { transform: rotate(0deg) }
  to { transform: rotate(360deg) }
}

@layer base {
  p { line-height: 1.6; }
}

@page { margin: 1cm }
@namespace svg url(http://www.w3.org/2000/svg);

.nested {
  color: red;
  &:hover { color: blue; }
  .child { color: green }
  margin: 0;
}

.broken { color: red; width: ; height 10px; background: url(a b c); padding: 1px }
.unclosed { color: blue
"#;

fn style_rules(rules: &[CssRule]) -> Vec<&StyleRule> {
    rules
        .iter()
        .filter_map(|r| match r {
            CssRule::Style(s) => Some(s),
            _ => None,
        })
        .collect()
}

fn find<'a>(sheet: &'a Stylesheet, selectors: &str) -> &'a StyleRule {
    style_rules(&sheet.rules)
        .into_iter()
        .find(|r| r.selectors.to_string() == selectors)
        .unwrap_or_else(|| panic!("no rule with selectors {selectors:?}"))
}

fn declaration<'a>(rule: &'a StyleRule, name: &str) -> &'a Declaration {
    rule.declarations
        .iter()
        .find(|d| d.name == name)
        .unwrap_or_else(|| panic!("no declaration {name:?}"))
}

#[test]
fn rule_counts() {
    let sheet = parse_stylesheet(SAMPLE);
    let kinds: Vec<&str> = sheet
        .rules
        .iter()
        .map(|r| match r {
            CssRule::Style(_) => "style",
            CssRule::Media(_) => "media",
            CssRule::Import(_) => "import",
            CssRule::FontFace(_) => "font-face",
            CssRule::Supports(_) => "supports",
        })
        .collect();
    assert_eq!(
        kinds,
        vec![
            "import",
            "import", // @charset is ignored
            "style",
            "style",
            "style",
            "style",
            "style",
            "style",
            "style",
            "style",
            "style",
            "style",
            "style",
            "style", // ::-webkit-scrollbar and the -moz- list are dropped
            "media",
            "media",
            "media",
            "supports",
            "supports",
            "font-face",
            // @keyframes is dropped; the @layer block is flattened
            "style", // @page and @namespace are dropped
            "style",
            "style",
            "style",
        ]
    );
    assert_eq!(style_rules(&sheet.rules).len(), 16);
}

#[test]
fn declarations() {
    let sheet = parse_stylesheet(SAMPLE);
    let root = find(&sheet, ":root");
    let names: Vec<&str> = root.declarations.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "--color-base",
            "--color-link",
            "--Spacing-Unit",
            "--empty",
            "--json"
        ]
    );
    assert_eq!(
        serialize_component_values(&declaration(root, "--Spacing-Unit").value),
        "calc(0.5rem + 2px)"
    );
    assert_eq!(declaration(root, "--empty").value.len(), 0);
    assert_eq!(
        serialize_component_values(&declaration(root, "--json").value),
        "{ \"a\": [1, 2] }"
    );

    let html = find(&sheet, "html, body");
    let mut p = declaration(html, "font-family").parser();
    let families = p
        .parse_comma_separated(|p| {
            let mut words = Vec::new();
            while let Ok(w) = p.expect_ident_or_string() {
                words.push(w.to_owned());
            }
            if words.is_empty() {
                Err(swb_css::ParseError::Unexpected)
            } else {
                Ok(words.join(" "))
            }
        })
        .expect("font-family list");
    assert_eq!(
        families,
        ["-apple-system", "Segoe UI", "Roboto", "sans-serif"]
    );

    let hover = find(&sheet, "a:hover, a:focus-visible");
    assert!(declaration(hover, "text-decoration").important);

    let after = find(&sheet, ".mw-body .mw-heading > h2::after");
    let content = &declaration(after, "content").value;
    assert_eq!(content[0], ComponentValue::String("\u{2014} ".into()));
    assert!(content[2].is_function("attr"));

    let nested = find(&sheet, ".nested");
    let names: Vec<&str> = nested
        .declarations
        .iter()
        .map(|d| d.name.as_str())
        .collect();
    assert_eq!(names, vec!["color", "margin"]);

    let broken = find(&sheet, ".broken");
    let names: Vec<&str> = broken
        .declarations
        .iter()
        .map(|d| d.name.as_str())
        .collect();
    assert_eq!(names, vec!["color", "width", "background", "padding"]);
    assert_eq!(declaration(broken, "width").value.len(), 0);
    assert_eq!(
        declaration(broken, "background").value,
        vec![ComponentValue::BadUrl]
    );

    let unclosed = find(&sheet, ".unclosed");
    assert_eq!(unclosed.declarations.len(), 1);
}

#[test]
fn at_rules() {
    let sheet = parse_stylesheet(SAMPLE);
    let CssRule::Import(print) = &sheet.rules[0] else {
        panic!("expected @import");
    };
    assert_eq!(print.url, "print.css");
    let screen = MediaEnvironment::default();
    let paper = MediaEnvironment {
        media_type: MediaType::Print,
        ..MediaEnvironment::default()
    };
    assert!(!print.media.matches(&screen));
    assert!(print.media.matches(&paper));
    let CssRule::Import(theme) = &sheet.rules[1] else {
        panic!("expected @import");
    };
    assert_eq!(theme.url, "theme.css");
    assert!(theme.media.matches(&screen));

    let font_face = sheet
        .rules
        .iter()
        .find_map(|r| match r {
            CssRule::FontFace(f) => Some(f),
            _ => None,
        })
        .expect("@font-face");
    let names: Vec<&str> = font_face
        .declarations
        .iter()
        .map(|d| d.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["font-family", "src", "font-display", "unicode-range"]
    );
    let ranges: Vec<(u32, u32)> = font_face.declarations[3]
        .value
        .iter()
        .filter_map(|v| match v {
            ComponentValue::UnicodeRange { start, end } => Some((*start, *end)),
            _ => None,
        })
        .collect();
    assert_eq!(ranges, vec![(0, 0xFF), (0x131, 0x131), (0x152, 0x153)]);
    let mut src = font_face.declarations[1].parser();
    assert_eq!(src.expect_url(), Ok("fonts/example.woff2"));
}

#[test]
fn effective_rules_depend_on_environment() {
    let sheet = parse_stylesheet(SAMPLE);
    let supports = |d: &Declaration| {
        d.name == "display" && {
            let mut p = Parser::new(&d.value);
            p.expect_ident_matching("grid").is_ok() && p.is_exhausted()
        }
    };
    let collect = |env: &MediaEnvironment| {
        let mut selectors = Vec::new();
        sheet.for_each_style_rule(env, &supports, &mut |rule| {
            selectors.push(rule.selectors.to_string());
        });
        selectors
    };
    let wide = MediaEnvironment {
        viewport_width: 1200.0,
        ..MediaEnvironment::default()
    };
    let wide_rules = collect(&wide);
    assert!(wide_rules.contains(&".sidebar".to_owned()));
    assert!(wide_rules.contains(&".grid".to_owned()));
    assert!(wide_rules.contains(&".has-support".to_owned()));
    assert!(wide_rules.contains(&".responsive".to_owned()));
    assert!(!wide_rules.contains(&".noprint".to_owned()));
    let narrow = MediaEnvironment {
        viewport_width: 600.0,
        ..MediaEnvironment::default()
    };
    let narrow_rules = collect(&narrow);
    assert!(!narrow_rules.contains(&".sidebar".to_owned()));
    assert!(narrow_rules.contains(&".responsive".to_owned()));
    let paper = MediaEnvironment {
        media_type: MediaType::Print,
        ..MediaEnvironment::default()
    };
    assert!(collect(&paper).contains(&".noprint".to_owned()));
}

#[test]
fn style_attribute() {
    let decls = swb_css::parse_style_attribute(
        "color: red; background: url(x.png) no-repeat !important; ; bogus; --X: 1",
    );
    let names: Vec<&str> = decls.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, vec!["color", "background", "--X"]);
    assert!(decls[1].important);
}

/// A synthetic stylesheet of about 300 KB, similar in shape to a large site
/// stylesheet.
fn large_stylesheet() -> String {
    use std::fmt::Write as _;
    let mut css = String::new();
    let mut i = 0;
    while css.len() < 300_000 {
        let _ = write!(
            css,
            ".mw-parser-output .c{i} > a:not(.new):hover, #id{i} li:nth-child(2n+1) {{ \
             color: #{:06x}; margin: 0 auto {i}px; font: 400 14px/1.5 sans-serif; \
             background: url(\"img/{i}.png\") no-repeat calc(100% - 10px) 50%; }}\n\
             @media screen and (min-width: {}px) {{ .r{i} {{ display: flex; gap: .5em }} }}\n",
            i * 7919 % 0xFF_FFFF,
            i % 1200
        );
        i += 1;
    }
    css
}

#[test]
fn large_stylesheet_parses() {
    let css = large_stylesheet();
    let start = std::time::Instant::now();
    let sheet = parse_stylesheet(&css);
    let elapsed = start.elapsed();
    let rules = sheet.rules.len();
    assert!(rules > 1000, "{rules} rules");
    assert!(sheet.rules.iter().all(|r| match r {
        CssRule::Style(s) => s.declarations.len() == 4,
        CssRule::Media(m) => m.rules.len() == 1,
        _ => false,
    }));
    // Generous bound for unoptimized builds; release builds take a few ms.
    assert!(elapsed.as_secs() < 5, "parsing took {elapsed:?}");
}
