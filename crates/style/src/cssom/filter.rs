//! The specified value of `filter` as `element.style` and style rules
//! write it (measured in Chromium 148, `tools/probes/filter.json`):
//! function names in lower case, lengths as in other properties (`0` is
//! `0px`, `calc()` simplified, units kept), numbers and percentages as
//! written (`brightness(50%)`), an argument that is limited to 1 written
//! as 1 (`grayscale(2)` is `grayscale(1)`, `invert(200%)` is
//! `invert(100%)`), angles as written (`0` and `-0deg` are `0deg`), a
//! `drop-shadow()` with its color first, and `url()` with quotes. The
//! tokens passed the property's parser.

use swb_css::{ComponentValue, Parser};

use super::value::{Style, serialize, trimmed};
use crate::parse::color::parse_color;

/// Serializes the tokens of a `filter` value.
pub(super) fn serialize_filter(tokens: &[ComponentValue]) -> String {
    let items: Vec<String> = tokens
        .iter()
        .filter(|t| !matches!(t, ComponentValue::Whitespace))
        .map(function)
        .collect();
    items.join(" ")
}

/// One function (or `url()`, or the keyword `none`).
fn function(token: &ComponentValue) -> String {
    let ComponentValue::Function(f) = token else {
        return serialize(std::slice::from_ref(token), Style::PLAIN);
    };
    let name = f.name.to_ascii_lowercase();
    let args = trimmed(&f.arguments);
    match name.as_str() {
        "drop-shadow" => format!("drop-shadow({})", drop_shadow(args)),
        "grayscale" | "invert" | "opacity" | "sepia" => format!("{name}({})", limited(args)),
        "hue-rotate" => format!("{name}({})", angle(args)),
        _ => serialize(std::slice::from_ref(token), Style::PLAIN),
    }
}

/// An amount that is at most 1: a larger number or percentage is 1 or
/// 100%.
fn limited(args: &[ComponentValue]) -> String {
    match args {
        [ComponentValue::Number(n)] if n.value > 1.0 => "1".to_owned(),
        [ComponentValue::Percentage(n)] if n.value > 100.0 => "100%".to_owned(),
        _ => serialize(args, Style::PLAIN),
    }
}

/// An angle; a zero is `0deg`.
fn angle(args: &[ComponentValue]) -> String {
    match args {
        [ComponentValue::Number(n)] if n.value == 0.0 => "0deg".to_owned(),
        [ComponentValue::Dimension { number, .. }] if number.value == 0.0 => "0deg".to_owned(),
        _ => serialize(args, Style::PLAIN),
    }
}

/// The color first, then the two or three lengths.
fn drop_shadow(args: &[ComponentValue]) -> String {
    let mut color = None;
    let mut lengths = Vec::new();
    for word in args
        .split(|t| matches!(t, ComponentValue::Whitespace))
        .filter(|w| !w.is_empty())
    {
        if color.is_none() && parse_color(&mut Parser::new(word)).is_ok() {
            color = Some(super::longhand::color_text(word));
        } else {
            lengths.push(serialize(word, Style::LENGTH));
        }
    }
    let mut parts: Vec<String> = color.into_iter().collect();
    parts.extend(lengths);
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ser(css: &str) -> String {
        serialize_filter(&swb_css::parse_component_values(css))
    }

    #[test]
    fn rewritings_measured_in_chromium() {
        assert_eq!(ser("NONE"), "none");
        assert_eq!(ser("blur()"), "blur()");
        assert_eq!(ser("blur(0)"), "blur(0px)");
        assert_eq!(ser("BLUR( 2px )"), "blur(2px)");
        assert_eq!(ser("blur(calc(1px + 2px))"), "blur(calc(3px))");
        assert_eq!(ser("blur(1e1px)"), "blur(10px)");
        assert_eq!(ser("brightness(50%)"), "brightness(50%)");
        assert_eq!(ser("brightness(.5)"), "brightness(0.5)");
        assert_eq!(ser("brightness(0.333333333)"), "brightness(0.333333)");
        assert_eq!(ser("brightness(calc(1 + 1))"), "brightness(calc(2))");
        assert_eq!(ser("grayscale(2)"), "grayscale(1)");
        assert_eq!(ser("grayscale(200%)"), "grayscale(100%)");
        assert_eq!(ser("invert(50%)"), "invert(50%)");
        assert_eq!(ser("hue-rotate(0)"), "hue-rotate(0deg)");
        assert_eq!(ser("hue-rotate(-0deg)"), "hue-rotate(0deg)");
        assert_eq!(ser("hue-rotate(0.25turn)"), "hue-rotate(0.25turn)");
        assert_eq!(ser("hue-rotate(180DEG)"), "hue-rotate(180deg)");
        assert_eq!(ser("drop-shadow(1px 2px)"), "drop-shadow(1px 2px)");
        assert_eq!(ser("drop-shadow(0 0)"), "drop-shadow(0px 0px)");
        assert_eq!(
            ser("drop-shadow(1px 2px 3px red)"),
            "drop-shadow(red 1px 2px 3px)"
        );
        assert_eq!(
            ser("drop-shadow(1px 2px 3px #00ff0080)"),
            "drop-shadow(rgba(0, 255, 0, 0.5) 1px 2px 3px)"
        );
        assert_eq!(
            ser("drop-shadow(1px 2px 3px currentColor)"),
            "drop-shadow(currentcolor 1px 2px 3px)"
        );
        assert_eq!(ser("url(#a)"), "url(\"#a\")");
        assert_eq!(ser("url('#a') blur(2px)"), "url(\"#a\") blur(2px)");
        assert_eq!(
            ser("invert(1)brightness(55%)contrast(250%)hue-rotate(180deg)"),
            "invert(1) brightness(55%) contrast(250%) hue-rotate(180deg)"
        );
    }
}
