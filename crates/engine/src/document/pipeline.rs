//! The rendering pipeline of a document: style, layout, display list and
//! raster, with per-stage timings, and their invalidation (ADR 0028
//! section 9). Each stage runs when something needs it (a frame, a query)
//! and its input changed; a flush after no change does nothing. A query of
//! a script that needs the style of one element while the styles are
//! outdated computes that element and its ancestors only
//! ([`DocumentState::query_element_style`]). The runs are counted in
//! [`DocumentState::flushes`], those that script queries force apart.
//!
//! After a change of the element states or of the DOM the styles are not
//! dropped but restyled in part (`run_targeted`: ADR 0041 for states, ADR
//! 0042 for DOM changes): after DOM changes the old map waits as
//! `stale_styles` and the layout as `kept_layout` (`invalidate_styles_for_dom`,
//! `set_layout_aside`, `restore_layout`), and the next style computation
//! restyles only what the recorded changes affect, or falls back to a full
//! restyle.

use std::time::Instant;

use swb_css::{MediaEnvironment, MediaQueryList};
use swb_layout::{BoxContent, FragmentRef, LayoutInput, Point, Rect};
use swb_net::Url;
use swb_paint::{DisplayList, NoHighlights, Pixmap, RasterParams, Scrolling};
use swb_style::Stylist;
use swb_text::FontContext;

use super::{DocumentState, ScrollTarget};
use crate::forms::{ControlType, InputType, LayoutControls, display_offset};
use crate::frames::paint::FrameSource;
use crate::frames::{FrameId, Frames};
use crate::page::RestylePaths;
use crate::selection::Highlight;

/// The most layout passes of one [`DocumentState::update_layout`]: web
/// fonts that are already loaded make layout run again.
const MAX_LAYOUT_PASSES: usize = 4;

/// The most extra style and layout runs of one
/// [`DocumentState::update_layout`] for container queries (ADR 0038
/// section 4): N nested containers whose queries size the next container
/// need N + 1 layouts; at the bound the last styles and their layout stay,
/// and the next update goes on from the sizes of that layout. The font loop
/// ([`MAX_LAYOUT_PASSES`]) runs inside each of these runs, so one update
/// does at most (1 + `MAX_CONTAINER_PASSES`) * `MAX_LAYOUT_PASSES` = 20
/// layouts and 1 + `MAX_CONTAINER_PASSES` = 5 style runs. The bounds stay
/// apart: the font loop repeats only when a loaded font file was reused
/// by another face, a container pass only when a query result changes; a
/// shared bound would let one of them starve the other, and the worst case
/// needs a page that makes both loop.
pub(crate) const MAX_CONTAINER_PASSES: u32 = 4;

/// The content boxes of the query containers of `fragments` whose box size
/// containment applies to (ADR 0038): one walk of the fragment tree.
fn container_sizes(
    fragments: &swb_layout::FragmentTree,
    styles: &swb_style::StyleMap,
) -> swb_style::ContainerSizes {
    let mut sizes = swb_style::ContainerSizes::new();
    fragments.walk(|fragment, _| {
        if let FragmentRef::Box(b) = fragment
            && let Some(node) = b.node
            && b.pseudo.is_none()
            && let Some(style) = styles.get(node)
            && style.is_query_container()
            && style.size_containment_applies()
        {
            let content = b.content_rect();
            sizes.insert(node, content.width, content.height);
        }
    });
    sizes
}

/// The change that a targeted restyle follows, which decides its counters,
/// its log text and the follow-up of the layout.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RestylePath {
    /// A change of the element states (ADR 0041).
    State,
    /// DOM changes, and the state changes since the styles (ADR 0042).
    Dom,
}

impl RestylePath {
    /// The cause as the debug log words it.
    fn cause(self) -> &'static str {
        match self {
            RestylePath::State => "a state change",
            RestylePath::Dom => "DOM changes",
        }
    }
}

/// What [`DocumentState::render_in_strips`] draws.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rendering {
    /// The viewport at the document's scroll offset, with the scroll
    /// indicators of the viewport.
    Viewport,
    /// The page from its top-left corner, without the viewport's scroll
    /// indicators (full-page screenshots).
    Page,
}

impl DocumentState {
    /// Drops the stylist, the styles and everything after them (the
    /// stylist is dirty). The parsed sheets stay.
    pub(crate) fn invalidate_style(&mut self) {
        self.stylist = None;
        self.web_fonts.env = None;
        self.invalidate_styles();
    }

    /// Drops the styles and everything after them (the styles are dirty);
    /// the next computation is a full restyle (with the old map's
    /// custom-property maps reused). The old map stays as the stale map and
    /// `styles_full` forbids the targeted restyle; for the targeted case see
    /// [`DocumentState::invalidate_styles_for_dom`].
    pub(crate) fn invalidate_styles(&mut self) {
        self.stash_styles();
        self.styles_full = true;
        self.clear_element_styles();
        self.invalidate_layout();
    }

    /// Makes the styles dirty after the DOM changes recorded in
    /// `dom_changes` (ADR 0042 section 3): the old map is kept for the
    /// targeted restyle, and the layout stays while every change is an
    /// attribute that only the style system reads (section 6).
    pub(crate) fn invalidate_styles_for_dom(&mut self) {
        self.stash_styles();
        self.clear_element_styles();
        if self.styles_full || !self.dom_changes.layout_free() {
            self.invalidate_layout();
        } else {
            self.set_layout_aside();
        }
    }

    /// Invalidates what the DOM changes of one drain affect: everything
    /// from the stylist on if a sheet changed, else the styles for a
    /// targeted restyle if a change is in the document.
    pub(crate) fn invalidate_for_batch(&mut self, stylist_dirty: bool, in_document: bool) {
        if stylist_dirty {
            self.invalidate_style();
        } else if in_document {
            self.invalidate_styles_for_dom();
        } else if self.styles.is_some() {
            // Changes outside the document only, with current styles: nothing
            // for a restyle to know.
            self.dom_changes.clear();
        }
    }

    /// Moves the styles aside as the stale map (the first one of a series
    /// of drains is the one that the recorded changes start from).
    fn stash_styles(&mut self) {
        if let Some(styles) = self.styles.take()
            && self.stale_styles.is_none()
        {
            self.stale_styles = Some(styles);
        }
    }

    /// Moves the layout and the display list aside while a restyle is
    /// pending (ADR 0042 section 6): readers see no layout and flush, and
    /// the restyle puts them back if no style changed
    /// ([`DocumentState::restore_layout`]). Only the first layout of a
    /// series of drains is kept.
    fn set_layout_aside(&mut self) {
        if self.kept_layout.is_some() || self.fragments.is_none() {
            return;
        }
        self.kept_layout = Some(Box::new(super::KeptLayout {
            fragments: self.fragments.take(),
            display_list: self.display_list.take(),
            used_boxes: self.used_boxes.take(),
            geometry: self.geometry.take(),
            client_rects: self.client_rects.take(),
            svg_geometry: self.svg_geometry.take(),
        }));
    }

    /// Puts back the layout that [`DocumentState::set_layout_aside`] kept.
    fn restore_layout(&mut self) {
        if let Some(kept) = self.kept_layout.take()
            && self.fragments.is_none()
        {
            let kept = *kept;
            self.fragments = kept.fragments;
            self.display_list = kept.display_list;
            self.used_boxes = kept.used_boxes;
            self.geometry = kept.geometry;
            self.client_rects = kept.client_rects;
            self.svg_geometry = kept.svg_geometry;
        }
    }

    /// Drops the layout and the display list.
    pub(crate) fn invalidate_layout(&mut self) {
        self.kept_layout = None;
        self.fragments = None;
        self.display_list = None;
        self.used_boxes = None;
        self.geometry = None;
        self.client_rects = None;
        self.svg_geometry = None;
    }

    /// Drops the styles of single elements and pseudo-elements that
    /// queries computed.
    pub(crate) fn clear_element_styles(&mut self) {
        self.element_styles.clear();
        self.pseudo_styles.clear();
    }

    /// Recomputes the styles after a change of the element states, from
    /// the states of the last styles to the current ones: only the elements
    /// that the change can affect (ADR 0041), or the whole document when
    /// the targeted restyle cannot handle the change. The layout stays if no
    /// style changed.
    pub(crate) fn restyle(&mut self) {
        self.sync();
        let Some(mut old) = self.styles.take() else {
            return;
        };
        if let Some(changed) = self.run_targeted(&mut old, RestylePath::State) {
            self.styles = Some(old);
            self.after_targeted(changed, RestylePath::State);
            return;
        }
        self.compute_styles_from(Some(&old));
        if !self
            .styles
            .as_ref()
            .is_some_and(|new| new.same_styles(&old))
        {
            self.invalidate_layout();
        }
    }

    /// The counts of the restyle paths of `path`.
    fn restyle_counts(&mut self, path: RestylePath) -> &mut RestylePaths {
        match path {
            RestylePath::State => &mut self.flushes.state_restyles,
            RestylePath::Dom => &mut self.flushes.dom_restyles,
        }
    }

    /// The targeted restyle of `path` on `map`, the styles for
    /// [`DocumentState::style_states`] (before the recorded DOM changes for
    /// [`RestylePath::Dom`], ADR 0042; ADR 0041 for [`RestylePath::State`]).
    /// `Some(drop_layout)` if it brought `map` up to date for the current
    /// states and DOM (`drop_layout` if a style changed or, after DOM
    /// changes, a change can change the layout), `None` if the whole
    /// document must be restyled (then `map` is unchanged).
    fn run_targeted(&mut self, map: &mut swb_style::StyleMap, path: RestylePath) -> Option<bool> {
        if path == RestylePath::Dom {
            self.refresh_forms();
        }
        let current = self.stylist.is_some() && !self.adopted.out_of_date(&self.dom);
        if !current || self.images.sources_outdated {
            self.restyle_counts(path)
                .fallback(swb_style::FullReason::OtherStates);
            if path == RestylePath::Dom {
                log::debug!("style: full restyle after DOM changes (stylist)");
            }
            return None;
        }
        let env = self.media_environment();
        let stylist = self.stylist.as_ref()?;
        let started = Instant::now();
        let inputs = swb_style::RestyleInputs {
            doc: &self.dom,
            stylist,
            env: &env,
            base_url: self.base_url.value(),
            sizes: &self.container_sizes,
        };
        // The owners of animations whose values can have changed (ADR 0044
        // section 3) are restyled with the change. A fallback to a full
        // restyle restyles them anyway.
        let marked = self.animations.borrow_mut().take_marked();
        let none = swb_style::DomChanges::default();
        let dom = match path {
            RestylePath::State => &none,
            RestylePath::Dom => &self.dom_changes,
        };
        let outcome = swb_style::restyle_animated(
            &inputs,
            &self.style_states,
            &self.states,
            dom,
            &marked,
            Some(&self.animations),
            map,
        );
        let (changed, restyled, image_urls) = match outcome {
            swb_style::Restyle::Full(reason) => {
                log::debug!("style: full restyle after {} ({reason:?})", path.cause());
                self.restyle_counts(path).fallback(reason);
                return None;
            }
            swb_style::Restyle::Done {
                changed,
                restyled,
                image_urls,
            } => (changed, restyled, image_urls),
        };
        self.timings.style = started.elapsed();
        self.flushes.style(self.timings.style, self.in_query);
        let counts = self.restyle_counts(path);
        if restyled == 0 {
            counts.nothing = counts.nothing.saturating_add(1);
        } else {
            counts.targeted = counts.targeted.saturating_add(1);
        }
        counts.restyled_elements = counts.restyled_elements.saturating_add(restyled as u64);
        log::debug!("style: {:?}", self.timings.style);
        log::debug!(
            "targeted restyle after {}: {restyled} elements restyled",
            path.cause()
        );
        let layout_free = self.dom_changes.layout_free();
        self.style_states = self.states.clone();
        self.styles_stale = false;
        self.clear_element_styles();
        if path == RestylePath::Dom {
            self.dom_changes.clear();
        }
        #[cfg(debug_assertions)]
        self.check_targeted_restyle(map);
        for url in image_urls {
            if let Ok(url) = Url::parse(&url) {
                self.start_image(url);
            }
        }
        Some(changed || (path == RestylePath::Dom && !layout_free))
    }

    /// After a targeted restyle of `path` that left its map in `styles`:
    /// the canvas text color may have changed (a full restyle follows),
    /// else the layout is dropped if `drop_layout` and, after DOM changes,
    /// put back otherwise.
    fn after_targeted(&mut self, drop_layout: bool, path: RestylePath) {
        if self.frame && self.update_canvas_text() {
            self.compute_styles();
            self.invalidate_layout();
        } else if drop_layout {
            self.invalidate_layout();
        } else if path == RestylePath::Dom {
            self.restore_layout();
        }
    }

    /// The engine's check mode (debug builds, tests only): after a targeted
    /// restyle, a full restyle must give the same result (ADR 0041 section
    /// 6).
    #[cfg(debug_assertions)]
    fn check_targeted_restyle(&self, map: &swb_style::StyleMap) {
        if !super::restyle_check::enabled() {
            return;
        }
        let Some(stylist) = &self.stylist else {
            return;
        };
        let full = swb_style::compute_styles_animated(
            &self.dom,
            stylist,
            &self.media_environment(),
            &self.states,
            self.base_url.value(),
            &self.container_sizes,
            None,
            &self.animations,
        );
        if let Some(difference) = map.first_difference(&full) {
            panic!(
                "the targeted restyle of {} differs from a full restyle: {difference}",
                self.url.as_str()
            );
        }
        super::restyle_check::count();
    }

    /// The media environment of the document: the viewport, the scale,
    /// the color scheme and the scripting flag.
    pub(crate) fn media_environment(&self) -> MediaEnvironment {
        MediaEnvironment {
            viewport_width: self.view.viewport.width,
            viewport_height: self.view.viewport.height,
            device_pixel_ratio: self.view.scale,
            prefers_color_scheme: self.view.color_scheme,
            scripting: self.dom.scripting(),
            ..MediaEnvironment::default()
        }
    }

    /// Brings the styles up to date (a style flush): processes the DOM
    /// changes first. Rendering waits for stylesheets: until they are
    /// loaded, there are no styles.
    pub fn update_style(&mut self) {
        self.settle_animations();
        self.sync();
        if !self.stylesheets_settled() {
            // A layout kept for a targeted restyle (ADR 0042 section 6) must
            // not outlive styles that cannot be computed now.
            if self.styles.is_none() {
                self.kept_layout = None;
            }
            return;
        }
        if self.styles.is_none() {
            self.compute_styles();
        } else if self.styles_stale || self.animations.borrow().has_marked() {
            // A script changed the focus since the styles (ADR 0041
            // section 5), or animated values can have changed (ADR 0044
            // section 3): a targeted restyle.
            self.restyle();
        }
    }

    /// A layout flush for a query of a script: the runs count as forced.
    pub fn query_layout(&mut self, fonts: &mut FontContext) {
        let outer = std::mem::replace(&mut self.in_query, true);
        self.update_layout(fonts);
        self.in_query = outer;
    }

    /// The computed style of the element `node` for a query of a script,
    /// with its ancestors' styles computed too (ADR 0028 section 9, "the
    /// style of one element on demand"): from the styles of the document
    /// if they are current and `node` has one, else from the cascade of
    /// `node` and its ancestors, which is kept until the next change
    /// ([`DocumentState::element_styles`]). The DOM changes are processed
    /// first. `None` if the element is not in the document or the style
    /// sheets have not arrived (rendering waits for them).
    pub(crate) fn query_element_style(
        &mut self,
        node: swb_dom::NodeId,
    ) -> Option<std::sync::Arc<swb_style::ComputedStyle>> {
        self.process_changes();
        self.settle_animations();
        // Animated values that can have changed: the targeted restyle of
        // their owners (a seek by a script, the timeline).
        if self.styles.is_some() && self.animations.borrow().has_marked() {
            let outer = std::mem::replace(&mut self.in_query, true);
            self.update_style();
            self.in_query = outer;
        }
        if !self.styles_stale
            && let Some(style) = self.styles.as_ref().and_then(|s| s.get(node))
        {
            return Some(std::sync::Arc::clone(style));
        }
        if let Some(style) = self.element_styles.get(&node) {
            return Some(std::sync::Arc::clone(style));
        }
        if !self.stylesheets_settled() {
            return None;
        }
        // Many queries without a change between them (a script that reads
        // the style of every element): one full restyle costs less than
        // the cascade of each element with its ancestors.
        if self.element_styles.len() > (self.dom.len() / 16).max(256) {
            let outer = std::mem::replace(&mut self.in_query, true);
            self.update_style();
            self.in_query = outer;
            if let Some(style) = self.styles.as_ref().and_then(|s| s.get(node)) {
                return Some(std::sync::Arc::clone(style));
            }
        }
        if self.images.sources_outdated {
            self.select_image_sources();
        }
        let env = self.media_environment();
        self.ensure_stylist(&env);
        // The states of the form controls only if a selector depends on
        // them: their rebuild after a DOM change walks the document.
        if self
            .stylist
            .as_ref()
            .is_some_and(|s| s.state_dependencies().intersects(swb_style::CONTROL_STATES))
        {
            self.refresh_forms();
        }
        let stylist = self.stylist.as_ref()?;
        let containers = stylist.has_container_dependence();
        let started = Instant::now();
        let chain = swb_style::compute_element_styles_animated(
            &self.dom,
            stylist,
            &env,
            &self.states,
            self.base_url.value(),
            node,
            &self.container_sizes,
            Some(&self.animations),
        );
        self.flushes.element_style(started.elapsed());
        // An element inside a query container: its style can depend on the
        // container's size in the current layout (measured: a script's
        // change of a container's width shows in `getComputedStyle` at
        // once). A forced layout flush, then the full styles (ADR 0038
        // section 6). Elements outside containers and pages without
        // container rules and units keep the cheap path.
        let inside_container = containers
            && chain
                .iter()
                .rev()
                .skip(1)
                .any(|(_, s)| s.is_query_container());
        if inside_container && let Some(style) = self.style_after_layout(node) {
            return Some(style);
        }
        let style = chain.last().map(|(_, s)| std::sync::Arc::clone(s));
        self.element_styles.extend(chain);
        style
    }

    /// The style of `node` after a forced layout flush: from the full
    /// styles, or for an element without one (below `display: none`) from
    /// its chain with the container sizes of the new layout. `None` if the
    /// flush cannot run (no fonts lent: not in a script task).
    fn style_after_layout(
        &mut self,
        node: swb_dom::NodeId,
    ) -> Option<std::sync::Arc<swb_style::ComputedStyle>> {
        if !self.script_layout_flush() {
            return None;
        }
        if !self.styles_stale
            && let Some(style) = self.styles.as_ref().and_then(|s| s.get(node))
        {
            return Some(std::sync::Arc::clone(style));
        }
        let env = self.media_environment();
        let stylist = self.stylist.as_ref()?;
        let started = Instant::now();
        let chain = swb_style::compute_element_styles_animated(
            &self.dom,
            stylist,
            &env,
            &self.states,
            self.base_url.value(),
            node,
            &self.container_sizes,
            Some(&self.animations),
        );
        self.flushes.element_style(started.elapsed());
        let style = chain.last().map(|(_, s)| std::sync::Arc::clone(s));
        self.element_styles.extend(chain);
        style
    }

    /// Brings styles and layout up to date (a layout flush). Rendering
    /// waits for stylesheets: until they are loaded, there is no layout.
    /// After each layout until the document is loaded, scrolls to the
    /// fragment or to the restored position.
    ///
    /// Layout can request web fonts. A font whose file is already loaded
    /// (another face of the same file) is used at once and invalidates the
    /// layout; layout then runs again, up to [`MAX_LAYOUT_PASSES`] times.
    pub fn update_layout(&mut self, fonts: &mut FontContext) {
        self.update_style();
        if self.styles.is_none() {
            return;
        }
        self.update_web_font_faces(fonts);
        let laid_out = self.lay_out_with_fonts(fonts);
        // Container queries (ADR 0038 section 4): a layout that gives the
        // query containers sizes that change a query result or a unit
        // makes the styles and the layout run again, at most
        // `MAX_CONTAINER_PASSES` times. Each pass is a whole restyle and
        // layout; a page whose styles read no container never loops.
        let mut passes = 0;
        while laid_out && self.fragments.is_some() && self.containers_changed() {
            if passes == MAX_CONTAINER_PASSES {
                log::warn!(
                    "container queries: {MAX_CONTAINER_PASSES} extra passes did not settle; \
                     the last styles stay"
                );
                self.flushes.container_pass_limits =
                    self.flushes.container_pass_limits.saturating_add(1);
                break;
            }
            passes += 1;
            self.flushes.container_passes = self.flushes.container_passes.saturating_add(1);
            let Some(old) = self.styles.take() else {
                break;
            };
            self.compute_styles_from(Some(&old));
            if self
                .styles
                .as_ref()
                .is_some_and(|new| new.same_styles(&old))
            {
                break;
            }
            self.invalidate_layout();
            self.update_web_font_faces(fonts);
            if !self.lay_out_with_fonts(fonts) {
                break;
            }
        }
        if laid_out && self.fragments.is_some() {
            // The scroll target is applied after every layout until the
            // document is loaded, because images that arrive later move it.
            // Scrolling by the user cancels it.
            match self.pending_scroll.clone() {
                Some(ScrollTarget::Fragment(fragment)) => {
                    self.reveal_fragment(&fragment);
                }
                Some(ScrollTarget::Position(position)) => self.scroll = position,
                None => {}
            }
            self.clamp_scroll();
            if !self.is_loading() {
                self.pending_scroll = None;
            }
        }
    }

    /// Lays out the document, up to [`MAX_LAYOUT_PASSES`] times while web
    /// fonts that are already loaded invalidate the layout. Returns true if
    /// it laid out.
    ///
    /// After the last pass, `fragments` can be `None` again: a font that
    /// loaded at once invalidated the layout. This is correct: every caller
    /// (paint, scroll, hit testing) calls `update_layout` before it uses the
    /// fragments, so the next call lays out again. The pass limit only
    /// bounds the work of one call.
    fn lay_out_with_fonts(&mut self, fonts: &mut FontContext) -> bool {
        let mut laid_out = false;
        for _ in 0..MAX_LAYOUT_PASSES {
            if self.fragments.is_some() || !self.run_layout(fonts) {
                break;
            }
            laid_out = true;
            self.start_font_loads(fonts);
        }
        laid_out
    }

    /// Reads the content boxes of the query containers from the layout
    /// into [`DocumentState::container_sizes`] and returns true if they
    /// change what the styles read (a query result or a container unit).
    /// Free when the styles read no container.
    fn containers_changed(&mut self) -> bool {
        let (Some(styles), Some(fragments)) = (&self.styles, &self.fragments) else {
            return false;
        };
        if styles.container_log().is_empty() {
            return false;
        }
        let sizes = container_sizes(fragments, styles);
        let changed = !styles.container_log().holds(&sizes);
        self.container_sizes = sizes;
        changed
    }

    /// Lays out the document, if it has styles. Returns false if not.
    fn run_layout(&mut self, fonts: &mut FontContext) -> bool {
        let Some(styles) = &self.styles else {
            return false;
        };
        let controls = LayoutControls {
            doc: &self.dom,
            forms: self.forms.value(),
            focus: self.states.focus,
        };
        let input = LayoutInput {
            document: &self.dom,
            styles,
            viewport: self.view.viewport,
            replaced: &self.images,
            controls: &controls,
        };
        let started = Instant::now();
        let fragments = swb_layout::layout(&input, fonts);
        self.timings.layout = started.elapsed();
        self.flushes.layout(self.timings.layout, self.in_query);
        log::debug!("layout: {:?}", self.timings.layout);
        self.scrollers.update(&fragments);
        self.fragments = Some(fragments);
        self.layout_serial += 1;
        self.keep_control_scroll();
        self.select_auto_sized_images();
        true
    }

    /// Stores the scroll offsets of the text in form controls that layout
    /// used, so that the next layout starts from them.
    fn keep_control_scroll(&mut self) {
        let Some(tree) = &self.fragments else {
            return;
        };
        let forms = self.forms.value_mut();
        tree.walk(|fragment, _| {
            if let FragmentRef::Box(b) = fragment
                && let (Some(node), BoxContent::Control(c)) = (b.node, &b.content)
                && let Some(state) = forms.get_mut(node)
            {
                state.scroll = c.scroll;
            }
        });
        // The layout showed the caret that the user moved.
        forms.reveal_focus = false;
    }

    /// Brings the display list up to date.
    pub(crate) fn update_display_list(&mut self, fonts: &mut FontContext) {
        self.update_layout(fonts);
        // A change of the selection (also a script's) shows when the page
        // selection that the list shows changed.
        let generation = self.dom.selection_generation();
        if self.display_list.is_some() && generation != self.display_selection.0 {
            if self.visible_selection() == self.display_selection.1 {
                self.display_selection.0 = generation;
            } else {
                self.display_list = None;
            }
        }
        if self.display_list.is_some() || self.fragments.is_none() {
            // A scroll or a resize keeps the list.
            self.update_animation_visibility();
            return;
        }
        let page = self.visible_selection();
        self.display_selection = (generation, page);
        let control = self.control_selection();
        if page.is_some() {
            // Built before the borrows below.
            self.tree_order();
        }
        let Some(fragments) = &self.fragments else {
            return;
        };
        let started = Instant::now();
        let order = self.tree_order.value();
        let page = page.map(|r| order.keys(&self.dom, r.start, r.end));
        let scrolling = Scrolling {
            offsets: self.scrollers.offsets(),
            indicators: self.view.indicators,
            scale: self.view.scale,
        };
        let list = if page.is_some() || control.is_some() {
            let highlight = Highlight {
                page,
                control,
                order,
                doc: &self.dom,
            };
            swb_paint::build_display_list(fragments, &self.images, &highlight, &scrolling)
        } else {
            swb_paint::build_display_list(fragments, &self.images, &NoHighlights, &scrolling)
        };
        self.timings.display_list = started.elapsed();
        self.flushes.display_list_runs = self.flushes.display_list_runs.saturating_add(1);
        log::debug!("display list: {:?}", self.timings.display_list);
        if self.frame {
            self.list_ends = swb_paint::transform_ends(&list.items);
        }
        self.list_serial = super::paint_stamp::next_list_serial();
        self.display_list = Some(list);
        self.update_animation_visibility();
    }

    /// The selection in the focused text control, as offsets in its shown
    /// text (for the highlight).
    pub(crate) fn control_selection(&self) -> Option<(swb_dom::NodeId, u32, u32)> {
        let node = self.states.focus?;
        let state = self.forms.value().get(node).filter(|s| s.ty.is_text())?;
        let range = state.edit.selection();
        if range.is_empty() {
            return None;
        }
        let password = state.ty == ControlType::Input(InputType::Password);
        let to_u32 = |v: usize| u32::try_from(v).unwrap_or(u32::MAX);
        let start = display_offset(state.edit.text(), range.start, password);
        let end = display_offset(state.edit.text(), range.end, password);
        Some((node, to_u32(start), to_u32(end)))
    }

    /// Builds the stylist if it is dirty, from the parsed sheets (a sheet
    /// is parsed once, at its first use).
    fn ensure_stylist(&mut self, env: &MediaEnvironment) {
        if self.stylist.is_some() && self.adopted.out_of_date(&self.dom) {
            self.stylist = None;
        }
        if self.stylist.is_some() {
            return;
        }
        let started = Instant::now();
        let mut stylist = Stylist::new(self.dom.quirks_mode);
        stylist.set_animations(self.animations_enabled);
        let mut in_shadow_trees = Vec::new();
        for index in 0..self.sheets.len() {
            // The sheets of shadow trees go to their trees' stylists.
            if self
                .sheets
                .get(index)
                .is_some_and(|slot| slot.scope != swb_dom::NodeId::DOCUMENT)
            {
                in_shadow_trees.push(index);
                continue;
            }
            // A sheet that was not made and does not apply (its `media`
            // attribute does not match) stays unparsed, as before the CSSOM
            // (a script that reads it makes it). A disabled sheet exists
            // already: `disabled` is a property of the sheet object.
            if let Some(slot) = self.sheets.get(index)
                && !slot.has_sheet()
                && slot
                    .media
                    .as_deref()
                    .is_some_and(|m| !MediaQueryList::parse_str(m).matches(env))
            {
                continue;
            }
            let Some(sheet) = self.cssom_sheet(index) else {
                continue;
            };
            let sheet = sheet.borrow();
            if sheet.disabled || !sheet.media.matches(env) {
                continue;
            }
            if let Some(slot) = self.sheets.get(index) {
                stylist.add_cssom_sheet(&sheet, &slot.base_url);
            }
        }
        // The document's adopted sheets come after its other sheets and
        // join their layer order (measured in feature 1 and 5c-2).
        for sheet in self.adopted.document_sheets(env) {
            let sheet = sheet.borrow();
            if let Some(base) = sheet.constructed_base_url() {
                stylist.add_cssom_sheet(&sheet, base);
            }
        }
        let adopted_roots = self.adopted.root_sheets(&self.dom, env);
        if !in_shadow_trees.is_empty() || !adopted_roots.is_empty() {
            self.add_shadow_scopes(&mut stylist, &in_shadow_trees, adopted_roots, env);
        }
        self.stylist = Some(stylist);
        self.timings.stylesheets = started.elapsed();
        self.flushes.stylist(self.timings.stylesheets);
    }

    /// Computes the styles, after the stylist if it is dirty: from the stale
    /// map and the DOM changes since it if it can (ADR 0042), else in full.
    fn compute_styles(&mut self) {
        let Some(mut map) = self.stale_styles.take() else {
            self.compute_styles_from(None);
            return;
        };
        if self.styles_full {
            self.compute_styles_from(Some(&map));
            return;
        }
        match self.run_targeted(&mut map, RestylePath::Dom) {
            Some(drop_layout) => {
                self.styles = Some(map);
                self.after_targeted(drop_layout, RestylePath::Dom);
            }
            None => self.compute_styles_from(Some(&map)),
        }
    }

    /// [`Self::compute_styles`] for a restyle: equal custom-property maps
    /// of `previous` are reused (ADR 0007).
    fn compute_styles_from(&mut self, previous: Option<&swb_style::StyleMap>) {
        // A full computation is current for every change so far.
        self.stale_styles = None;
        self.kept_layout = None;
        self.dom_changes.clear();
        self.styles_full = false;
        self.refresh_forms();
        if self.images.sources_outdated {
            self.select_image_sources();
        }
        let env = self.media_environment();
        self.ensure_stylist(&env);
        let Some(stylist) = &self.stylist else {
            return;
        };
        let started = Instant::now();
        // A full restyle restyles every owner of an animation.
        self.animations.borrow_mut().take_marked();
        let styles = swb_style::compute_styles_animated(
            &self.dom,
            stylist,
            &env,
            &self.states,
            self.base_url.value(),
            &self.container_sizes,
            previous,
            &self.animations,
        );
        self.timings.style = started.elapsed();
        self.flushes.style(self.timings.style, self.in_query);
        log::debug!("style: {:?}", self.timings.style);
        let image_urls = styles.image_urls();
        self.styles = Some(styles);
        self.style_states = self.states.clone();
        self.styles_stale = false;
        self.clear_element_styles();
        for url in image_urls {
            if let Ok(url) = Url::parse(&url) {
                self.start_image(url);
            }
        }
        if self.frame && self.update_canvas_text() {
            self.compute_styles();
        }
    }

    /// Discards the parsed stylesheets, the stylist, the styles, the
    /// layout and the display list, so that the next render runs all
    /// stages again. For benchmarks.
    pub fn restart_pipeline(&mut self) {
        for slot in self.sheets.iter_mut() {
            slot.parse_again();
        }
        self.invalidate_style();
    }

    /// Renders `what` into `target` in strips of `strip_rows` device rows,
    /// each with the rasterizer's budgets of a viewport
    /// ([`swb_paint::rasterize_in_strips`]). Fixed and sticky boxes are
    /// where they are at the document's scroll offset.
    pub(crate) fn render_in_strips(
        &mut self,
        target: &mut Pixmap,
        strip_rows: u32,
        what: Rendering,
        fonts: &mut FontContext,
        frames: &Frames,
    ) {
        swb_paint::fill(target, swb_style::Rgba::WHITE);
        self.update_display_list(fonts);
        // The document point at the top-left corner of the target.
        let origin = match what {
            Rendering::Viewport => self.scroll,
            Rendering::Page => Point::default(),
        };
        if let Some(list) = &self.display_list {
            let params = RasterParams {
                scroll: origin,
                viewport_scroll: self.scroll,
                scale: self.view.scale,
            };
            let started = Instant::now();
            if frames.count() == 0 {
                swb_paint::rasterize_in_strips(
                    list,
                    target,
                    params,
                    strip_rows,
                    fonts,
                    &self.images,
                );
            } else {
                let source = FrameSource::new(&self.images, frames, FrameId::TOP);
                swb_paint::rasterize_in_strips(list, target, params, strip_rows, fonts, &source);
            }
            self.timings.raster = started.elapsed();
        }
        // The viewport indicators belong on a rendering of the viewport.
        if self.view.indicators && what == Rendering::Viewport {
            self.render_viewport_indicators(target, fonts);
        }
    }

    /// Draws the overlay scroll indicators of the viewport over the page
    /// (the same indicators as those of scroll containers).
    fn render_viewport_indicators(&mut self, target: &mut Pixmap, fonts: &mut FontContext) {
        let Some(fragments) = &self.fragments else {
            return;
        };
        if fragments.viewport_scrollbar_hidden {
            return;
        }
        let viewport = self.view.viewport;
        let port = Rect::new(0.0, 0.0, viewport.width, viewport.height);
        // No indicator on axes with `overflow: hidden`.
        let (user_x, user_y) = self.viewport_user_axes();
        let size = fragments.scroll_size;
        let items = swb_paint::scroll_indicators(port, size, self.scroll, user_x, user_y);
        if items.is_empty() {
            return;
        }
        let params = RasterParams {
            scroll: Point::default(),
            viewport_scroll: Point::default(),
            scale: self.view.scale,
        };
        let list = DisplayList { items };
        swb_paint::rasterize(&list, target, params, fonts, &self.images);
    }
}
