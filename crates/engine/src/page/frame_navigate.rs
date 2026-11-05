//! Navigation by the user's input in documents with frames (ADR 0031
//! sections 2, 3 and 8; M9 feature 11): a link or a form of a document
//! navigates the browsing context that its `target` names (HTML "rules for
//! choosing a navigable",
//! <https://html.spec.whatwg.org/multipage/document-sequences.html#the-rules-for-choosing-a-navigable>):
//! the document's own frame (`_self`, no target), the parent (`_parent`), the
//! top level (`_top`), or the frame that has the name.
//!
//! swb has one tab, so a `_blank` target or a name that no frame has opens
//! nothing new: the navigation goes to the frame of the document that asked
//! (the top-level document ignored `target` before; Chromium opens a new tab
//! and leaves the frame as it is), and a sandboxed frame without
//! `allow-popups` does nothing (measured). A name is compared exactly; the
//! keywords without ASCII case (measured). The rule that a document may
//! navigate a frame is in [`Page::may_navigate`]. Chromium lets a cross-origin
//! frame navigate the top level after a user activation (measured, case
//! `input-link-cross-origin-top`); swb does not track activation and refuses,
//! with a log entry (ADR 0031 section 3). A navigation of a frame replaces its
//! entry in the session history (ADR 0031 section 2); one to a fragment of the
//! frame's own document scrolls it.

use swb_net::{Origin, Url};

use super::{HistoryHandling, Page, is_loadable, strip_fragment};
use crate::document::DocumentState;
use crate::frames::{FrameId, FrameTarget, Sandbox};
use crate::history::PostData;

/// A navigation that a link or form asked for.
#[derive(Clone, Debug)]
pub(super) struct FrameNavigation {
    /// The frame whose document has the link or form.
    pub(super) source: FrameId,
    pub(super) url: Url,
    /// The body of a `POST` submission.
    pub(super) post: Option<PostData>,
    /// The `target` that the link or form asks for.
    pub(super) target: Option<String>,
}

/// The browsing context that a navigation goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Chosen {
    /// This frame ([`FrameId::TOP`]: the page's own document).
    Frame(FrameId),
    /// The navigation does not happen.
    Refused,
}

impl Page {
    /// A link followed or a form submitted by the user (or by a script's
    /// `click()`) in the active document, with the `target` that it asks
    /// for. In the page's own document it navigates now; in a frame's
    /// document it waits until the frame's document is back in its place
    /// ([`Page::run_deferred_navigations`]). Returns true if the
    /// navigation was started (or, in a frame, will be tried).
    pub(super) fn navigate_from_input(
        &mut self,
        url: Url,
        post: Option<PostData>,
        target: Option<String>,
    ) -> bool {
        let navigation = FrameNavigation {
            source: self.active_frame,
            url,
            post,
            target,
        };
        if self.active_frame == FrameId::TOP {
            return self.navigate_as_asked(navigation);
        }
        self.deferred_navigations.push(navigation);
        true
    }

    /// Carries out the navigations that the input in a frame asked for.
    pub(super) fn run_deferred_navigations(&mut self) {
        if self.active_frame != FrameId::TOP {
            return;
        }
        for navigation in std::mem::take(&mut self.deferred_navigations) {
            self.navigate_as_asked(navigation);
        }
    }

    /// Navigates as the link or form asks (see the module documentation).
    fn navigate_as_asked(&mut self, navigation: FrameNavigation) -> bool {
        let FrameNavigation {
            source,
            url,
            post,
            target,
        } = navigation;
        let Chosen::Frame(chosen) = self.choose_navigable(source, target.as_deref()) else {
            return false;
        };
        let initiator = self.doc_of(source).map(DocumentState::origin);
        if chosen == FrameId::TOP {
            return self.navigate_top_as(&url, post, initiator);
        }
        self.navigate_frame_as(chosen, url, post, initiator)
    }

    /// A navigation of the top-level document by a link or form.
    fn navigate_top_as(
        &mut self,
        url: &Url,
        post: Option<PostData>,
        initiator: Option<Origin>,
    ) -> bool {
        let Some(post) = post else {
            return self.follow_link(url.clone());
        };
        if !is_loadable(url) || !self.may_load_subresource(url) {
            log::warn!("not allowed to submit a form to {url}");
            return false;
        }
        self.start_navigation(
            url.clone(),
            Some(post),
            HistoryHandling::Push,
            None,
            initiator,
        );
        true
    }

    /// A navigation of frame `id` by a link or form: to a fragment of its
    /// document it scrolls, else the frame loads the URL (a `POST` with its
    /// body).
    fn navigate_frame_as(
        &mut self,
        id: FrameId,
        url: Url,
        post: Option<PostData>,
        initiator: Option<Origin>,
    ) -> bool {
        if !is_loadable(&url) {
            log::info!("{id}: ignoring a link with the scheme {}:", url.scheme());
            return false;
        }
        let same_document = post.is_none()
            && url.fragment().is_some()
            && self
                .doc_of(id)
                .is_some_and(|d| strip_fragment(d.url()) == strip_fragment(&url));
        if same_document {
            return self.navigate_frame_to_fragment(id, url);
        }
        self.navigate_frame_with(id, FrameTarget::Url(url), post, initiator);
        true
    }

    /// HTML "navigate to a fragment" in frame `id`: the document's URL gets
    /// the fragment, the viewport scrolls to the element, and `:target` and
    /// the focus follow. The session history keeps one entry for the frame.
    fn navigate_frame_to_fragment(&mut self, id: FrameId, url: Url) -> bool {
        let Some(fragment) = url.fragment().map(str::to_owned) else {
            return false;
        };
        self.in_frame(id, |p| {
            if let Some(doc) = p.doc.as_deref_mut() {
                doc.set_url(url);
            }
            let target = p.set_target(Some(&fragment));
            p.scroll_to_fragment(&fragment);
            p.focus_fragment_target(target);
        })
        .is_some()
    }

    /// The browsing context that a navigation of `source` with the `target`
    /// name goes to.
    fn choose_navigable(&self, source: FrameId, target: Option<&str>) -> Chosen {
        let name = target.map_or("", str::trim);
        let parent = self.frames.get(source).and_then(|f| f.parent);
        let chosen = if name.is_empty() || name.eq_ignore_ascii_case("_self") {
            source
        } else if name.eq_ignore_ascii_case("_top") {
            FrameId::TOP
        } else if name.eq_ignore_ascii_case("_parent") {
            parent.unwrap_or(source)
        } else if name.eq_ignore_ascii_case("_blank") {
            return self.new_context_for(source);
        } else {
            return self.frame_named(source, name);
        };
        if self.may_navigate(source, chosen) {
            Chosen::Frame(chosen)
        } else {
            log::info!("{source}: not allowed to navigate {chosen} (target {name:?})");
            Chosen::Refused
        }
    }

    /// The first frame, in tree order, that has the `name` and that `source`
    /// may navigate (measured, case `input-target-name-first-refused`); if
    /// no frame has the name, as for a new browsing context.
    fn frame_named(&self, source: FrameId, name: &str) -> Chosen {
        let named: Vec<FrameId> = self
            .frames
            .tree_order()
            .into_iter()
            .skip(1)
            .filter(|&id| self.frames.get(id).is_some_and(|f| f.name == name))
            .collect();
        if named.is_empty() {
            return self.new_context_for(source);
        }
        if let Some(&id) = named.iter().find(|&&id| self.may_navigate(source, id)) {
            Chosen::Frame(id)
        } else {
            log::info!("{source}: not allowed to navigate the frame named {name:?}");
            Chosen::Refused
        }
    }

    /// A link or form of `source` asks for a new browsing context: swb has
    /// one tab, so the navigation goes to `source`. A sandbox without
    /// `allow-popups` opens nothing (measured, case `input-target-sb-blank`).
    fn new_context_for(&self, source: FrameId) -> Chosen {
        if self.sandbox_of(source).popups_allowed() {
            Chosen::Frame(source)
        } else {
            log::info!("{source}: the sandbox has no `allow-popups`: not navigating");
            Chosen::Refused
        }
    }

    /// The sandboxing flags of frame `id` (none for the page's own document).
    pub(super) fn sandbox_of(&self, id: FrameId) -> Sandbox {
        self.frames.get(id).map(|f| f.sandbox).unwrap_or_default()
    }

    /// True if the document of `source` may navigate frame `target` (HTML
    /// "allowed by sandboxing to navigate", and the origin rule of Chromium;
    /// measured, cases `input-target-*`):
    ///
    /// - itself and its descendants, always;
    /// - a sandboxed frame: else only the top level, with an
    ///   `allow-top-navigation` token (also from a nested frame, and the
    ///   flags go down to frames without `sandbox`);
    /// - else a frame whose document, or the document of one of whose
    ///   ancestors, has the origin of `source`. So a frame with the top
    ///   level's origin may navigate any frame, and a cross-origin frame may
    ///   not navigate its parent.
    fn may_navigate(&self, source: FrameId, target: FrameId) -> bool {
        let chain = self.frame_chain(target);
        if chain.contains(&source) {
            return true;
        }
        let sandbox = self.sandbox_of(source);
        if sandbox.sandboxed {
            return target == FrameId::TOP && sandbox.top_navigation_allowed();
        }
        let Some(origin) = self.doc_of(source).map(DocumentState::origin) else {
            return false;
        };
        chain
            .iter()
            .any(|&id| self.doc_of(id).is_some_and(|d| d.origin() == origin))
    }
}
