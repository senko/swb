//! Escaping of text for generated HTML pages (directory listings and
//! error pages).

/// Escapes `&`, `<`, `>` and `"` so that `text` is safe in HTML text and in
/// double-quoted attribute values.
pub fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            _ => escaped.push(c),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_markup_characters() {
        assert_eq!(
            escape_html("a&b <c> \"d\""),
            "a&amp;b &lt;c&gt; &quot;d&quot;"
        );
        assert_eq!(escape_html("plain"), "plain");
    }
}
