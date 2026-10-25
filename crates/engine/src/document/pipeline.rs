//! The rendering pipeline of a document: style, layout, display list and
//! raster, with per-stage timings, and their invalidation (ADR 0028
//! section 9). Each stage runs when something needs it (a frame, a query)
//! and its input changed; a flush after no change does nothing.

use std::time::Instant;

use swb_css::{MediaEnvironment, MediaQueryList};
use swb_layout::{BoxContent, FragmentRef, LayoutInput, Point, Rect};
use swb_net::Url;
use swb_paint::{DisplayList, NoHighlights, Pixmap, RasterParams, Scrolling};
use swb_style::Stylist;
use swb_text::FontContext;

use super::{DocumentState, ScrollTarget};
use crate::forms::{ControlType, InputType, LayoutControls, display_offset};
use crate::selection::Highlight;

/// The most layout passes of one [`DocumentState::update_layout`]: web
/// fonts that are already loaded make layout run again.
const MAX_LAYOUT_PASSES: usize = 4;

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

    /// Drops the styles and everything after them (the styles are dirty).
    pub(crate) fn invalidate_styles(&mut self) {
        self.styles = None;
        self.invalidate_layout();
    }

    /// Drops the layout and the display list.
    pub(crate) fn invalidate_layout(&mut self) {
        self.fragments = None;
        self.display_list = None;
    }

    /// Recomputes the styles after a change of the element states. The
    /// layout stays if no style changed.
    pub(crate) fn restyle(&mut self) {
        self.sync();
        let Some(old) = self.styles.take() else {
            return;
        };
        self.compute_styles();
        if !self
            .styles
            .as_ref()
            .is_some_and(|new| new.same_styles(&old))
        {
            self.invalidate_layout();
        }
    }

    /// The media environment of the document: the viewport, the scale
    /// and the scripting flag.
    pub(crate) fn media_environment(&self) -> MediaEnvironment {
        MediaEnvironment {
            viewport_width: self.view.viewport.width,
            viewport_height: self.view.viewport.height,
            device_pixel_ratio: self.view.scale,
            scripting: self.dom.scripting(),
            ..MediaEnvironment::default()
        }
    }

    /// Brings the styles up to date (a style flush): processes the DOM
    /// changes first. Rendering waits for stylesheets: until they are
    /// loaded, there are no styles.
    pub fn update_style(&mut self) {
        self.sync();
        if (self.styles.is_none() || self.styles_stale) && self.stylesheets_settled() {
            self.compute_styles();
        }
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
        let mut laid_out = false;
        // After the last pass, `fragments` can be `None` again: a font that
        // loaded at once invalidated the layout. This is correct: every
        // caller (paint, scroll, hit testing) calls `update_layout` before
        // it uses the fragments, so the next call lays out again. The pass
        // limit only bounds the work of one call.
        for _ in 0..MAX_LAYOUT_PASSES {
            if self.fragments.is_some() || !self.run_layout(fonts) {
                break;
            }
            laid_out = true;
            self.start_font_loads(fonts);
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
        log::debug!("layout: {:?}", self.timings.layout);
        self.scrollers.update(&fragments);
        self.fragments = Some(fragments);
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
    }

    /// Brings the display list up to date.
    pub(crate) fn update_display_list(&mut self, fonts: &mut FontContext) {
        self.update_layout(fonts);
        if self.display_list.is_some() || self.fragments.is_none() {
            return;
        }
        let page = self.visible_selection();
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
        let page = page.map(|s| s.ordered(order));
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
        log::debug!("display list: {:?}", self.timings.display_list);
        self.display_list = Some(list);
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

    /// Computes the styles: builds the stylist first if it is dirty, from
    /// the parsed sheets (a sheet is parsed once, at its first use).
    fn compute_styles(&mut self) {
        self.refresh_forms();
        if self.images.sources_outdated {
            self.select_image_sources();
        }
        let env = self.media_environment();
        if self.stylist.is_none() {
            let started = Instant::now();
            let mut stylist = Stylist::new(self.dom.quirks_mode);
            for sheet in self.sheets.iter_mut() {
                if let Some(media) = &sheet.media
                    && !MediaQueryList::parse_str(media).matches(&env)
                {
                    continue;
                }
                let base_url = sheet.base_url.clone();
                if let Some(parsed) = sheet.parsed() {
                    stylist.add_author_sheet(parsed, &base_url);
                }
            }
            self.stylist = Some(stylist);
            self.timings.stylesheets = started.elapsed();
        }
        let Some(stylist) = &self.stylist else {
            return;
        };
        let started = Instant::now();
        let styles = swb_style::compute_styles(
            &self.dom,
            stylist,
            &env,
            &self.states,
            self.base_url.value(),
        );
        self.timings.style = started.elapsed();
        log::debug!("style: {:?}", self.timings.style);
        let image_urls = styles.image_urls();
        self.styles = Some(styles);
        self.styles_stale = false;
        for url in image_urls {
            if let Ok(url) = Url::parse(&url) {
                self.start_image(url);
            }
        }
    }

    /// Discards the parsed stylesheets, the stylist, the styles, the
    /// layout and the display list, so that the next render runs all
    /// stages again. For benchmarks.
    pub fn restart_pipeline(&mut self) {
        for sheet in self.sheets.iter_mut() {
            sheet.parsed = None;
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
            swb_paint::rasterize_in_strips(list, target, params, strip_rows, fonts, &self.images);
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
