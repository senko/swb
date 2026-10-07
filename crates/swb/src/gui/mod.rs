//! The windowed browser: a winit window, a softbuffer surface, the toolbar
//! and one page.

mod toolbar;

use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use swb_automation::Automation;
use swb_engine::{Cursor, Modifiers, Page, PageConfig, Pixmap, Size, Url};
use swb_net::Fetcher;
use swb_paint::{DecodedImage, ImageRef, ImageSource, RasterParams, VectorCache};
use swb_text::FontContext;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{
    ElementState, KeyEvent, MouseButton, MouseScrollDelta, StartCause, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

use crate::url_input::parse_address;
use toolbar::{TOOLBAR_HEIGHT, Toolbar, ToolbarHit, ToolbarState};

/// Pixels scrolled per wheel "line".
const LINE_SCROLL: f32 = 48.0;

/// The longest time between the clicks of a double or triple click.
const MULTI_CLICK_TIME: Duration = Duration::from_millis(500);

/// The largest distance (in CSS px) between the clicks of a double click.
const MULTI_CLICK_DISTANCE: f32 = 4.0;

/// The window size (CSS px) that the page uses before the window exists.
const SIZE_WITHOUT_WINDOW: Size = Size::new(1024.0, 768.0);

/// Events sent to the event loop from other threads.
#[derive(Debug, Clone, Copy)]
enum UserEvent {
    /// A network request completed.
    Network,
    /// An automation request arrived.
    Automation,
}

/// Runs the GUI until the window is closed. With `remote_port`, also runs
/// the automation server.
pub(crate) fn run(
    fetcher: Arc<dyn Fetcher>,
    fonts: FontContext,
    start_url: Option<Url>,
    remote_port: Option<u16>,
) -> Result<()> {
    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .context("cannot create the event loop (is a display available?)")?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = event_loop.create_proxy();
    let automation = match remote_port {
        Some(port) => {
            let proxy = proxy.clone();
            let wake = Arc::new(move || {
                // The event loop may already be gone at shutdown.
                let _ = proxy.send_event(UserEvent::Automation);
            });
            let automation = Automation::start(port, wake)
                .with_context(|| format!("cannot start the automation server on port {port}"))?;
            crate::print_server_address(automation.port())?;
            Some(automation)
        }
        None => None,
    };
    let mut app = App::new(fetcher, fonts, start_url, proxy);
    app.automation = automation;
    event_loop.run_app(&mut app).context("event loop failed")?;
    Ok(())
}

struct Gfx {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
}

struct App {
    page: Page,
    start_url: Option<Url>,
    gfx: Option<Gfx>,
    toolbar: Toolbar,
    modifiers: ModifiersState,
    cursor: PhysicalPosition<f64>,
    /// True while the left button is held after a press in the address
    /// field (drag selection).
    selecting: bool,
    /// The last press of the left button in the page: time, position (CSS
    /// px) and click count, to detect double and triple clicks.
    last_press: Option<(Instant, (f32, f32), u32)>,
    /// True while the left button is held after a press in the page.
    page_pressed: bool,
    /// The system clipboard. `None` until the first use, and after a failed
    /// open (the next use tries again).
    clipboard: Option<arboard::Clipboard>,
    automation: Option<Automation>,
    /// The wheel delta (CSS px) since the last batch of events: the wheel
    /// events of one batch scroll once, so a scroll container's display
    /// list is built at most once per batch. The fraction of a pixel stays
    /// here (scroll offsets are whole pixels, as in Chromium).
    pending_wheel: (f32, f32),
}

struct NoImages;

impl ImageSource for NoImages {
    fn image(&self, _image: &ImageRef) -> Option<&DecodedImage> {
        None
    }

    fn vector_cache(&self) -> Option<&VectorCache> {
        None
    }
}

impl App {
    fn new(
        fetcher: Arc<dyn Fetcher>,
        fonts: FontContext,
        start_url: Option<Url>,
        proxy: EventLoopProxy<UserEvent>,
    ) -> Self {
        let notify = Arc::new(move || {
            // The event loop may already be gone at shutdown.
            let _ = proxy.send_event(UserEvent::Network);
        });
        let config = PageConfig {
            fetcher,
            notify,
            network_threads: PageConfig::DEFAULT_NETWORK_THREADS,
        };
        let mut page = Page::new(config, fonts, SIZE_WITHOUT_WINDOW, 1.0);
        // Scrollbars take no space; overlay indicators show what scrolls.
        page.set_scroll_indicators(true);
        App {
            page,
            start_url,
            gfx: None,
            toolbar: Toolbar::default(),
            modifiers: ModifiersState::empty(),
            cursor: PhysicalPosition::new(0.0, 0.0),
            selecting: false,
            last_press: None,
            page_pressed: false,
            clipboard: None,
            automation: None,
            pending_wheel: (0.0, 0.0),
        }
    }

    /// Scrolls by the whole pixels of the wheel delta collected since the
    /// last batch of events: over the page, the innermost scroll container
    /// under the pointer that can scroll in that direction, else the page;
    /// over the toolbar, the page. Returns true if something scrolled.
    fn apply_wheel(&mut self) -> bool {
        let ((whole_x, whole_y), rest) = split_wheel(self.pending_wheel);
        self.pending_wheel = rest;
        if whole_x == 0.0 && whole_y == 0.0 {
            return false;
        }
        let scale = self.scale();
        let x = self.cursor.x as f32 / scale;
        let y = self.cursor.y as f32 / scale - TOOLBAR_HEIGHT;
        if y >= 0.0 {
            self.page.wheel(x, y, whole_x, whole_y)
        } else {
            self.page.wheel_page(whole_x, whole_y)
        }
    }

    /// The modifiers in the engine's form.
    fn engine_modifiers(&self) -> Modifiers {
        Modifiers {
            shift: self.modifiers.shift_key(),
            ctrl: self.modifiers.control_key(),
            alt: self.modifiers.alt_key(),
            meta: self.modifiers.super_key(),
        }
    }

    /// Puts text on the clipboard (or, on Linux, the primary selection,
    /// which a middle click pastes).
    fn copy(&mut self, text: String, primary: bool) {
        if text.is_empty() {
            return;
        }
        let Some(clipboard) = self.clipboard() else {
            return;
        };
        let result = if primary {
            set_primary(clipboard, text)
        } else {
            clipboard.set_text(text)
        };
        if let Err(e) = result {
            log::warn!("cannot copy to the clipboard: {e}");
        }
    }

    /// The text on the clipboard.
    fn paste(&mut self) -> Option<String> {
        self.clipboard()?.get_text().ok()
    }

    /// The system clipboard. Opens it on first use; logs a warning and
    /// returns `None` if it cannot be opened.
    fn clipboard(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new()
                .map_err(|e| log::warn!("cannot open the clipboard: {e}"))
                .ok();
        }
        self.clipboard.as_mut()
    }

    /// The click count of a press at (x, y): 2 or 3 if it follows earlier
    /// presses closely, otherwise 1.
    fn click_count(&mut self, x: f32, y: f32) -> u32 {
        let now = Instant::now();
        let count = match self.last_press {
            Some((time, (px, py), count))
                if now.duration_since(time) <= MULTI_CLICK_TIME
                    && (x - px).hypot(y - py) <= MULTI_CLICK_DISTANCE =>
            {
                count % 3 + 1
            }
            _ => 1,
        };
        self.last_press = Some((now, (x, y), count));
        count
    }

    fn scale(&self) -> f32 {
        self.gfx
            .as_ref()
            .map_or(1.0, |g| g.window.scale_factor() as f32)
    }

    /// Window size in CSS px.
    fn window_size(&self) -> Size {
        let Some(gfx) = &self.gfx else {
            return SIZE_WITHOUT_WINDOW;
        };
        let s = gfx.window.inner_size();
        let scale = self.scale();
        Size::new(s.width as f32 / scale, s.height as f32 / scale)
    }

    fn update_viewport(&mut self) {
        let size = self.window_size();
        let scale = self.scale();
        self.page.set_viewport(
            Size::new(size.width, (size.height - TOOLBAR_HEIGHT).max(1.0)),
            scale,
        );
    }

    fn request_redraw(&self) {
        if let Some(gfx) = &self.gfx {
            gfx.window.request_redraw();
        }
    }

    fn sync_address(&mut self) {
        if !self.toolbar.focused {
            let text = self.page.url().map_or("", Url::as_str).to_owned();
            if text != self.toolbar.address.text() {
                self.toolbar.address.set_text(&text);
            }
        }
        if let Some(gfx) = &self.gfx {
            let title = match self.page.title() {
                "" => "swb".to_owned(),
                t => format!("{t} — swb"),
            };
            gfx.window.set_title(&title);
        }
    }

    fn navigate_to_input(&mut self) {
        match parse_address(self.toolbar.address.text()) {
            Ok(url) => {
                self.toolbar.focused = false;
                self.page.navigate(url);
            }
            Err(e) => log::warn!("invalid address: {e}"),
        }
        self.sync_address();
    }

    fn redraw(&mut self) -> Result<()> {
        let scale = self.scale();
        let Some(gfx) = &mut self.gfx else {
            return Ok(());
        };
        let phys = gfx.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(phys.width), NonZeroU32::new(phys.height)) else {
            return Ok(());
        };
        gfx.surface
            .resize(w, h)
            .map_err(|e| anyhow::anyhow!("resize failed: {e}"))?;

        let frame = compose_frame(&mut self.page, &self.toolbar, w.get(), h.get(), scale)?;

        let Some(gfx) = &mut self.gfx else {
            return Ok(());
        };
        let mut buffer = gfx
            .surface
            .buffer_mut()
            .map_err(|e| anyhow::anyhow!("buffer failed: {e}"))?;
        let (pixels, _) = frame.data().as_chunks::<4>();
        for (dst, src) in buffer.iter_mut().zip(pixels) {
            *dst = (u32::from(src[0]) << 16) | (u32::from(src[1]) << 8) | u32::from(src[2]);
        }
        buffer
            .present()
            .map_err(|e| anyhow::anyhow!("present failed: {e}"))?;
        Ok(())
    }

    /// Executes automation requests. Returns true if one was executed.
    fn process_automation(&mut self, event_loop: &ActiveEventLoop) -> bool {
        let Some(automation) = &mut self.automation else {
            return false;
        };
        let executed = automation.process(&mut self.page);
        if automation.close_requested() {
            automation.finish_close();
            event_loop.exit();
        }
        executed
    }

    fn handle_key(&mut self, event_loop: &ActiveEventLoop, event: &KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        let ctrl = self.modifiers.control_key();
        let alt = self.modifiers.alt_key();

        // Global shortcuts.
        match &event.logical_key {
            Key::Character(c)
                if ctrl && (c.eq_ignore_ascii_case("q") || c.eq_ignore_ascii_case("w")) =>
            {
                event_loop.exit();
                return;
            }
            Key::Character(c) if ctrl && c.eq_ignore_ascii_case("l") => {
                self.toolbar.focused = true;
                self.toolbar.address.select_all();
            }
            Key::Character(c) if ctrl && c.eq_ignore_ascii_case("r") => self.page.reload(),
            Key::Named(NamedKey::F5) => self.page.reload(),
            Key::Named(NamedKey::ArrowLeft) if alt => {
                self.page.go_back();
                self.sync_address();
            }
            Key::Named(NamedKey::ArrowRight) if alt => {
                self.page.go_forward();
                self.sync_address();
            }
            _ if self.toolbar.focused => self.handle_address_key(event),
            _ => self.handle_page_key(event),
        }
        self.request_redraw();
    }

    /// A key press while the address field has the focus.
    fn handle_address_key(&mut self, event: &KeyEvent) {
        let ctrl = self.modifiers.control_key();
        let shift = self.modifiers.shift_key();
        let field = &mut self.toolbar.address;
        match &event.logical_key {
            Key::Named(NamedKey::Enter) => {
                self.navigate_to_input();
            }
            Key::Named(NamedKey::Escape) => {
                self.toolbar.focused = false;
                self.sync_address();
            }
            Key::Named(NamedKey::Backspace) => {
                field.backspace();
            }
            Key::Named(NamedKey::Delete) => {
                field.delete();
            }
            Key::Named(NamedKey::ArrowLeft) => field.left(shift),
            Key::Named(NamedKey::ArrowRight) => field.right(shift),
            Key::Named(NamedKey::Home) => field.home(shift),
            Key::Named(NamedKey::End) => field.end(shift),
            Key::Character(c) if ctrl && c.eq_ignore_ascii_case("a") => field.select_all(),
            Key::Character(c)
                if ctrl && (c.eq_ignore_ascii_case("c") || c.eq_ignore_ascii_case("x")) =>
            {
                let range = field.selection();
                if !range.is_empty() {
                    let text = field.text().get(range).unwrap_or("").to_owned();
                    if c.eq_ignore_ascii_case("x") {
                        field.delete_selection();
                    }
                    self.copy(text, false);
                }
            }
            Key::Character(c) if ctrl && c.eq_ignore_ascii_case("v") => {
                if let Some(text) = self.paste() {
                    // The address is one line.
                    let line = text.replace(['\n', '\r'], " ");
                    self.toolbar.address.insert(line.trim());
                }
            }
            _ if !ctrl => {
                // Tab and other control keys produce text too.
                if let Some(text) = event
                    .text
                    .as_deref()
                    .filter(|t| !t.chars().any(char::is_control))
                {
                    field.insert(text);
                }
            }
            _ => {}
        }
    }

    /// A key press while the page has the focus: copy, back and stop here;
    /// the page handles focus navigation, activation and scrolling. While
    /// a text field of the page has the focus, typed text, Backspace and
    /// cut and paste go to the field.
    fn handle_page_key(&mut self, event: &KeyEvent) {
        let modifiers = self.engine_modifiers();
        let editing = self.page.has_editable_focus();
        match &event.logical_key {
            Key::Character(c) if modifiers.ctrl && c.eq_ignore_ascii_case("c") => {
                let text = self.page.selected_text();
                self.copy(text, false);
            }
            Key::Character(c) if editing && modifiers.ctrl && c.eq_ignore_ascii_case("x") => {
                let text = self.page.cut_selection();
                self.copy(text, false);
            }
            Key::Character(c) if editing && modifiers.ctrl && c.eq_ignore_ascii_case("v") => {
                if let Some(text) = self.paste() {
                    self.page.insert_text(&text);
                }
            }
            // Typed text (also composed characters and Space) without
            // shortcut modifiers.
            _ if editing
                && !modifiers.ctrl
                && !modifiers.alt
                && !modifiers.meta
                && event
                    .text
                    .as_deref()
                    .is_some_and(|t| !t.chars().any(char::is_control)) =>
            {
                if let Some(text) = &event.text {
                    self.page.insert_text(text);
                }
            }
            Key::Named(NamedKey::Backspace) if modifiers.is_empty() && !editing => {
                self.page.go_back();
                self.sync_address();
            }
            Key::Named(NamedKey::Escape) => {
                self.page.stop();
                self.sync_address();
            }
            key => {
                if let Some(key) = engine_key(key) {
                    self.page.key_down(&key, modifiers);
                    self.sync_address();
                }
            }
        }
    }

    fn handle_mouse_button(&mut self, button: MouseButton, state: ElementState) {
        let scale = self.scale();
        let x = self.cursor.x as f32 / scale;
        let y = self.cursor.y as f32 / scale;
        match (button, state) {
            (MouseButton::Left, ElementState::Pressed) => {
                if y < TOOLBAR_HEIGHT {
                    self.press_toolbar(x, y);
                } else {
                    self.toolbar.focused = false;
                    let y = y - TOOLBAR_HEIGHT;
                    let count = self.click_count(x, y);
                    let modifiers = self.engine_modifiers();
                    self.page_pressed = true;
                    self.page
                        .mouse_down(x, y, swb_engine::MouseButton::Primary, modifiers, count);
                }
                self.sync_address();
                self.request_redraw();
            }
            (MouseButton::Left, ElementState::Released) => {
                if self.selecting {
                    self.selecting = false;
                } else if self.page_pressed {
                    self.page_pressed = false;
                    self.page
                        .mouse_up(x, y - TOOLBAR_HEIGHT, swb_engine::MouseButton::Primary);
                    let selected = self.page.selected_text();
                    self.copy(selected, true);
                    self.sync_address();
                    self.request_redraw();
                }
            }
            (MouseButton::Back, ElementState::Pressed) => {
                self.page.go_back();
                self.sync_address();
                self.request_redraw();
            }
            (MouseButton::Forward, ElementState::Pressed) => {
                self.page.go_forward();
                self.sync_address();
                self.request_redraw();
            }
            _ => {}
        }
    }

    /// A press of the left button at (x, y) in the toolbar (CSS px of the
    /// window).
    fn press_toolbar(&mut self, x: f32, y: f32) {
        match Toolbar::hit(x, y, self.window_size().width) {
            Some(ToolbarHit::Back) => {
                self.page.go_back();
            }
            Some(ToolbarHit::Forward) => {
                self.page.go_forward();
            }
            Some(ToolbarHit::Reload) => {
                // The button shows "stop" while loading.
                if self.page.is_loading() {
                    self.page.stop();
                } else {
                    self.page.reload();
                }
            }
            Some(ToolbarHit::Address(text_x)) => {
                if self.toolbar.focused {
                    self.toolbar.place_cursor(
                        self.page.fonts(),
                        text_x,
                        self.modifiers.shift_key(),
                    );
                    self.selecting = true;
                } else {
                    self.toolbar.focused = true;
                    self.toolbar.address.select_all();
                }
            }
            None => {}
        }
    }

    fn handle_cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        self.cursor = position;
        let scale = self.scale();
        let x = position.x as f32 / scale;
        let y = position.y as f32 / scale;
        if self.selecting {
            // Dragging outside the field selects to the start or the end.
            let text_x = Toolbar::text_x(x, self.window_size().width);
            self.toolbar.place_cursor(self.page.fonts(), text_x, true);
            self.request_redraw();
            return;
        }
        // While the button is held after a press in the page, the page
        // gets the movement also above the page area (drag selection).
        let in_page = y >= TOOLBAR_HEIGHT || self.page_pressed;
        let changed = if in_page {
            self.page.mouse_move(x, y - TOOLBAR_HEIGHT)
        } else {
            self.page.mouse_leave()
        };
        if let Some(gfx) = &self.gfx {
            let icon = if in_page {
                cursor_icon(self.page.cursor())
            } else {
                match Toolbar::hit(x, y, self.window_size().width) {
                    Some(ToolbarHit::Address(_)) => Some(CursorIcon::Text),
                    Some(_) => Some(CursorIcon::Pointer),
                    None => Some(CursorIcon::Default),
                }
            };
            gfx.window.set_cursor_visible(icon.is_some());
            if let Some(icon) = icon {
                gfx.window.set_cursor(icon);
            }
        }
        if changed {
            self.request_redraw();
        }
    }
}

/// The engine's form of a key, for keys that the page handles.
fn engine_key(key: &Key) -> Option<swb_engine::Key> {
    use swb_engine::Key as K;
    Some(match key {
        Key::Character(c) => K::Character(c.to_string()),
        Key::Named(NamedKey::Space) => K::Character(" ".to_owned()),
        Key::Named(named) => match named {
            NamedKey::Tab => K::Tab,
            NamedKey::Enter => K::Enter,
            NamedKey::Escape => K::Escape,
            NamedKey::Backspace => K::Backspace,
            NamedKey::Delete => K::Delete,
            NamedKey::ArrowUp => K::ArrowUp,
            NamedKey::ArrowDown => K::ArrowDown,
            NamedKey::ArrowLeft => K::ArrowLeft,
            NamedKey::ArrowRight => K::ArrowRight,
            NamedKey::PageUp => K::PageUp,
            NamedKey::PageDown => K::PageDown,
            NamedKey::Home => K::Home,
            NamedKey::End => K::End,
            _ => return None,
        },
        _ => return None,
    })
}

/// The window cursor for a CSS cursor; `None` hides the cursor.
fn cursor_icon(cursor: Cursor) -> Option<CursorIcon> {
    Some(match cursor {
        Cursor::None => return None,
        Cursor::Auto | Cursor::Default => CursorIcon::Default,
        Cursor::Pointer => CursorIcon::Pointer,
        Cursor::Text => CursorIcon::Text,
        Cursor::VerticalText => CursorIcon::VerticalText,
        Cursor::Help => CursorIcon::Help,
        Cursor::Wait => CursorIcon::Wait,
        Cursor::Progress => CursorIcon::Progress,
        Cursor::Crosshair => CursorIcon::Crosshair,
        Cursor::Move => CursorIcon::Move,
        Cursor::Grab => CursorIcon::Grab,
        Cursor::Grabbing => CursorIcon::Grabbing,
        Cursor::NotAllowed => CursorIcon::NotAllowed,
        Cursor::NoDrop => CursorIcon::NoDrop,
        Cursor::ContextMenu => CursorIcon::ContextMenu,
        Cursor::Cell => CursorIcon::Cell,
        Cursor::Copy => CursorIcon::Copy,
        Cursor::Alias => CursorIcon::Alias,
        Cursor::ColResize => CursorIcon::ColResize,
        Cursor::RowResize => CursorIcon::RowResize,
        Cursor::EwResize => CursorIcon::EwResize,
        Cursor::NsResize => CursorIcon::NsResize,
        Cursor::NeswResize => CursorIcon::NeswResize,
        Cursor::NwseResize => CursorIcon::NwseResize,
        Cursor::NResize => CursorIcon::NResize,
        Cursor::EResize => CursorIcon::EResize,
        Cursor::SResize => CursorIcon::SResize,
        Cursor::WResize => CursorIcon::WResize,
        Cursor::NeResize => CursorIcon::NeResize,
        Cursor::NwResize => CursorIcon::NwResize,
        Cursor::SeResize => CursorIcon::SeResize,
        Cursor::SwResize => CursorIcon::SwResize,
        Cursor::AllScroll => CursorIcon::AllScroll,
        Cursor::ZoomIn => CursorIcon::ZoomIn,
        Cursor::ZoomOut => CursorIcon::ZoomOut,
    })
}

/// Splits a wheel delta (CSS px) into the whole pixels to scroll now and
/// the fraction to keep for later, on each axis (toward zero, so that
/// small deltas in either direction add up). A component that is not
/// finite (a sum that overflowed) counts as 0.
fn split_wheel((dx, dy): (f32, f32)) -> ((f32, f32), (f32, f32)) {
    let finite = |v: f32| if v.is_finite() { v } else { 0.0 };
    let (dx, dy) = (finite(dx), finite(dy));
    let whole = (dx.trunc(), dy.trunc());
    (whole, (dx - whole.0, dy - whole.1))
}

/// Sets the primary selection (Linux), which a middle click pastes.
#[cfg(target_os = "linux")]
fn set_primary(clipboard: &mut arboard::Clipboard, text: String) -> Result<(), arboard::Error> {
    use arboard::{LinuxClipboardKind, SetExtLinux};
    clipboard
        .set()
        .clipboard(LinuxClipboardKind::Primary)
        .text(text)
}

/// Other platforms have no primary selection.
#[cfg(not(target_os = "linux"))]
fn set_primary(_clipboard: &mut arboard::Clipboard, _text: String) -> Result<(), arboard::Error> {
    Ok(())
}

/// Renders a whole window frame: the page below the toolbar, then the
/// toolbar and the status bubble. Sizes are in device pixels.
fn compose_frame(
    page: &mut Page,
    toolbar: &Toolbar,
    width: u32,
    height: u32,
    scale: f32,
) -> Result<Pixmap> {
    let mut frame = Pixmap::new(width, height).context("cannot allocate frame")?;
    let toolbar_px = (TOOLBAR_HEIGHT * scale).round() as u32;
    let content_h = height.saturating_sub(toolbar_px).max(1);
    let mut content = Pixmap::new(width, content_h).context("cannot allocate page")?;
    page.render(&mut content);
    frame.draw_pixmap(
        0,
        toolbar_px as i32,
        content.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        tiny_skia::Transform::identity(),
        None,
    );

    let status = page.hovered_link().map(|u| u.as_str().to_owned());
    let state = ToolbarState {
        can_go_back: page.can_go_back(),
        can_go_forward: page.can_go_forward(),
        loading: page.is_loading(),
        status: status.as_deref(),
    };
    let css_width = width as f32 / scale;
    let css_height = height as f32 / scale;
    let list = toolbar.display_list(page.fonts(), css_width, css_height, &state);
    swb_paint::rasterize(
        &list,
        &mut frame,
        RasterParams {
            scroll: swb_layout::Point::default(),
            viewport_scroll: swb_layout::Point::default(),
            scale,
        },
        page.fonts(),
        &NoImages,
    );
    Ok(frame)
}

/// Renders what the browser window would show for `page` (toolbar included)
/// at a window size in CSS px. Used by headless mode to test the user
/// interface without a display.
pub(crate) fn render_window(page: &mut Page, window: Size, scale: f32) -> Result<Pixmap> {
    let mut toolbar = Toolbar::default();
    toolbar.address.set_text(page.url().map_or("", Url::as_str));
    page.set_scroll_indicators(true);
    page.set_viewport(
        Size::new(window.width, (window.height - TOOLBAR_HEIGHT).max(1.0)),
        scale,
    );
    let w = (window.width * scale).round() as u32;
    let h = (window.height * scale).round() as u32;
    compose_frame(page, &toolbar, w.max(1), h.max(1), scale)
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("swb")
            .with_inner_size(LogicalSize::new(1280.0, 900.0));
        let window = match event_loop.create_window(attributes) {
            Ok(w) => Rc::new(w),
            Err(e) => {
                log::error!("cannot create window: {e}");
                event_loop.exit();
                return;
            }
        };
        let context = match softbuffer::Context::new(Rc::clone(&window)) {
            Ok(c) => c,
            Err(e) => {
                log::error!("cannot create graphics context: {e}");
                event_loop.exit();
                return;
            }
        };
        let surface = match softbuffer::Surface::new(&context, Rc::clone(&window)) {
            Ok(s) => s,
            Err(e) => {
                log::error!("cannot create surface: {e}");
                event_loop.exit();
                return;
            }
        };
        self.gfx = Some(Gfx { window, surface });
        self.update_viewport();
        match self.start_url.take() {
            Some(url) => self.page.navigate(url),
            None => {
                self.toolbar.focused = true;
            }
        }
        self.sync_address();
        self.request_redraw();
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        let changed = match event {
            UserEvent::Network => self.page.process_network(),
            UserEvent::Automation => false,
        };
        let automated = self.process_automation(event_loop);
        if changed || automated {
            self.sync_address();
            self.request_redraw();
        }
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::ResumeTimeReached { .. })
            && self.process_automation(event_loop)
        {
            self.request_redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // The wheel events of this batch scroll once.
        if self.apply_wheel() {
            self.request_redraw();
        }
        // Wake up when a waiting automation request times out.
        let deadline = self.automation.as_ref().and_then(Automation::next_deadline);
        event_loop.set_control_flow(deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.update_viewport();
                self.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.redraw() {
                    log::error!("redraw failed: {e:#}");
                }
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::KeyboardInput { event, .. } => self.handle_key(event_loop, &event),
            WindowEvent::CursorMoved { position, .. } => self.handle_cursor_moved(position),
            WindowEvent::CursorLeft { .. } => {
                if self.page.mouse_leave() {
                    self.request_redraw();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.handle_mouse_button(button, state);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (-x * LINE_SCROLL, -y * LINE_SCROLL),
                    MouseScrollDelta::PixelDelta(p) => {
                        let scale = f64::from(self.scale());
                        (-(p.x / scale) as f32, -(p.y / scale) as f32)
                    }
                };
                // Applied after this batch of events (`apply_wheel` in
                // `about_to_wait`).
                if dx.is_finite() && dy.is_finite() {
                    self.pending_wheel.0 += dx;
                    self.pending_wheel.1 += dy;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_deltas_scroll_whole_pixels_and_keep_the_fraction() {
        assert_eq!(split_wheel((48.0, -2.75)), ((48.0, -2.0), (0.0, -0.75)));
        // Half-pixel deltas (a touchpad at scale 2) add up in both
        // directions.
        let (whole, rest) = split_wheel((0.0, -0.5));
        assert_eq!((whole, rest), ((0.0, 0.0), (0.0, -0.5)));
        let (whole, _) = split_wheel((rest.0, rest.1 - 0.5));
        assert_eq!(whole, (0.0, -1.0));
        // An overflowed sum is dropped, not kept as NaN.
        assert_eq!(split_wheel((f32::INFINITY, 3.5)), ((0.0, 3.0), (0.0, 0.5)));
    }
}
