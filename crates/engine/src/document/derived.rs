//! Data derived from the DOM, keyed by the DOM generation (ADR 0028
//! section 3): a [`Derived`] value records the generation it was built
//! for, and is rebuilt when the document's generation differs. The drain
//! of the journal can also confirm a value for a new generation without a
//! rebuild, when no change of the drained batch can affect it (a change of
//! a `class` does not change the title).
//!
//! The values: the document base URL and the title (kept current by the
//! drain, because the page reads them without `&mut`), the form controls
//! and the tree order (rebuilt when they are used).

use swb_dom::{Document, NodeId, local_name};
use swb_net::Url;

/// A value derived from the DOM and the generation it is valid for.
#[derive(Debug)]
pub(crate) struct Derived<T> {
    value: T,
    /// `None` before the first build.
    generation: Option<u64>,
}

impl<T> Derived<T> {
    /// A value that is not valid for any generation yet.
    pub(crate) fn new(value: T) -> Self {
        Derived {
            value,
            generation: None,
        }
    }

    /// True if the value was built (or confirmed) for `generation`.
    pub(crate) fn is_current(&self, generation: u64) -> bool {
        self.generation == Some(generation)
    }

    /// True if the value was never built.
    pub(crate) fn is_unbuilt(&self) -> bool {
        self.generation.is_none()
    }

    /// The value, whatever generation it was built for.
    pub(crate) fn value(&self) -> &T {
        &self.value
    }

    /// The value, for changes that keep it valid (user input into form
    /// controls).
    pub(crate) fn value_mut(&mut self) -> &mut T {
        &mut self.value
    }

    /// The value for `generation`, built again with `build` if it was
    /// built for another one.
    pub(crate) fn get(&mut self, generation: u64, build: impl FnOnce() -> T) -> &T {
        if !self.is_current(generation) {
            self.value = build();
            self.generation = Some(generation);
        }
        &self.value
    }

    /// Builds the value again from the old one, for `generation`.
    pub(crate) fn rebuild(&mut self, generation: u64, build: impl FnOnce(&T) -> T) {
        self.value = build(&self.value);
        self.generation = Some(generation);
    }

    /// Marks a built value as valid for `generation` without a rebuild:
    /// the changes since it was built cannot affect it.
    pub(crate) fn confirm(&mut self, generation: u64) {
        if self.generation.is_some() {
            self.generation = Some(generation);
        }
    }
}

/// The document base URL: the `href` of the first `<base>` element that
/// has one, resolved against the document URL
/// (<https://html.spec.whatwg.org/multipage/urls-and-fetching.html#document-base-url>).
pub(crate) fn base_url(doc: &Document, url: &Url) -> Url {
    doc.find_element(NodeId::DOCUMENT, |e| {
        e.is_html_named(&local_name!("base")) && e.has_attr("href")
    })
    .and_then(|base| doc.element(base)?.attr("href"))
    .and_then(|href| url.join(href.trim()).ok())
    .unwrap_or_else(|| url.clone())
}

/// The document title: the text of the `<title>` element, with ASCII
/// whitespace stripped and collapsed
/// (<https://html.spec.whatwg.org/multipage/dom.html#document.title>).
pub(crate) fn document_title(doc: &Document) -> String {
    let title = doc
        .head()
        .and_then(|head| {
            doc.element_children(head)
                .find(|&c| doc.is_html_element(c, &local_name!("title")))
        })
        .or_else(|| doc.find_element(NodeId::DOCUMENT, |e| e.is_html_named(&local_name!("title"))));
    title.map_or_else(String::new, |t| {
        doc.text_content(t)
            .split_ascii_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_is_rebuilt_when_the_generation_differs() {
        let mut builds = 0;
        let mut derived = Derived::new(0);
        assert!(derived.is_unbuilt());
        assert_eq!(
            *derived.get(1, || {
                builds += 1;
                10
            }),
            10
        );
        assert_eq!(*derived.get(1, || unreachable!("current")), 10);
        assert_eq!(
            *derived.get(2, || {
                builds += 1;
                20
            }),
            20
        );
        assert_eq!(builds, 2);
        // A confirmed value is current for the new generation.
        derived.confirm(3);
        assert!(derived.is_current(3));
        assert_eq!(*derived.get(3, || unreachable!("confirmed")), 20);
        derived.rebuild(4, |old| old + 1);
        assert_eq!((*derived.value(), derived.is_current(4)), (21, true));
    }

    #[test]
    fn an_unbuilt_value_is_not_confirmed() {
        let mut derived = Derived::new(String::new());
        derived.confirm(5);
        assert!(!derived.is_current(5) && derived.is_unbuilt());
    }

    #[test]
    fn title_and_base_url_follow_the_dom() {
        let url = Url::parse("https://a.test/dir/page").unwrap();
        let mut doc = swb_dom::parse_html("<title>  A \n b </title><base href='../x/'>");
        assert_eq!(document_title(&doc), "A b");
        assert_eq!(base_url(&doc, &url).as_str(), "https://a.test/x/");
        let base = doc.find_element(NodeId::DOCUMENT, |e| e.is_html_named(&local_name!("base")));
        doc.remove(base.unwrap());
        assert_eq!(base_url(&doc, &url), url);
        let title = doc.find_element(NodeId::DOCUMENT, |e| e.is_html_named(&local_name!("title")));
        let text = doc.children(title.unwrap()).next().unwrap();
        doc.set_data(text, "New").unwrap();
        assert_eq!(document_title(&doc), "New");
    }
}
