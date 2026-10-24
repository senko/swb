//! The subresources of a document: the requests of stylesheets, images
//! (also video posters) and web font files, their completions, the
//! selection of image sources, and the web fonts that layout asks for
//! (ADR 0022).
//!
//! Web fonts: after each layout the document takes the faces that the
//! text crate requested and loads their sources in order: a `url()`
//! through the loader (each URL once), a `local()` source loads at once
//! from the installed fonts. A face whose sources all fail is marked as
//! failed. Each arriving font invalidates the layout; the next layout uses
//! it and can request more faces. Loading waits for fonts like for images,
//! so headless rendering and `page.waitForLoad` see the final fonts.
//!
//! Image sources with `sizes="auto"`: a lazy-loaded `img` whose source
//! depends on the width of its own box is selected after each layout, and
//! again when the width (or the viewport or the scale) changes
//! (<https://html.spec.whatwg.org/multipage/images.html#parse-a-sizes-attribute>).
//! This does not loop: such an image has size containment (user-agent
//! style sheet), so its box does not depend on its source; and each image
//! selects at most [`MAX_AUTO_SELECTIONS`](crate::resources::MAX_AUTO_SELECTIONS)
//! times (an image with `sizes=" auto"` has no containment).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use encoding_rs::Encoding;
use swb_dom::NodeId;
use swb_layout::{FragmentRef, FragmentTree};
use swb_net::{Destination, NetError, Request, Response, Url};
use swb_text::{FontContext, WebFaceId};

use super::DocumentState;
use super::changes::LoadPlan;
use crate::image_source;
use crate::resources::{AutoSizes, ImageState, Pending, Requests, SheetSlot};
use crate::web_fonts::{FontFile, Source, web_font_face};

impl DocumentState {
    /// Handles the completion of a subresource request.
    pub(crate) fn handle_completion(
        &mut self,
        pending: Pending,
        result: Result<Response, NetError>,
        fonts: &mut FontContext,
    ) {
        match pending {
            Pending::Stylesheet { owner, serial } => self.stylesheet_arrived(owner, serial, result),
            Pending::Font { url } => self.font_file_arrived(&url, result, fonts),
            Pending::Script { node } => self.script_arrived(node, result),
            Pending::Image { url } => {
                let state = ImageState::from_fetch(&url, result, &self.images.counting);
                self.images.by_url.insert(url.clone(), state);
                self.images.load_ended(&url);
                self.invalidate_layout();
            }
        }
    }

    fn stylesheet_arrived(
        &mut self,
        owner: NodeId,
        serial: u64,
        result: Result<Response, NetError>,
    ) {
        let Some(sheet) = self.sheets.of(owner).filter(|s| s.serial == serial) else {
            // The element left the document or loads another sheet now.
            return;
        };
        match result {
            Ok(response) if response.is_success() => {
                let charset = response.content_type().and_then(|c| c.charset);
                sheet.css = Some(decode_css(&response.body, charset.as_deref()));
                sheet.parsed = None;
                sheet.base_url = response.url;
            }
            Ok(response) => {
                log::warn!(
                    "stylesheet {} failed: HTTP {}",
                    response.url,
                    response.status
                );
                sheet.failed = true;
            }
            Err(e) => {
                log::warn!("stylesheet failed: {e}");
                sheet.failed = true;
            }
        }
        self.invalidate_style();
    }

    /// Starts the loads that a drain of the journal found. `full` selects
    /// the sources of all images of the document (a walk of the whole
    /// document), else those of `plan.images`.
    pub(super) fn start_plan(&mut self, plan: LoadPlan, full: bool) {
        self.start_stylesheets(plan.sheets);
        for url in plan.posters {
            self.start_image(url);
        }
        if full {
            self.select_image_sources();
        } else if !plan.images.is_empty() {
            self.select_images(Some(&plan.images));
        }
    }

    /// Starts the requests of the external stylesheets `loads` (owner,
    /// load number, URL); a stylesheet that the document may not load
    /// fails.
    fn start_stylesheets(&mut self, loads: Vec<(NodeId, u64, Url)>) {
        for (owner, serial, url) in loads {
            if !self.may_load(&url) {
                if let Some(sheet) = self.sheets.of(owner) {
                    sheet.failed = true;
                }
                continue;
            }
            #[cfg(test)]
            self.load_log.push(format!("style {url}"));
            let request = Request::get(url, Destination::Style);
            let id = self
                .loader
                .start(request.with_initiator(Some(self.origin())));
            self.requests
                .pending
                .insert(id, Pending::Stylesheet { owner, serial });
        }
    }

    /// Selects the source of each image of the document for the current
    /// viewport and scale (`crate::image_source`) and starts loading the
    /// new ones: when the subresources of a document start to load, and at
    /// the next style computation after a viewport or scale change
    /// (`Images::sources_outdated`), so that a series of resize events
    /// selects once. An image whose source changes keeps its current image
    /// until the new one has loaded (`Images::reselect`); the dimension
    /// sources change at once.
    pub(super) fn select_image_sources(&mut self) {
        self.images.sources_outdated = false;
        self.select_images(None);
    }

    /// Selects the sources of the images `only` (in tree order), or of all
    /// images of the document, and starts loading the new ones.
    fn select_images(&mut self, only: Option<&[NodeId]>) {
        if only.is_none() {
            // `stop` cancels requests but leaves their images `Loading`
            // (`stop_loading` forgets them); this is a safety net for a
            // full selection, which walks the document anyway.
            let in_flight: HashSet<&Url> = self
                .requests
                .pending
                .values()
                .filter_map(|p| match p {
                    Pending::Image { url } => Some(url),
                    _ => None,
                })
                .collect();
            self.images.forget_cancelled(&in_flight);
        }
        let env = self.media_environment();
        let doc = &self.dom;
        let base = self.base_url.value();
        let selections = match only {
            Some(nodes) => image_source::select_images(doc, base, &env, nodes.iter().copied()),
            None => image_source::select_images(doc, base, &env, doc.descendants(NodeId::DOCUMENT)),
        };
        // The dimension sources change only for the nodes of `only` that
        // had one or get one; `Arc::make_mut` clones only a shared map.
        match only {
            Some(nodes) => {
                for node in nodes {
                    if self.states.dimension_sources.contains_key(node) {
                        Arc::make_mut(&mut self.states.dimension_sources).remove(node);
                    }
                }
            }
            None => self.states.dimension_sources = Arc::default(),
        }
        let mut loads = Vec::new();
        let mut auto_nodes = Vec::new();
        for (node, selection) in selections {
            if let Some(source) = selection.dimension_source {
                Arc::make_mut(&mut self.states.dimension_sources).insert(node, source);
            }
            // The width of the box decides: selected after layout.
            if selection.auto {
                auto_nodes.push(node);
                continue;
            }
            // Without a candidate, the image stays as it is (the
            // specification returns early).
            if let Some(image) = selection.image {
                let url = image.url.clone();
                if self.images.reselect(node, image) {
                    loads.push(url);
                }
            }
        }
        self.update_auto_images(only, auto_nodes);
        for url in loads {
            self.start_image(url);
        }
    }

    /// Sets the images that wait for their width, with the state of their
    /// last selection. With `only`, the other images keep their place, and
    /// the list is not touched when no node of `only` is or becomes auto.
    fn update_auto_images(&mut self, only: Option<&[NodeId]>, auto_nodes: Vec<NodeId>) {
        if let Some(nodes) = only
            && auto_nodes.is_empty()
            && !nodes.iter().any(|n| self.images.auto_set.contains(n))
        {
            return;
        }
        let mut old: HashMap<NodeId, AutoSizes> = HashMap::new();
        let mut kept = Vec::new();
        let selected: HashSet<NodeId> = only.unwrap_or_default().iter().copied().collect();
        for (node, state) in std::mem::take(&mut self.images.auto) {
            if only.is_some() && !selected.contains(&node) {
                kept.push((node, state));
            } else {
                old.insert(node, state);
            }
        }
        kept.extend(
            auto_nodes
                .into_iter()
                .map(|node| (node, old.remove(&node).unwrap_or_default())),
        );
        self.images.auto_set = kept.iter().map(|(n, _)| *n).collect();
        self.images.auto = kept;
    }

    /// Forgets the image of an element that left the document.
    pub(super) fn forget_image(&mut self, node: NodeId) {
        self.images.by_node.remove(&node);
        if self.images.auto_set.remove(&node) {
            self.images.auto.retain(|(n, _)| *n != node);
        }
        if self.states.dimension_sources.contains_key(&node) {
            Arc::make_mut(&mut self.states.dimension_sources).remove(&node);
        }
    }

    /// Starts loading the image at `url`, unless it is known already or
    /// the document may not load it (then it fails).
    pub(crate) fn start_image(&mut self, url: Url) {
        if self.images.by_url.contains_key(&url) {
            return;
        }
        if !self.may_load(&url) {
            self.images.by_url.insert(url.clone(), ImageState::Failed);
            self.images.load_ended(&url);
            return;
        }
        log::debug!("image request: {url}");
        #[cfg(test)]
        self.load_log.push(format!("image {url}"));
        self.images.by_url.insert(url.clone(), ImageState::Loading);
        let request = Request::get(url.clone(), Destination::Image);
        let id = self
            .loader
            .start(request.with_initiator(Some(self.origin())));
        self.requests.pending.insert(id, Pending::Image { url });
    }

    /// True if every stylesheet arrived or failed: rendering can start.
    pub(crate) fn stylesheets_settled(&self) -> bool {
        self.sheets.iter().all(SheetSlot::is_settled)
    }

    /// Cancels the running requests (the page stopped loading): sheets
    /// that did not arrive are skipped, fonts that are loading fail.
    pub(crate) fn stop_loading(&mut self, fonts: &mut FontContext) {
        self.requests = Requests::default();
        // The parse ends here (HTML "abort a parser"), and so do the
        // fetches of scripts.
        self.parser = None;
        self.script_fetches = crate::resources::ScriptFetches::default();
        // The images of the cancelled requests are requested again by the
        // next selection or style computation.
        self.images.forget_cancelled(&HashSet::new());
        let mut sheets_dropped = false;
        for sheet in self.sheets.iter_mut() {
            if !sheet.is_settled() {
                sheet.failed = true;
                sheets_dropped = true;
            }
        }
        if sheets_dropped {
            self.invalidate_style();
        }
        self.cancel_font_loads(fonts);
        self.invalidate_layout();
    }

    // ----- Images with `sizes="auto"` -----

    /// Selects the sources of the images that wait for the width of their
    /// box, after a layout, and starts loading the new ones. An image that
    /// has no box uses the width of its last selection, else `100vw`.
    pub(super) fn select_auto_sized_images(&mut self) {
        if self.images.auto.is_empty() {
            return;
        }
        let Some(tree) = &self.fragments else {
            return;
        };
        let env = self.media_environment();
        let env_key = [
            env.viewport_width.to_bits(),
            env.viewport_height.to_bits(),
            env.device_pixel_ratio.to_bits(),
        ];
        let nodes: Vec<NodeId> = self.images.auto.iter().map(|(node, _)| *node).collect();
        let widths = content_widths(tree, &nodes);
        let (doc, base) = (&self.dom, self.base_url.value());
        let mut loads: Vec<Url> = Vec::new();
        for (index, node) in nodes.into_iter().enumerate() {
            let Some((_, state)) = self.images.auto.get_mut(index) else {
                continue;
            };
            if !state.update(widths.get(&node).copied(), env_key) {
                continue;
            }
            let width = state.width();
            let Some(image) = image_source::select_auto_image(doc, base, node, &env, width) else {
                continue;
            };
            let url = image.url.clone();
            if self.images.reselect(node, image) {
                loads.push(url);
            }
        }
        for url in loads {
            self.start_image(url);
        }
    }

    // ----- Web fonts -----

    /// Gives the font context the `@font-face` rules of the stylist that
    /// apply in the current media environment, when the stylist or the
    /// environment changed.
    pub(super) fn update_web_font_faces(&mut self, fonts: &mut FontContext) {
        let env = self.media_environment();
        if self.web_fonts.env.as_ref() == Some(&env) {
            return;
        }
        let Some(stylist) = &self.stylist else {
            return;
        };
        let faces = stylist
            .font_faces(&env)
            .into_iter()
            .map(web_font_face)
            .collect();
        fonts.set_web_fonts(faces);
        self.web_fonts.env = Some(env);
        self.invalidate_layout();
    }

    /// Starts loading the faces that the last layout requested.
    pub(super) fn start_font_loads(&mut self, fonts: &mut FontContext) {
        for id in fonts.take_web_font_requests() {
            self.load_font_source(id, 0, fonts);
        }
    }

    /// Loads the first source of face `id` at index `from` or later that
    /// can work: a loaded file is used at once, a file that is loading
    /// gets the face as a waiter, a new URL is fetched. Without such a
    /// source the face fails.
    fn load_font_source(&mut self, id: WebFaceId, from: usize, fonts: &mut FontContext) {
        let Some(sources) = fonts.web_font_sources(id).map(<[String]>::to_vec) else {
            return;
        };
        for (index, source) in sources.iter().enumerate().skip(from) {
            let url = match Source::parse(source) {
                Source::Url(url) => url,
                Source::Local(name) => {
                    // An installed font loads at once.
                    match fonts.web_font_local(id, &name) {
                        Ok(()) => {
                            self.invalidate_layout();
                            return;
                        }
                        Err(e) => log::debug!("font source local({name}): {e}"),
                    }
                    continue;
                }
                Source::Invalid => continue,
            };
            match self.web_fonts.files.get_mut(&url) {
                Some(FontFile::Loaded(data)) => {
                    let data = data.clone();
                    match fonts.web_font_loaded(id, url.as_str(), &data) {
                        Ok(()) => {
                            self.invalidate_layout();
                            return;
                        }
                        Err(e) => log::warn!("{e}"),
                    }
                }
                Some(FontFile::Loading(waiting)) => {
                    waiting.push((id, index));
                    return;
                }
                Some(FontFile::Failed) => {}
                None => {
                    if self.may_load(&url) {
                        self.start_font_file(url, id, index);
                        return;
                    }
                    self.web_fonts.files.insert(url, FontFile::Failed);
                }
            }
        }
        fonts.web_font_failed(id);
        self.invalidate_layout();
    }

    fn start_font_file(&mut self, url: Url, id: WebFaceId, index: usize) {
        log::debug!("font request: {url}");
        self.web_fonts
            .files
            .insert(url.clone(), FontFile::Loading(vec![(id, index)]));
        let request = Request::get(url.clone(), Destination::Font);
        let request_id = self
            .loader
            .start(request.with_initiator(Some(self.origin())));
        self.requests
            .pending
            .insert(request_id, Pending::Font { url });
    }

    /// Handles the response for the font file at `url`: decodes it, then
    /// gives it to the faces that wait for it (or lets them try their next
    /// source).
    fn font_file_arrived(
        &mut self,
        url: &Url,
        result: Result<Response, NetError>,
        fonts: &mut FontContext,
    ) {
        let file = match result {
            Ok(response) if response.is_success() => {
                match swb_text::decode_web_font(url.as_str(), &response.body) {
                    Ok(data) => FontFile::Loaded(data),
                    Err(e) => {
                        log::warn!("{e}");
                        FontFile::Failed
                    }
                }
            }
            Ok(response) => {
                log::warn!("font {url}: HTTP {}", response.status);
                FontFile::Failed
            }
            Err(e) => {
                log::warn!("font {url}: {e}");
                FontFile::Failed
            }
        };
        let waiting = match self.web_fonts.files.insert(url.clone(), file) {
            Some(FontFile::Loading(waiting)) => waiting,
            _ => Vec::new(),
        };
        for (id, index) in waiting {
            let data = match self.web_fonts.files.get(url) {
                Some(FontFile::Loaded(data)) => Some(data.clone()),
                _ => None,
            };
            let loaded = data.is_some_and(|data| {
                fonts
                    .web_font_loaded(id, url.as_str(), &data)
                    .map_err(|e| log::warn!("{e}"))
                    .is_ok()
            });
            if !loaded {
                self.load_font_source(id, index + 1, fonts);
            }
        }
        self.invalidate_layout();
    }

    /// Fails the faces whose files were still loading when loading was
    /// stopped or a navigation started.
    pub(crate) fn cancel_font_loads(&mut self, fonts: &mut FontContext) {
        for id in self.web_fonts.forget_loading() {
            fonts.web_font_failed(id);
        }
        fonts.fail_web_font_requests();
    }
}

/// The content-box width of the box of each of `nodes` that has one, in
/// CSS px: the "concrete object size" width of an image.
fn content_widths(tree: &FragmentTree, nodes: &[NodeId]) -> HashMap<NodeId, f32> {
    let wanted: HashSet<NodeId> = nodes.iter().copied().collect();
    let mut widths = HashMap::new();
    tree.walk(|fragment, _| {
        if let FragmentRef::Box(b) = fragment
            && b.pseudo.is_none()
            && let Some(node) = b.node
            && wanted.contains(&node)
        {
            let width = b.border_rect.width
                - (b.border.left + b.border.right + b.padding.left + b.padding.right);
            widths.entry(node).or_insert_with(|| width.max(0.0));
        }
    });
    widths
}

fn decode_css(bytes: &[u8], charset: Option<&str>) -> String {
    // CSS Syntax §3.2 (<https://www.w3.org/TR/css-syntax-3/#input-byte-stream>):
    // BOM, then the protocol charset, then @charset, then UTF-8. The BOM
    // (handled by `decode`), the protocol charset and UTF-8 are supported.
    let encoding = charset
        .and_then(|c| Encoding::for_label(c.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);
    let (text, _, _) = encoding.decode(bytes);
    text.into_owned()
}
