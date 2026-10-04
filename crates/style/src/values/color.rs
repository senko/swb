//! Colors: computed RGBA values, `currentColor`, named and system colors.
//!
//! <https://www.w3.org/TR/css-color-4/>

/// An sRGB color with 8-bit channels and 8-bit alpha (non-premultiplied).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Rgba {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
    /// Alpha: 0 is transparent, 255 is opaque.
    pub a: u8,
}

impl Rgba {
    /// Fully transparent black.
    pub const TRANSPARENT: Rgba = Rgba::new(0, 0, 0, 0);
    /// Opaque black.
    pub const BLACK: Rgba = Rgba::new(0, 0, 0, 255);
    /// Opaque white.
    pub const WHITE: Rgba = Rgba::new(255, 255, 255, 255);

    /// Creates a color from channels.
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Rgba { r, g, b, a }
    }

    /// Creates an opaque color.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Rgba { r, g, b, a: 255 }
    }

    /// Creates a color from floating-point channels in 0..=255 and alpha in
    /// 0..=1, clamping and rounding as CSS serialization does.
    pub fn from_f32(r: f32, g: f32, b: f32, alpha: f32) -> Self {
        let channel = |v: f32| v.clamp(0.0, 255.0).round() as u8;
        Rgba {
            r: channel(r),
            g: channel(g),
            b: channel(b),
            a: channel(alpha * 255.0),
        }
    }

    /// True if the alpha is zero.
    pub fn is_transparent(self) -> bool {
        self.a == 0
    }

    /// Alpha as a fraction in 0..=1.
    pub fn alpha_f32(self) -> f32 {
        f32::from(self.a) / 255.0
    }
}

/// A computed color that can still refer to the element's `color`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Color {
    /// The value of the `color` property.
    #[default]
    CurrentColor,
    /// A concrete color.
    Rgba(Rgba),
}

impl Color {
    /// Transparent black.
    pub const TRANSPARENT: Color = Color::Rgba(Rgba::TRANSPARENT);

    /// The used color, given the element's `color` value.
    pub fn resolve(self, current_color: Rgba) -> Rgba {
        match self {
            Color::CurrentColor => current_color,
            Color::Rgba(c) => c,
        }
    }
}

impl From<Rgba> for Color {
    fn from(c: Rgba) -> Self {
        Color::Rgba(c)
    }
}

/// Looks up a CSS named color (ASCII case-insensitive), including
/// `transparent`.
pub(crate) fn named_color(name: &str) -> Option<Rgba> {
    let lower = name.to_ascii_lowercase();
    if lower == "transparent" {
        return Some(Rgba::TRANSPARENT);
    }
    NAMED_COLORS
        .binary_search_by(|(n, _)| n.cmp(&lower.as_str()))
        .ok()
        .map(|i| {
            let rgb = NAMED_COLORS[i].1;
            Rgba::rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
        })
}

/// Looks up a CSS system color (ASCII case-insensitive). The values are
/// those Chromium uses for a light color scheme.
/// <https://www.w3.org/TR/css-color-4/#css-system-colors>
pub(crate) fn system_color(name: &str) -> Option<Rgba> {
    let c = match name.to_ascii_lowercase().as_str() {
        "canvas" | "field" | "buttonhighlight" | "accentcolortext" => Rgba::WHITE,
        "canvastext" | "fieldtext" | "buttontext" | "highlighttext" | "marktext" => Rgba::BLACK,
        "linktext" => Rgba::rgb(0x00, 0x00, 0xEE),
        "visitedtext" => Rgba::rgb(0x55, 0x1A, 0x8B),
        "activetext" => Rgba::rgb(0xFF, 0x00, 0x00),
        "buttonface" => Rgba::rgb(0xEF, 0xEF, 0xEF),
        "buttonborder" => Rgba::rgb(0x76, 0x76, 0x76),
        "graytext" => Rgba::rgb(0x6D, 0x6D, 0x6D),
        "highlight" => Rgba::rgb(0xB5, 0xD5, 0xFF),
        "mark" => Rgba::rgb(0xFF, 0xFF, 0x00),
        "accentcolor" => Rgba::rgb(0x00, 0x75, 0xFF),
        _ => return None,
    };
    Some(c)
}

/// CSS named colors, sorted by name, as 0xRRGGBB.
/// <https://www.w3.org/TR/css-color-4/#named-colors>
#[allow(clippy::unreadable_literal)] // 0xRRGGBB, as in the specification.
const NAMED_COLORS: &[(&str, u32)] = &[
    ("aliceblue", 0xF0F8FF),
    ("antiquewhite", 0xFAEBD7),
    ("aqua", 0x00FFFF),
    ("aquamarine", 0x7FFFD4),
    ("azure", 0xF0FFFF),
    ("beige", 0xF5F5DC),
    ("bisque", 0xFFE4C4),
    ("black", 0x000000),
    ("blanchedalmond", 0xFFEBCD),
    ("blue", 0x0000FF),
    ("blueviolet", 0x8A2BE2),
    ("brown", 0xA52A2A),
    ("burlywood", 0xDEB887),
    ("cadetblue", 0x5F9EA0),
    ("chartreuse", 0x7FFF00),
    ("chocolate", 0xD2691E),
    ("coral", 0xFF7F50),
    ("cornflowerblue", 0x6495ED),
    ("cornsilk", 0xFFF8DC),
    ("crimson", 0xDC143C),
    ("cyan", 0x00FFFF),
    ("darkblue", 0x00008B),
    ("darkcyan", 0x008B8B),
    ("darkgoldenrod", 0xB8860B),
    ("darkgray", 0xA9A9A9),
    ("darkgreen", 0x006400),
    ("darkgrey", 0xA9A9A9),
    ("darkkhaki", 0xBDB76B),
    ("darkmagenta", 0x8B008B),
    ("darkolivegreen", 0x556B2F),
    ("darkorange", 0xFF8C00),
    ("darkorchid", 0x9932CC),
    ("darkred", 0x8B0000),
    ("darksalmon", 0xE9967A),
    ("darkseagreen", 0x8FBC8F),
    ("darkslateblue", 0x483D8B),
    ("darkslategray", 0x2F4F4F),
    ("darkslategrey", 0x2F4F4F),
    ("darkturquoise", 0x00CED1),
    ("darkviolet", 0x9400D3),
    ("deeppink", 0xFF1493),
    ("deepskyblue", 0x00BFFF),
    ("dimgray", 0x696969),
    ("dimgrey", 0x696969),
    ("dodgerblue", 0x1E90FF),
    ("firebrick", 0xB22222),
    ("floralwhite", 0xFFFAF0),
    ("forestgreen", 0x228B22),
    ("fuchsia", 0xFF00FF),
    ("gainsboro", 0xDCDCDC),
    ("ghostwhite", 0xF8F8FF),
    ("gold", 0xFFD700),
    ("goldenrod", 0xDAA520),
    ("gray", 0x808080),
    ("green", 0x008000),
    ("greenyellow", 0xADFF2F),
    ("grey", 0x808080),
    ("honeydew", 0xF0FFF0),
    ("hotpink", 0xFF69B4),
    ("indianred", 0xCD5C5C),
    ("indigo", 0x4B0082),
    ("ivory", 0xFFFFF0),
    ("khaki", 0xF0E68C),
    ("lavender", 0xE6E6FA),
    ("lavenderblush", 0xFFF0F5),
    ("lawngreen", 0x7CFC00),
    ("lemonchiffon", 0xFFFACD),
    ("lightblue", 0xADD8E6),
    ("lightcoral", 0xF08080),
    ("lightcyan", 0xE0FFFF),
    ("lightgoldenrodyellow", 0xFAFAD2),
    ("lightgray", 0xD3D3D3),
    ("lightgreen", 0x90EE90),
    ("lightgrey", 0xD3D3D3),
    ("lightpink", 0xFFB6C1),
    ("lightsalmon", 0xFFA07A),
    ("lightseagreen", 0x20B2AA),
    ("lightskyblue", 0x87CEFA),
    ("lightslategray", 0x778899),
    ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xB0C4DE),
    ("lightyellow", 0xFFFFE0),
    ("lime", 0x00FF00),
    ("limegreen", 0x32CD32),
    ("linen", 0xFAF0E6),
    ("magenta", 0xFF00FF),
    ("maroon", 0x800000),
    ("mediumaquamarine", 0x66CDAA),
    ("mediumblue", 0x0000CD),
    ("mediumorchid", 0xBA55D3),
    ("mediumpurple", 0x9370DB),
    ("mediumseagreen", 0x3CB371),
    ("mediumslateblue", 0x7B68EE),
    ("mediumspringgreen", 0x00FA9A),
    ("mediumturquoise", 0x48D1CC),
    ("mediumvioletred", 0xC71585),
    ("midnightblue", 0x191970),
    ("mintcream", 0xF5FFFA),
    ("mistyrose", 0xFFE4E1),
    ("moccasin", 0xFFE4B5),
    ("navajowhite", 0xFFDEAD),
    ("navy", 0x000080),
    ("oldlace", 0xFDF5E6),
    ("olive", 0x808000),
    ("olivedrab", 0x6B8E23),
    ("orange", 0xFFA500),
    ("orangered", 0xFF4500),
    ("orchid", 0xDA70D6),
    ("palegoldenrod", 0xEEE8AA),
    ("palegreen", 0x98FB98),
    ("paleturquoise", 0xAFEEEE),
    ("palevioletred", 0xDB7093),
    ("papayawhip", 0xFFEFD5),
    ("peachpuff", 0xFFDAB9),
    ("peru", 0xCD853F),
    ("pink", 0xFFC0CB),
    ("plum", 0xDDA0DD),
    ("powderblue", 0xB0E0E6),
    ("purple", 0x800080),
    ("rebeccapurple", 0x663399),
    ("red", 0xFF0000),
    ("rosybrown", 0xBC8F8F),
    ("royalblue", 0x4169E1),
    ("saddlebrown", 0x8B4513),
    ("salmon", 0xFA8072),
    ("sandybrown", 0xF4A460),
    ("seagreen", 0x2E8B57),
    ("seashell", 0xFFF5EE),
    ("sienna", 0xA0522D),
    ("silver", 0xC0C0C0),
    ("skyblue", 0x87CEEB),
    ("slateblue", 0x6A5ACD),
    ("slategray", 0x708090),
    ("slategrey", 0x708090),
    ("snow", 0xFFFAFA),
    ("springgreen", 0x00FF7F),
    ("steelblue", 0x4682B4),
    ("tan", 0xD2B48C),
    ("teal", 0x008080),
    ("thistle", 0xD8BFD8),
    ("tomato", 0xFF6347),
    ("turquoise", 0x40E0D0),
    ("violet", 0xEE82EE),
    ("wheat", 0xF5DEB3),
    ("white", 0xFFFFFF),
    ("whitesmoke", 0xF5F5F5),
    ("yellow", 0xFFFF00),
    ("yellowgreen", 0x9ACD32),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_colors_are_sorted() {
        assert!(NAMED_COLORS.windows(2).all(|w| w[0].0 < w[1].0));
        assert_eq!(NAMED_COLORS.len(), 148);
    }

    #[test]
    fn lookup() {
        assert_eq!(
            named_color("RebeccaPurple"),
            Some(Rgba::rgb(0x66, 0x33, 0x99))
        );
        assert_eq!(named_color("transparent"), Some(Rgba::TRANSPARENT));
        assert_eq!(named_color("nope"), None);
        assert_eq!(system_color("LinkText"), Some(Rgba::rgb(0, 0, 0xEE)));
    }

    #[test]
    fn current_color_resolves() {
        let red = Rgba::rgb(255, 0, 0);
        assert_eq!(Color::CurrentColor.resolve(red), red);
        assert_eq!(Color::Rgba(Rgba::WHITE).resolve(red), Rgba::WHITE);
    }
}
