//! The HTML fragment parsing algorithm with a context element
//! (`innerHTML`, `outerHTML`, `insertAdjacentHTML`,
//! `createContextualFragment`) and the parse of a whole document into a
//! new document of the arena (`DOMParser`).
//! <https://html.spec.whatwg.org/multipage/parsing.html#parsing-html-fragments>
//!
//! html5ever parses into a temporary arena, as the algorithm does with its
//! new `Document`; the nodes then move into the target arena (with new
//! ids) under a new document fragment (or document), and their insertion is
//! journaled.

use std::rc::Rc;

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{BufferQueue, Tokenizer, TokenizerOpts};
use html5ever::tree_builder::{
    QuirksMode as HtmlQuirksMode, TreeBuilder, TreeBuilderOpts, create_element,
};
use html5ever::{QualName, TokenizerResult, local_name, ns};

use super::sink::Sink;
use super::{HtmlParser, ParseStep, push_text};
use crate::mutation::{DomError, element_bytes, name_bytes};
use crate::tree::{Document, Limits, NodeData, NodeId, QuirksMode};

/// Nodes of the temporary arena that are not copied: its document node,
/// the `html` root, the context element, a form element, a template
/// contents owner.
const TEMPORARY_NODES: usize = 8;

/// Parses `markup` as a fragment in the context of the element `context`
/// and returns a new document fragment of `doc` (whose node document is
/// the context's) with the resulting nodes. The fragment is detached: the
/// caller inserts its children where they belong. A caller that keeps the
/// fragment from scripts gives it back with [`Document::recycle_fragment`]
/// after the insertion, so that the next parse reuses the node.
///
/// The parser's scripting flag is that of the context
/// ([`Document::scripting_for`]): the content of a `noscript` is markup in
/// the documents that scripts make. The sink of the parser (its
/// `mark_script_already_started`, which the tree builder calls for each
/// script of a fragment) marks the `script` elements "already started",
/// and the copy into `doc` keeps the flag. Elements
/// in the fragment get no form element pointer; their form owner comes
/// from their ancestors when they are inserted.
///
/// # Errors
///
/// [`DomError::InvalidNodeType`] if `context` is not an element;
/// [`DomError::NodeLimit`] or [`DomError::ByteLimit`] if the nodes do not
/// fit the limits of `doc`.
pub fn parse_fragment(
    doc: &mut Document,
    context: NodeId,
    markup: &str,
) -> Result<NodeId, DomError> {
    let temp = parse_into_temporary(doc, context, markup)?;
    doc.check_room(temp.len(), temp.text_bytes())?;
    let owner = doc.node_document(context);
    let fragment = doc
        .take_scratch_fragment(owner)
        .unwrap_or_else(|| doc.push_node(NodeData::DocumentFragment, owner));
    let Some(root) = temp.document_element() else {
        return Ok(fragment);
    };
    let top: Vec<NodeId> = temp.children(root).collect();
    for node in top {
        let copy = copy_subtree(doc, &temp, node, owner, false);
        doc.insert(copy, fragment, None, false);
    }
    Ok(fragment)
}

/// Parses `markup` as a whole HTML document (scripting off, as the
/// `DOMParser` of `text/html`) into a new document node of `doc` and
/// returns it. The scripts of the new document are marked "already
/// started": they never run, also not after a move into another document.
/// The quirks mode of the new document is the parser's.
///
/// # Errors
///
/// [`DomError::NodeLimit`] or [`DomError::ByteLimit`] if the nodes do not
/// fit the limits of `doc`.
pub fn parse_document(doc: &mut Document, markup: &str) -> Result<NodeId, DomError> {
    let limits = doc.limits();
    let mut temp = Document::with_limits(Limits {
        max_nodes: limits.max_nodes.saturating_sub(doc.len()) + TEMPORARY_NODES,
        max_text_bytes: limits.max_text_bytes.saturating_sub(doc.text_bytes()),
    });
    temp.set_journaling(false);
    let mut parser = HtmlParser::new(&mut temp, markup, false);
    while let Ok(ParseStep::Script(_)) = parser.resume(&mut temp) {}
    if parser.stopped_at_limit() {
        return Err(limit_error(&temp, limits));
    }
    doc.check_room(temp.len(), temp.text_bytes())?;
    let document = doc.create_document()?;
    doc.set_other_quirks_mode(document, temp.quirks_mode);
    let top: Vec<NodeId> = temp.children(NodeId::DOCUMENT).collect();
    for node in top {
        let copy = copy_subtree(doc, &temp, node, document, true);
        doc.insert(copy, document, None, false);
    }
    Ok(document)
}

/// Runs html5ever's fragment parser into a new arena whose limits are the
/// room left in `doc`.
fn parse_into_temporary(
    doc: &Document,
    context: NodeId,
    markup: &str,
) -> Result<Document, DomError> {
    let element = doc.element(context).ok_or(DomError::InvalidNodeType)?;
    let name = element.name.clone();
    let attrs: Vec<html5ever::Attribute> = element
        .attributes()
        .iter()
        .map(|a| html5ever::Attribute {
            name: a.name.clone(),
            value: StrTendril::from_slice(&a.value),
        })
        .collect();
    // The context element and the stand-in form count in the temporary
    // arena, but they are not copied.
    let form_name = QualName::new(None, ns!(html), local_name!("form"));
    let context_bytes = element_bytes(element) + name_bytes(&form_name);
    // The form element pointer is set if the context is in a form (step 9
    // of the algorithm); a stand-in form element keeps the tree builder's
    // rules for nested forms.
    let in_form = std::iter::once(context)
        .chain(doc.ancestors(context))
        .any(|n| doc.is_html_element(n, &local_name!("form")));
    let quirks = doc.quirks_mode_of(doc.node_document(context));
    // The scripting flag of the context's document, except for a `template`
    // context: Chromium parses its contents as the contents' document does
    // (scripting off; measured in `tools/probes/web-dom-query.json`).
    let scripting =
        doc.scripting_for(context) && !doc.is_html_element(context, &local_name!("template"));
    let limits = doc.limits();
    let mut temp = Document::with_limits(Limits {
        max_nodes: limits.max_nodes.saturating_sub(doc.len()) + TEMPORARY_NODES,
        max_text_bytes: limits.max_text_bytes.saturating_sub(doc.text_bytes()) + context_bytes,
    });
    // The copy into `doc` is journaled; the temporary arena is not.
    temp.set_journaling(false);
    let input = Rc::new(BufferQueue::default());
    push_text(&input, markup);
    let sink = Sink::new(temp, Rc::clone(&input));
    let context_element = create_element(&sink, name, attrs);
    let form = in_form.then(|| create_element(&sink, form_name, vec![]));
    let opts = TreeBuilderOpts {
        scripting_enabled: scripting,
        quirks_mode: match quirks {
            QuirksMode::NoQuirks => HtmlQuirksMode::NoQuirks,
            QuirksMode::LimitedQuirks => HtmlQuirksMode::LimitedQuirks,
            QuirksMode::Quirks => HtmlQuirksMode::Quirks,
        },
        ..Default::default()
    };
    let tree_builder = TreeBuilder::new_for_fragment(sink, context_element, form, opts);
    let state = tree_builder.tokenizer_state_for_context_elem(scripting);
    let tokenizer = Tokenizer::new(
        tree_builder,
        TokenizerOpts {
            initial_state: Some(state),
            ..Default::default()
        },
    );
    // Scripts in fragments never run, so a script end tag does not pause.
    while !matches!(tokenizer.feed(&input), TokenizerResult::Done) {}
    tokenizer.end();
    let stopped = tokenizer.sink.sink.stopped.get();
    let temp = tokenizer.sink.sink.doc.into_inner();
    if stopped {
        return Err(limit_error(&temp, limits));
    }
    Ok(temp)
}

/// The error for a parse that stopped at a limit of its temporary arena.
fn limit_error(temp: &Document, limits: Limits) -> DomError {
    if temp.len() >= temp.limits().max_nodes {
        DomError::NodeLimit(limits.max_nodes)
    } else {
        DomError::ByteLimit(limits.max_text_bytes)
    }
}

/// Copies the subtree of `src` in `temp` (with template contents) into
/// `doc` as a detached subtree whose node document is `owner`. Iterative.
/// With `started`, every `script` element gets "already started".
fn copy_subtree(
    doc: &mut Document,
    temp: &Document,
    src: NodeId,
    owner: NodeId,
    started: bool,
) -> NodeId {
    let copy = |doc: &mut Document, src: NodeId, owner: NodeId| {
        let mut data = temp.node(src).data.clone();
        if let NodeData::Element(e) = &mut data {
            e.template_contents = None;
            e.parser_form = None;
            if started && e.is_html_named(&local_name!("script")) {
                e.script_already_started = true;
            }
        }
        doc.push_node(data, owner)
    };
    let root = copy(doc, src, owner);
    // (source node, its copy): the children of each are copied next.
    let mut stack = vec![(src, root)];
    while let Some((src, dst)) = stack.pop() {
        // Template contents go into the contents of the copy.
        let contents = temp
            .element(src)
            .and_then(|e| e.template_contents)
            .zip(doc.element(dst).and_then(|e| e.template_contents));
        for (from, to) in std::iter::once((src, dst)).chain(contents) {
            let owner = doc.node_document(to);
            for child in temp.children(from) {
                let child_copy = copy(doc, child, owner);
                doc.link(to, child_copy, None);
                stack.push((child, child_copy));
            }
        }
    }
    root
}
