//! The text of generated content (`content` on `::before`, `::after` and
//! `::marker`).

use crate::ComputedStyle;
use crate::values::{Content, ContentItem};

/// The text of the `content` property for a pseudo-element: the strings
/// concatenated, `None` for `normal` and `none`.
///
/// `attr()` is already replaced by the attribute value in the computed
/// style, and `counter()` and `counters()` by their text in the styles of
/// a [`crate::StyleMap`] (see `counters.rs`). `open-quote` and
/// `close-quote` produce English double quotes (the `quotes` property and
/// quote nesting are not supported). Images, and counters in a style that
/// is not part of a style map, produce no text.
pub fn content_text(style: &ComputedStyle) -> Option<String> {
    let Content::Items(items) = &style.content else {
        return None;
    };
    let mut text = String::new();
    for item in items.iter() {
        match item {
            ContentItem::String(s) => text.push_str(s),
            ContentItem::OpenQuote => text.push('\u{201C}'),
            ContentItem::CloseQuote => text.push('\u{201D}'),
            ContentItem::Counter { .. } | ContentItem::Image(_) => {}
        }
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn text_of_items() {
        let mut style = (*ComputedStyle::initial()).clone();
        assert_eq!(content_text(&style), None);
        style.content = Content::None;
        assert_eq!(content_text(&style), None);
        style.content = Content::Items(Arc::from([
            ContentItem::OpenQuote,
            ContentItem::String("a".into()),
            ContentItem::Counter {
                name: "x".into(),
                separator: None,
                style: crate::values::ListStyleType::Decimal,
            },
            ContentItem::String("b".into()),
            ContentItem::CloseQuote,
        ]));
        assert_eq!(content_text(&style).as_deref(), Some("\u{201C}ab\u{201D}"));
    }
}
