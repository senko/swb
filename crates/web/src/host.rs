//! What scripts need from the engine (ADR 0028 section 1): the trait
//! [`EngineHost`], which the engine implements for its document state.
//!
//! `web` does not depend on `engine`. The engine lends its boxed document
//! state to the runtime for each task ([`crate::ScriptRuntime::lend`]) as a
//! `Box<dyn EngineHost>` and takes it back afterwards
//! ([`crate::ScriptRuntime::take_back`]); [`EngineHost::into_any`] turns the
//! box back into the engine's type. Features 7 and later add the style and
//! layout queries here.

use std::any::Any;

use swb_dom::{Document, NodeId, ParseStep, TextPosition};

/// The document of a task, as the engine lends it to the bindings.
pub trait EngineHost: Any {
    /// The DOM of the document.
    fn dom(&self) -> &Document;

    /// The DOM, for changes through the change functions of `swb_dom`. The
    /// engine processes the changes when the state comes back (the journal
    /// drain of ADR 0028 section 3).
    fn dom_mut(&mut self) -> &mut Document;

    /// The document URL, serialized (`document.URL`).
    fn url(&self) -> String;

    /// The document base URL, serialized (`baseURI`). It reflects the
    /// `base` elements of the DOM as it is now, also after changes of this
    /// task.
    fn base_url(&mut self) -> String;

    /// `value` parsed as a URL relative to the document base URL and
    /// serialized, or `None` if it does not parse (`[ReflectURL]`).
    fn parse_url(&mut self, value: &str) -> Option<String>;

    /// The name of the document's character encoding (`characterSet`,
    /// for example `UTF-8` or `windows-1252`).
    fn character_set(&self) -> &'static str;

    /// The content type of the document (`text/html`, `text/plain`,
    /// `image/png`).
    fn content_type(&self) -> &str;

    /// One step of the document's parser (feature 5c): it parses until the
    /// end tag of a `script` element (with the position of the script's
    /// text in the source) or to the end of the input, and processes the
    /// changes. `None` if the document has no parser (it is complete; a
    /// [`ParseStep::Finished`] drops the parser).
    fn parse_step(&mut self) -> Option<(ParseStep, TextPosition)> {
        None
    }

    /// True if the document has a style sheet that is blocking scripts
    /// (HTML §4.2.4): a style sheet of the parser, for the current media,
    /// that has not loaded yet.
    fn style_sheets_block_scripts(&mut self) -> bool {
        false
    }

    /// Starts the fetch of the classic script at `url` (absolute, from
    /// [`EngineHost::parse_url`]) for the script element `node`; the
    /// engine hands the result to [`crate::ScriptRuntime::script_fetched`].
    /// False if the document may not load the URL (the fetch fails at
    /// once).
    fn fetch_script(&mut self, node: NodeId, url: &str) -> bool {
        let _ = (node, url);
        false
    }

    /// The box as `Any`, so that the engine gets its own type back.
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
}
