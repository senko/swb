//! `link rel=modulepreload` (HTML "link type "modulepreload"",
//! <https://html.spec.whatwg.org/multipage/links.html#link-type-modulepreload>;
//! ADR 0034 section 9), with JavaScript on only: one fetch of the `href` URL
//! (a URL, not a module specifier) into the module map, with the `link`'s
//! `crossorigin`, without descendants, parse or run. The `link` gets `load`
//! or `error` in a task after the fetch. The fetch does not delay the
//! window's `load` event; a later graph that needs the module uses the entry
//! (`module_map::preload`).
//!
//! The steps run when a `link` is inserted into the document (a script's
//! insertion: `after_insertion`; the parser's: [`parser_links`]) and when its
//! `href` is set or its `rel` changes (measured, case `modulepreload-script`:
//! a change of `crossorigin`, or a `rel` set to the same value, does nothing; a
//! move of the element fires the event again, with no new request).
//!
//! Departures from Chromium (measured, case `modulepreload-as`): a `link`
//! with an `as` other than `script` (Chromium: `image`, `worker`) gets
//! `error` without a request; Chromium makes the request and fires `error`.

use swb_dom::{ElementData, NodeId, local_name};
use swb_js::{Runtime, VmResult};

use super::{Phase, fire_at_element, module_map, scripts};
use crate::event_loop::{Task, queue_task};
use crate::state::{dom, engine_mut, state};

/// True for an HTML `link` element whose `rel` has the token `modulepreload`
/// (ASCII case-insensitive).
pub(super) fn is_modulepreload(element: &ElementData) -> bool {
    element.is_html()
        && *element.local_name() == local_name!("link")
        && element.attr("rel").is_some_and(|rel| {
            rel.split_ascii_whitespace()
                .any(|token| token.eq_ignore_ascii_case("modulepreload"))
        })
}

/// "Process the `link`" for a `modulepreload` `link` element that is in the
/// document: the module fetch and the event.
pub(crate) fn process(rt: &mut Runtime, node: NodeId) -> VmResult<()> {
    if state(rt)?.doc.scripts_off || scripts(rt)?.phase == Phase::Unloaded {
        return Ok(());
    }
    let dom = dom(rt)?;
    let Some(element) = dom.element(node) else {
        return Ok(());
    };
    if !is_modulepreload(element)
        || !dom.is_connected(node)
        || dom.node_document(node) != NodeId::DOCUMENT
    {
        return Ok(());
    }
    // An empty `href`, or one that is not a URL, does nothing (measured).
    let Some(href) = element.attr("href").filter(|href| !href.is_empty()) else {
        return Ok(());
    };
    let href = href.to_owned();
    let destination_ok = element
        .attr("as")
        .is_none_or(|value| value.is_empty() || value.eq_ignore_ascii_case("script"));
    let credentials = element
        .attr("crossorigin")
        .is_some_and(|value| value.eq_ignore_ascii_case("use-credentials"));
    if !destination_ok {
        return queue_task(rt, Task::LinkEvent { node, error: true });
    }
    let Some(url) = engine_mut(rt)?.parse_url(&href) else {
        return Ok(());
    };
    module_map::preload(rt, &url, node, credentials)
}

/// Processes the `modulepreload` links that the parser of `document.write`
/// text inserted in its last step.
pub(crate) fn written_links(rt: &mut Runtime) -> VmResult<()> {
    for node in engine_mut(rt)?.take_parser_links() {
        process(rt, node)?;
    }
    Ok(())
}

/// [`Task::LinkEvent`]: `load` or `error` at the `link` element.
pub(crate) fn fire_link_event(rt: &mut Runtime, node: NodeId, error: bool) -> VmResult<()> {
    fire_at_element(rt, node, if error { "error" } else { "load" })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links(html: &str) -> Vec<bool> {
        let dom = swb_dom::parse_html_with(html, true);
        dom.descendants(NodeId::DOCUMENT)
            .filter_map(|n| dom.element(n))
            .filter(|e| *e.local_name() == local_name!("link"))
            .map(is_modulepreload)
            .collect()
    }

    #[test]
    fn the_rel_tokens_decide() {
        assert_eq!(
            links(
                "<link rel=modulepreload><link rel='preload  ModulePreload'>\
                 <link rel=preload><link rel=modulepreloads><link>"
            ),
            [true, true, false, false, false]
        );
    }
}
