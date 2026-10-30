//! The serialization of one longhand's specified value (the value that a
//! script reads from `element.style`), with the rewritings that Chromium
//! applies per property (measured, `tools/probes/web-cssom-style.json`):
//! keyword pairs in x/y order, implied defaults (`counter-reset: x` is
//! `x 0`), equal pairs written once (`background-size: 10px auto` is
//! `10px`), two-value `display` as its short form, and the like.

use swb_css::{ComponentValue, Parser};

use super::props::Ser;
use super::value::{Style, serialize, split_commas, trimmed};
use crate::parse::color::parse_color;
use crate::properties::inert::declared_text as declared;

/// The generic font families (lower case; a quoted one stays quoted).
const GENERIC_FAMILIES: &[&str] = &[
    "serif",
    "sans-serif",
    "cursive",
    "fantasy",
    "monospace",
    "system-ui",
    "math",
    "emoji",
    "fangsong",
    "ui-serif",
    "ui-sans-serif",
    "ui-monospace",
    "ui-rounded",
    "-webkit-body",
];

/// Serializes the value `tokens` (accepted by the property's parser,
/// without `!important`, CSS-wide keywords and `var()`) of a longhand.
#[allow(clippy::too_many_lines)] // One arm per serialization class.
pub(crate) fn serialize_longhand(ser: Ser, tokens: &[ComponentValue]) -> String {
    let tokens = trimmed(tokens);
    match ser {
        Ser::Plain | Ser::Content => serialize(tokens, Style::PLAIN),
        Ser::Length => serialize(tokens, Style::LENGTH),
        Ser::Color => color(tokens),
        Ser::FontFamily => font_family(tokens),
        Ser::Position => position(&words(tokens, Style::LENGTH)),
        Ser::Repeat => per_layer(tokens, |w| repeat(&words(w, Style::PLAIN))),
        Ser::BackgroundSize => per_layer(tokens, |w| {
            let mut w = words(w, Style::LENGTH);
            if w.len() == 2 && w.get(1).is_some_and(|s| s == "auto") {
                w.truncate(1);
            }
            w.join(" ")
        }),
        Ser::AspectRatio => aspect_ratio(tokens),
        Ser::Counter(default) => counter(tokens, default),
        Ser::FontFeatures => per_layer(tokens, font_feature),
        Ser::Radius => {
            let w = words(tokens, Style::LENGTH);
            match w.as_slice() {
                [a, b] if a == b => a.clone(),
                _ => w.join(" "),
            }
        }
        Ser::GridAreas => tokens
            .iter()
            .filter_map(|t| match t {
                ComponentValue::String(s) => Some(grid_area_row(s)),
                ComponentValue::Ident(i) => Some(i.to_ascii_lowercase()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" "),
        Ser::Dasharray => {
            let items: Vec<String> = tokens
                .iter()
                .filter(|t| !matches!(t, ComponentValue::Whitespace | ComponentValue::Comma))
                .map(|t| serialize(std::slice::from_ref(t), Style::PLAIN))
                .collect();
            items.join(", ")
        }
        Ser::Integer => match tokens {
            [ComponentValue::Number(n)] => n
                .int_value
                .map_or_else(|| serialize(tokens, Style::PLAIN), |i| i.to_string()),
            _ => serialize(tokens, Style::PLAIN),
        },
        Ser::Opacity => match tokens {
            [ComponentValue::Percentage(n)] => {
                super::value::format_number(f64::from(n.value) / 100.0)
            }
            _ => serialize(tokens, Style::PLAIN),
        },
        Ser::Display => display(&words(tokens, Style::PLAIN)),
        Ser::DecorationLine => ordered(
            words(tokens, Style::PLAIN),
            &["underline", "overline", "line-through", "blink"],
        ),
        Ser::Contain => ordered(
            words(tokens, Style::PLAIN),
            &["size", "inline-size", "layout", "style", "paint"],
        ),
        Ser::JustifyItems => {
            let mut w = words(tokens, Style::PLAIN);
            if w.len() == 2 && w.get(1).is_some_and(|s| s == "legacy") {
                w.reverse();
            }
            w.join(" ")
        }
        Ser::Alignment => {
            let w = words(tokens, Style::PLAIN);
            if w == ["first", "baseline"] {
                "baseline".to_owned()
            } else {
                w.join(" ")
            }
        }
        Ser::TransitionProperty => declared("transition-property", tokens)
            .unwrap_or_else(|| serialize(tokens, Style::NAMES)),
        Ser::Easing => declared("transition-timing-function", tokens)
            .unwrap_or_else(|| serialize(tokens, Style::PLAIN)),
        Ser::WillChange => {
            declared("will-change", tokens).unwrap_or_else(|| serialize(tokens, Style::NAMES))
        }
        Ser::TouchAction => {
            declared("touch-action", tokens).unwrap_or_else(|| serialize(tokens, Style::PLAIN))
        }
        Ser::GridLine => grid_line(tokens),
        Ser::GridAutoFlow => {
            let w = words(tokens, Style::PLAIN);
            let column = w.iter().any(|s| s == "column");
            let dense = w.iter().any(|s| s == "dense");
            match (column, dense) {
                (true, true) => "column dense".to_owned(),
                (true, false) => "column".to_owned(),
                (false, true) => "dense".to_owned(),
                (false, false) => "row".to_owned(),
            }
        }
    }
}

/// The top-level space-separated items of a value, each serialized.
fn words(tokens: &[ComponentValue], style: Style) -> Vec<String> {
    tokens
        .split(|t| matches!(t, ComponentValue::Whitespace))
        .filter(|w| !w.is_empty())
        .map(|w| serialize(w, style))
        .collect()
}

/// Applies `f` to each comma-separated layer and joins the results.
fn per_layer(tokens: &[ComponentValue], f: impl Fn(&[ComponentValue]) -> String) -> String {
    split_commas(tokens)
        .into_iter()
        .map(f)
        .collect::<Vec<_>>()
        .join(", ")
}

/// A color: a keyword as a lower-case name, otherwise `rgb()` (`value.rs`).
/// (swb's parser has no hashless colors of the quirks mode.)
fn color(tokens: &[ComponentValue]) -> String {
    if let [ComponentValue::Ident(name)] = tokens
        && parse_color(&mut Parser::new(tokens)).is_ok()
    {
        return name.to_ascii_lowercase();
    }
    serialize(tokens, Style::PLAIN)
}

/// `font-family`: generic families in lower case, a name of several
/// identifiers or one that needs escapes in quotes, a quoted name that is
/// one plain identifier without quotes.
fn font_family(tokens: &[ComponentValue]) -> String {
    let families: Vec<String> = split_commas(tokens)
        .into_iter()
        .map(|family| match family {
            [ComponentValue::String(s)] => {
                if is_plain_identifier(s) && !is_generic(s) {
                    s.to_string()
                } else {
                    quoted(s)
                }
            }
            [ComponentValue::Ident(name)] if is_generic(name) => name.to_ascii_lowercase(),
            [ComponentValue::Ident(name)] if is_plain_identifier(name) => name.to_string(),
            parts => {
                let words: Vec<&str> = parts
                    .iter()
                    .filter_map(|p| match p {
                        ComponentValue::Ident(s) => Some(&**s),
                        _ => None,
                    })
                    .collect();
                quoted(&words.join(" "))
            }
        })
        .collect();
    families.join(", ")
}

fn is_generic(name: &str) -> bool {
    GENERIC_FAMILIES
        .iter()
        .any(|g| g.eq_ignore_ascii_case(name))
}

/// True if `s` is an identifier that needs no escapes.
pub(crate) fn is_plain_identifier(s: &str) -> bool {
    let mut out = String::new();
    swb_css::serialize_identifier(s, &mut out);
    out == s
        && !s.is_empty()
        && !s.starts_with(|c: char| c.is_ascii_digit())
        && !matches!(
            s.to_ascii_lowercase().as_str(),
            "inherit" | "initial" | "unset" | "default" | "revert" | "revert-layer"
        )
}

fn quoted(s: &str) -> String {
    let mut out = String::new();
    swb_css::serialize_string(s, &mut out);
    out
}

/// `transform-origin` and `object-position` with one or two keywords:
/// x before y, `center` for a missing one.
fn position(w: &[String]) -> String {
    let vertical = |s: &str| matches!(s, "top" | "bottom");
    let horizontal = |s: &str| matches!(s, "left" | "right");
    match w {
        [one] if vertical(one) => format!("center {one}"),
        [one] => format!("{one} center"),
        [a, b] if vertical(a) && (horizontal(b) || b == "center") => format!("{b} {a}"),
        [a, b] if a == "center" && horizontal(b) => format!("{b} {a}"),
        _ => w.join(" "),
    }
}

/// One layer of `background-repeat`.
fn repeat(w: &[String]) -> String {
    match w {
        [a, b] if a == "repeat" && b == "no-repeat" => "repeat-x".to_owned(),
        [a, b] if a == "no-repeat" && b == "repeat" => "repeat-y".to_owned(),
        [a, b] if a == b => a.clone(),
        _ => w.join(" "),
    }
}

/// `aspect-ratio`: a ratio has both terms; `auto` comes first.
fn aspect_ratio(tokens: &[ComponentValue]) -> String {
    let text = serialize(tokens, Style::PLAIN);
    let (auto, rest) = if text == "auto" {
        return text;
    } else if let Some(rest) = text.strip_prefix("auto ") {
        (true, rest.to_owned())
    } else if let Some(rest) = text.strip_suffix(" auto") {
        (true, rest.to_owned())
    } else {
        (false, text)
    };
    let ratio = if rest.contains('/') {
        rest
    } else {
        format!("{rest} / 1")
    };
    if auto { format!("auto {ratio}") } else { ratio }
}

/// `counter-reset` and the like: each name with its number.
fn counter(tokens: &[ComponentValue], default: i32) -> String {
    let items: Vec<&ComponentValue> = tokens
        .iter()
        .filter(|t| !matches!(t, ComponentValue::Whitespace))
        .collect();
    if let [ComponentValue::Ident(none)] = items.as_slice()
        && none.eq_ignore_ascii_case("none")
    {
        return "none".to_owned();
    }
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while let Some(item) = items.get(i) {
        let name = serialize(std::slice::from_ref(*item), Style::NAMES);
        let value = match items.get(i + 1) {
            Some(ComponentValue::Number(n)) => {
                i += 1;
                n.int_value.unwrap_or(default)
            }
            _ => default,
        };
        out.push(format!("{name} {value}"));
        i += 1;
    }
    out.join(" ")
}

/// One feature of `font-feature-settings`: `on` and `1` are left out,
/// `off` is `0`.
fn font_feature(tokens: &[ComponentValue]) -> String {
    let w = words(tokens, Style::PLAIN);
    match w.as_slice() {
        [tag, value] if value == "on" || value == "1" => tag.clone(),
        [tag, value] if value == "off" => format!("{tag} 0"),
        _ => w.join(" "),
    }
}

/// One row of `grid-template-areas`: one space between the cells, a run
/// of dots as one dot.
fn grid_area_row(row: &str) -> String {
    let cells: Vec<&str> = row.split_ascii_whitespace().flat_map(split_dots).collect();
    quoted(&cells.join(" "))
}

/// Splits a cell token at the boundaries between dots and names
/// (`a...b` is `a`, `.`, `b`), each run of dots one `.`.
fn split_dots(cell: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = cell.as_bytes();
    for i in 1..=bytes.len() {
        let boundary =
            i == bytes.len() || (bytes.get(i) == Some(&b'.')) != (bytes.get(i - 1) == Some(&b'.'));
        if boundary {
            let part = cell.get(start..i).unwrap_or("");
            out.push(if part.starts_with('.') { "." } else { part });
            start = i;
        }
    }
    out
}

/// `display`: the two-value forms that have a one-value form.
fn display(w: &[String]) -> String {
    let has = |k: &str| w.iter().any(|s| s == k);
    if w.len() == 2 {
        let inline = has("inline");
        let block = has("block");
        let inner = w
            .iter()
            .find(|s| !matches!(s.as_str(), "inline" | "block"))
            .map_or("flow", String::as_str);
        let short = match (block, inline, inner) {
            (true, _, "flow") => Some("block"),
            (_, true, "flow") => Some("inline"),
            (true, _, "flow-root") => Some("flow-root"),
            (_, true, "flow-root") => Some("inline-block"),
            (true, _, "flex") => Some("flex"),
            (_, true, "flex") => Some("inline-flex"),
            (true, _, "grid") => Some("grid"),
            (_, true, "grid") => Some("inline-grid"),
            (true, _, "table") => Some("table"),
            (_, true, "table") => Some("inline-table"),
            _ => None,
        };
        if let Some(short) = short {
            return short.to_owned();
        }
    }
    w.join(" ")
}

/// Keywords sorted by their place in `order`.
fn ordered(mut w: Vec<String>, order: &[&str]) -> String {
    w.sort_by_key(|s| order.iter().position(|o| o == s).unwrap_or(order.len()));
    w.join(" ")
}

/// A grid line: `span`, the number, then the name.
fn grid_line(tokens: &[ComponentValue]) -> String {
    let mut span = false;
    let mut number = None;
    let mut name = None;
    for t in tokens {
        match t {
            ComponentValue::Ident(s) if s.eq_ignore_ascii_case("span") => span = true,
            ComponentValue::Ident(s) if s.eq_ignore_ascii_case("auto") => {
                return "auto".to_owned();
            }
            ComponentValue::Ident(_) => {
                name = Some(serialize(std::slice::from_ref(t), Style::NAMES));
            }
            ComponentValue::Number(n) => number = n.int_value,
            _ => {}
        }
    }
    let mut parts = Vec::new();
    if span {
        parts.push("span".to_owned());
    }
    if let Some(n) = number {
        parts.push(n.to_string());
    }
    if let Some(n) = name {
        parts.push(n);
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ser(kind: Ser, css: &str) -> String {
        serialize_longhand(kind, &swb_css::parse_component_values(css))
    }

    #[test]
    fn rewritings_measured_in_chromium() {
        assert_eq!(ser(Ser::Color, "RED"), "red");
        assert_eq!(ser(Ser::Color, "#ff000080"), "rgba(255, 0, 0, 0.5)");
        assert_eq!(ser(Ser::Color, "currentColor"), "currentcolor");
        assert_eq!(
            ser(
                Ser::FontFamily,
                "Arial  ,  Helvetica Neue, 'sans-serif', \"Arial\", SANS-SERIF, a\\,b"
            ),
            "Arial, \"Helvetica Neue\", \"sans-serif\", Arial, sans-serif, \"a,b\""
        );
        assert_eq!(ser(Ser::Position, "top left"), "left top");
        assert_eq!(ser(Ser::Position, "center"), "center center");
        assert_eq!(ser(Ser::Position, "bottom"), "center bottom");
        assert_eq!(ser(Ser::Position, "0 0"), "0px 0px");
        assert_eq!(
            ser(Ser::Repeat, "repeat no-repeat, repeat repeat"),
            "repeat-x, repeat"
        );
        assert_eq!(ser(Ser::BackgroundSize, "10px auto"), "10px");
        assert_eq!(ser(Ser::AspectRatio, "2"), "2 / 1");
        assert_eq!(ser(Ser::AspectRatio, "1/1 auto"), "auto 1 / 1");
        assert_eq!(ser(Ser::Counter(0), "x 1 y"), "x 1 y 0");
        assert_eq!(ser(Ser::Counter(1), "x"), "x 1");
        assert_eq!(
            ser(Ser::FontFeatures, "'smcp', 'kern' 1, \"liga\" off"),
            "\"smcp\", \"kern\", \"liga\" 0"
        );
        assert_eq!(ser(Ser::Radius, "5px 5px"), "5px");
        assert_eq!(
            ser(Ser::GridAreas, "'a  b'  'c ... d'"),
            "\"a b\" \"c . d\""
        );
        assert_eq!(ser(Ser::Dasharray, "1 2"), "1, 2");
        assert_eq!(ser(Ser::Integer, "+3"), "3");
        assert_eq!(ser(Ser::Opacity, "50%"), "0.5");
        assert_eq!(ser(Ser::Display, "inline flex"), "inline-flex");
        assert_eq!(ser(Ser::Display, "block flow"), "block");
        assert_eq!(
            ser(Ser::DecorationLine, "line-through underline"),
            "underline line-through"
        );
        assert_eq!(ser(Ser::Contain, "paint layout"), "layout paint");
        assert_eq!(ser(Ser::JustifyItems, "center legacy"), "legacy center");
        assert_eq!(ser(Ser::Alignment, "first baseline"), "baseline");
        assert_eq!(ser(Ser::GridLine, "a 2"), "2 a");
        assert_eq!(ser(Ser::GridLine, "span A"), "span A");
        assert_eq!(ser(Ser::GridAutoFlow, "dense column"), "column dense");
        assert_eq!(ser(Ser::GridAutoFlow, "row dense"), "dense");
    }
}
