//! Web fonts: the face set of the document's `@font-face` rules and the
//! loading of the faces that text needs (ADR 0022).
//!
//! After each layout the page takes the faces that the text crate
//! requested and loads their sources in order: a `url()` through the
//! loader (each URL once), a `local()` source fails (not supported yet).
//! A face whose sources all fail is marked as failed. Each arriving font
//! invalidates the layout; the next layout uses it and can request more
//! faces. Loading waits for fonts like for images, so headless rendering
//! and `page.waitForLoad` see the final fonts.

use swb_net::{NetError, Request, Response, Url};
use swb_text::WebFaceId;

use super::Page;
use crate::resources::Pending;
use crate::web_fonts::{FontFile, Source, web_font_face};

impl Page {
    /// Gives the font context the `@font-face` rules of the stylist that
    /// apply in the current media environment, when the stylist or the
    /// environment changed.
    pub(super) fn update_web_font_faces(&mut self) {
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
        self.fonts.set_web_fonts(faces);
        self.web_fonts.env = Some(env);
        self.invalidate_layout();
    }

    /// Starts loading the faces that the last layout requested.
    pub(super) fn start_font_loads(&mut self) {
        for id in self.fonts.take_web_font_requests() {
            self.load_font_source(id, 0);
        }
    }

    /// Loads the first source of face `id` at index `from` or later that
    /// can work: a loaded file is used at once, a file that is loading
    /// gets the face as a waiter, a new URL is fetched. Without such a
    /// source the face fails.
    fn load_font_source(&mut self, id: WebFaceId, from: usize) {
        let Some(sources) = self.fonts.web_font_sources(id).map(<[String]>::to_vec) else {
            return;
        };
        for (index, source) in sources.iter().enumerate().skip(from) {
            let url = match Source::parse(source) {
                Source::Url(url) => url,
                Source::Local(name) => {
                    log::debug!("font source local({name}) is not supported yet");
                    continue;
                }
                Source::Invalid => continue,
            };
            match self.web_fonts.files.get_mut(&url) {
                Some(FontFile::Loaded(data)) => {
                    let data = data.clone();
                    match self.fonts.web_font_loaded(id, url.as_str(), &data) {
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
                    if self.may_load_subresource(&url) {
                        self.start_font_file(url, id, index);
                        return;
                    }
                    self.web_fonts.files.insert(url, FontFile::Failed);
                }
            }
        }
        self.fonts.web_font_failed(id);
        self.invalidate_layout();
    }

    fn start_font_file(&mut self, url: Url, id: WebFaceId, index: usize) {
        log::debug!("font request: {url}");
        self.web_fonts
            .files
            .insert(url.clone(), FontFile::Loading(vec![(id, index)]));
        let request = Request::get(url.clone(), swb_net::Destination::Font);
        let request_id = self
            .loader
            .start(request.with_initiator(self.document_origin()));
        self.requests
            .pending
            .insert(request_id, Pending::Font { url });
    }

    /// Handles the response for the font file at `url`: decodes it, then
    /// gives it to the faces that wait for it (or lets them try their next
    /// source).
    pub(super) fn font_file_arrived(&mut self, url: &Url, result: Result<Response, NetError>) {
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
                self.fonts
                    .web_font_loaded(id, url.as_str(), &data)
                    .map_err(|e| log::warn!("{e}"))
                    .is_ok()
            });
            if !loaded {
                self.load_font_source(id, index + 1);
            }
        }
        self.invalidate_layout();
    }

    /// Fails the faces whose files were still loading when loading was
    /// stopped or a navigation started.
    pub(super) fn cancel_font_loads(&mut self) {
        for id in self.web_fonts.forget_loading() {
            self.fonts.web_font_failed(id);
        }
        self.fonts.fail_web_font_requests();
    }
}
