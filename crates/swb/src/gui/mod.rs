//! The windowed browser: a winit window, a softbuffer surface, the toolbar
//! and one page.

mod compose;
mod framelog;
mod input_batch;
mod toolbar;

use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use swb_automation::Automation;
use swb_engine::{ClockMode, Cursor, Modifiers, Page, PageClock, PageConfig, Pixmap, Size, Url};
use swb_net::Fetcher;
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
use compose::{Composer, ToolbarKey, compose_window, device_size};
use framelog::FrameLog;
use input_batch::{Input, InputBatch};
use toolbar::{TOOLBAR_HEIGHT, Toolbar, ToolbarHit};

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
    (javascript, animations): (bool, bool),
    now: Option<f64>,
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
    let mut app = App::new(fetcher, fonts, start_url, proxy, javascript);
    app.page.set_clock(PageClock::new(ClockMode::Real, now));
    app.page.set_animations(animations);
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
    /// The pointer move and the wheel delta that the current batch of
    /// window events left for its end (ADR 0040 section 1).
    batch: InputBatch,
    /// What the last frame showed besides the page; `None` before the first
    /// frame. A frame is drawn when it differs from the state now, or when
    /// the page's frame is outdated (ADR 0040 section 2).
    drawn: Option<FrameKey>,
    /// True after a frame failed (the surface or the buffer): no frame is
    /// requested until the window changes or the user's next input, so a
    /// lasting failure does not loop and a passing one does not leave the
    /// window stale.
    frame_failed: bool,
    /// The page title that the window title shows (`None`: not set yet).
    shown_title: Option<String>,
    /// The mouse cursor that the window shows (ADR 0040 section 4).
    shown_cursor: ShownCursor,
    /// The input of the current batch of window events, for the frame log.
    frame_log: FrameLog,
    /// The kept pixmaps of the frame (`None` before the first frame).
    composer: Option<Composer>,
}

/// What a frame shows besides the page (see [`App::frame_wanted`]).
#[derive(Clone, Debug, PartialEq)]
struct FrameKey {
    /// The window size in device pixels.
    size: (u32, u32),
    scale: f32,
    /// The inputs of the toolbar.
    toolbar: ToolbarKey,
    /// The hovered link (the status bubble).
    status: Option<String>,
}

/// The mouse cursor that the window shows.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ShownCursor {
    /// Not set yet (a new window).
    Unset,
    /// Hidden (`cursor: none`).
    Hidden,
    /// The icon.
    Icon(CursorIcon),
}

impl ShownCursor {
    fn of(icon: Option<CursorIcon>) -> ShownCursor {
        icon.map_or(ShownCursor::Hidden, ShownCursor::Icon)
    }

    fn is_visible(self) -> bool {
        matches!(self, ShownCursor::Icon(_))
    }
}

impl App {
    fn new(
        fetcher: Arc<dyn Fetcher>,
        fonts: FontContext,
        start_url: Option<Url>,
        proxy: EventLoopProxy<UserEvent>,
        javascript: bool,
    ) -> Self {
        let notify = Arc::new(move || {
            // The event loop may already be gone at shutdown.
            let _ = proxy.send_event(UserEvent::Network);
        });
        // The page runs on the main thread: its stack limit must be enough
        // for JavaScript (ADR 0028 section 2). Without it, the page keeps
        // JavaScript off whatever sets it.
        let scripts_allowed = crate::stack::main_stack_is_enough();
        if !scripts_allowed {
            log::warn!(
                "the stack limit of the process is below {} MiB: JavaScript stays off (see `ulimit -s`)",
                crate::stack::MIN_MAIN_STACK / (1024 * 1024)
            );
        }
        let config = PageConfig {
            fetcher,
            notify,
            network_threads: PageConfig::DEFAULT_NETWORK_THREADS,
            scripts_allowed,
        };
        let mut page = Page::new(config, fonts, SIZE_WITHOUT_WINDOW, 1.0);
        page.set_javascript(javascript && scripts_allowed);
        // The window paints frames: running animations get a rendering
        // update at each frame (ADR 0044 section 8).
        page.set_paints_frames(true);
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
            batch: InputBatch::default(),
            drawn: None,
            frame_failed: false,
            shown_title: None,
            shown_cursor: ShownCursor::Unset,
            frame_log: FrameLog::new(),
            composer: None,
        }
    }

    /// Scrolls by the whole pixels of the wheel delta collected since the
    /// last batch of events: over the page, the innermost scroll container
    /// under the pointer that can scroll in that direction, else the page;
    /// over the toolbar, the page.
    fn apply_wheel(&mut self) {
        let (whole_x, whole_y) = self.batch.take_wheel();
        if whole_x == 0.0 && whole_y == 0.0 {
            return;
        }
        let scale = self.scale();
        let x = self.cursor.x as f32 / scale;
        let y = self.cursor.y as f32 / scale - TOOLBAR_HEIGHT;
        if y >= 0.0 {
            self.page.wheel(x, y, whole_x, whole_y);
        } else {
            self.page.wheel_page(whole_x, whole_y);
        }
    }

    /// Notes an input event and applies the pointer move that it must see
    /// (ADR 0040 section 1).
    fn before_input(&mut self, input: Input) {
        // The user's input tries a failed frame again.
        self.frame_failed = false;
        if let Some(position) = self.batch.push(input) {
            self.apply_move(position);
        }
    }

    /// Applies the pending pointer move, for work that reads the page's
    /// pointer state.
    fn flush_move(&mut self) {
        if let Some(position) = self.batch.take_move() {
            self.apply_move(position);
        }
    }

    /// The state of the window that a frame shows, besides the page: the
    /// toolbar, the status bubble, the window size and the scale.
    fn frame_key(&self) -> Option<FrameKey> {
        let gfx = self.gfx.as_ref()?;
        let size = gfx.window.inner_size();
        Some(FrameKey {
            size: (size.width, size.height),
            scale: self.scale(),
            toolbar: ToolbarKey::of(&self.page, &self.toolbar),
            status: self.page.hovered_link().map(|u| u.as_str().to_owned()),
        })
    }

    /// True if a frame would differ from the last one: the page's frame is
    /// outdated, or the toolbar or the window changed (ADR 0040 section 2).
    /// A window without pixels (minimized) shows nothing.
    fn frame_wanted(&self) -> bool {
        if self.frame_failed {
            return false;
        }
        let Some(key) = self.frame_key() else {
            return false;
        };
        if key.size.0 == 0 || key.size.1 == 0 {
            return false;
        }
        self.drawn.as_ref() != Some(&key) || self.page.frame_outdated()
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
        // The window title only when the page title changed (ADR 0040
        // section 4): on Wayland a new title draws the decorations again.
        let page_title = self.page.title();
        if let Some(gfx) = &self.gfx
            && self.shown_title.as_deref() != Some(page_title)
        {
            let title = match page_title {
                "" => "swb".to_owned(),
                t => format!("{t} — swb"),
            };
            gfx.window.set_title(&title);
            self.shown_title = Some(page_title.to_owned());
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
        let started = Instant::now();
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

        // The layers stay between frames; a new window size or scale makes
        // new ones (ADR 0040 section 5).
        let size = (w.get(), h.get());
        if !self.composer.as_ref().is_some_and(|c| c.fits(size, scale)) {
            self.composer = None;
            self.composer = Some(Composer::new(size, scale)?);
        }
        let Some(composer) = &mut self.composer else {
            return Ok(());
        };
        let rendered = composer.compose(&mut self.page, &self.toolbar);

        let Some(gfx) = &mut self.gfx else {
            return Ok(());
        };
        let mut buffer = gfx
            .surface
            .buffer_mut()
            .map_err(|e| anyhow::anyhow!("buffer failed: {e}"))?;
        // A buffer that holds the last frame (age 1) gets only the rows that
        // changed; any other gets every pixel.
        let changed = composer.take_changed_rows().filter(|_| buffer.age() == 1);
        match &changed {
            Some(rows) => composer.write_rows(&mut buffer, rows),
            None => composer.write_buffer(&mut buffer),
        }
        let composed = Instant::now();
        // The compositor's frame callback paces the next frame (ADR 0040
        // section 3).
        gfx.window.pre_present_notify();
        let presented = match changed {
            Some(rows) => buffer.present_with_damage(&damage_rects(&rows, w)),
            None => buffer.present(),
        };
        presented.map_err(|e| anyhow::anyhow!("present failed: {e}"))?;
        self.frame_log
            .frame(started, rendered, composed, composer.damage.as_ref());
        self.drawn = self.frame_key();
        Ok(())
    }

    /// Executes automation requests. Returns true if one was executed.
    fn process_automation(&mut self, event_loop: &ActiveEventLoop) -> bool {
        if self.automation.is_none() {
            return false;
        }
        // A request reads the page's pointer state.
        self.flush_move();
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
            // The page sees the release of a key (`keyup`).
            if !self.toolbar.focused
                && let Some(key) = engine_key(&event.logical_key)
            {
                let modifiers = self.engine_modifiers();
                self.page.key_up(&key, modifiers);
            }
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
            }
            Key::Character(c) if ctrl && c.eq_ignore_ascii_case("l") => {
                self.toolbar.focused = true;
                self.toolbar.address.select_all();
            }
            Key::Character(c) if ctrl && c.eq_ignore_ascii_case("r") => self.page.reload(),
            // Ctrl+Shift+J: the JavaScript toggle.
            Key::Character(c)
                if ctrl && self.modifiers.shift_key() && c.eq_ignore_ascii_case("j") =>
            {
                self.toggle_javascript();
            }
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
                    self.page.paste_text(&text);
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
                    let mut chars = text.chars();
                    let single = chars.next().is_some() && chars.next().is_none();
                    if single && self.page.has_script_runtime() {
                        // One character is a key press for the page's
                        // key events; a composed string is only text.
                        self.page
                            .key_down(&swb_engine::Key::Character(text.to_string()), modifiers);
                    } else {
                        self.page.insert_text(text);
                    }
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
                }
            }
            (MouseButton::Back, ElementState::Pressed) => {
                self.page.go_back();
                self.sync_address();
            }
            (MouseButton::Forward, ElementState::Pressed) => {
                self.page.go_forward();
                self.sync_address();
            }
            _ => {}
        }
    }

    /// Flips the JavaScript setting. It applies to the next document (a
    /// reload). It does nothing if the stack limit rules JavaScript out
    /// (the page refuses it).
    fn toggle_javascript(&mut self) {
        if !self.page.javascript_available() {
            log::warn!("JavaScript is not available: the stack limit of the process is too low");
            return;
        }
        let enabled = !self.page.javascript();
        self.page.set_javascript(enabled);
        log::info!(
            "JavaScript {} for the next document",
            if enabled { "on" } else { "off" }
        );
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
            Some(ToolbarHit::JavaScript) => self.toggle_javascript(),
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

    /// A `CursorMoved` event: the shell's record of the pointer follows at
    /// once; the call into the page waits for the end of the batch or for
    /// an event that needs it (ADR 0040 section 1).
    fn handle_cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        self.cursor = position;
        self.before_input(Input::Move(position));
    }

    /// Applies a pointer move: the toolbar's drag selection, or the page's
    /// hover, and the mouse cursor.
    fn apply_move(&mut self, position: PhysicalPosition<f64>) {
        let scale = self.scale();
        let x = position.x as f32 / scale;
        let y = position.y as f32 / scale;
        if self.selecting {
            // Dragging outside the field selects to the start or the end.
            let text_x = Toolbar::text_x(x, self.window_size().width);
            self.toolbar.place_cursor(self.page.fonts(), text_x, true);
            return;
        }
        // While the button is held after a press in the page, the page
        // gets the movement also above the page area (drag selection).
        let in_page = y >= TOOLBAR_HEIGHT || self.page_pressed;
        if in_page {
            self.page.mouse_move(x, y - TOOLBAR_HEIGHT);
        } else {
            self.page.mouse_leave();
        }
        let icon = if in_page {
            cursor_icon(self.page.cursor())
        } else {
            match Toolbar::hit(x, y, self.window_size().width) {
                Some(ToolbarHit::Address(_)) => Some(CursorIcon::Text),
                Some(_) => Some(CursorIcon::Pointer),
                None => Some(CursorIcon::Default),
            }
        };
        self.show_cursor(icon);
    }

    /// Sets the window's mouse cursor if it differs from the one shown
    /// (`None` hides it; ADR 0040 section 4).
    fn show_cursor(&mut self, icon: Option<CursorIcon>) {
        let Some(gfx) = &self.gfx else {
            return;
        };
        let shown = self.shown_cursor;
        let wanted = ShownCursor::of(icon);
        if shown == wanted {
            return;
        }
        if shown == ShownCursor::Unset || shown.is_visible() != wanted.is_visible() {
            gfx.window.set_cursor_visible(wanted.is_visible());
        }
        if let ShownCursor::Icon(icon) = wanted {
            gfx.window.set_cursor(icon);
        }
        self.shown_cursor = wanted;
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
    let (w, h) = device_size(window, scale);
    compose_window(page, &toolbar, (w.max(1), h.max(1)), scale)
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
        self.shown_title = None;
        self.shown_cursor = ShownCursor::Unset;
        self.drawn = None;
        self.update_viewport();
        match self.start_url.take() {
            Some(url) => self.page.navigate(url),
            None => {
                self.toolbar.focused = true;
            }
        }
        self.sync_address();
        // The first frame: `about_to_wait` finds no frame drawn.
        self.request_redraw();
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        if matches!(event, UserEvent::Network) {
            self.page.process_network();
        }
        self.process_automation(event_loop);
        // The frame decision comes at the end of the batch (`about_to_wait`).
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        self.frame_log.begin_batch();
        if matches!(cause, StartCause::ResumeTimeReached { .. }) {
            self.process_automation(event_loop);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // The last pointer move of this batch, then the wheel events of the
        // batch (they scroll once).
        self.flush_move();
        self.apply_wheel();
        // The page's event loop: the timers and other tasks that are due.
        let pump = self.page.pump();
        if pump.changed {
            // Tasks may have settled what a waiting request waits for.
            self.process_automation(event_loop);
        }
        // One frame decision per batch, after the pump and the input
        // (ADR 0040 section 2).
        self.sync_address();
        let frame = self.frame_wanted();
        if frame {
            self.request_redraw();
        }
        // Wake up when the next task is due or a waiting automation request
        // times out; otherwise wait for input or the network.
        let deadline = self.automation.as_ref().and_then(Automation::next_deadline);
        let deadline = match (deadline, self.page.next_task_instant()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        event_loop.set_control_flow(deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
        self.frame_log.end_batch(frame);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                // The page's pointer state comes first: a new viewport
                // moves the content under the pointer.
                self.flush_move();
                self.update_viewport();
                self.frame_failed = false;
            }
            WindowEvent::RedrawRequested => {
                // winit delivers the redraw after the turn's window events
                // and before `about_to_wait`: the frame shows the pending
                // move and wheel of this batch, instead of a second frame
                // after them.
                self.flush_move();
                self.apply_wheel();
                if let Err(e) = self.redraw() {
                    log::error!("redraw failed: {e:#}");
                    self.frame_failed = true;
                }
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::KeyboardInput { event, .. } => {
                self.frame_log.counts_mut().keys += 1;
                self.before_input(Input::Key);
                self.handle_key(event_loop, &event);
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.frame_log.counts_mut().moves += 1;
                self.handle_cursor_moved(position);
            }
            WindowEvent::CursorLeft { .. } => {
                self.before_input(Input::Leave);
                self.page.mouse_leave();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let counts = self.frame_log.counts_mut();
                match state {
                    ElementState::Pressed => counts.presses += 1,
                    ElementState::Released => counts.releases += 1,
                }
                self.before_input(Input::Button);
                self.handle_mouse_button(button, state);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.frame_log.counts_mut().wheels += 1;
                self.before_input(Input::Wheel);
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (-x * LINE_SCROLL, -y * LINE_SCROLL),
                    MouseScrollDelta::PixelDelta(p) => {
                        let scale = f64::from(self.scale());
                        (-(p.x / scale) as f32, -(p.y / scale) as f32)
                    }
                };
                // Applied after this batch of events (`apply_wheel` in
                // `about_to_wait`).
                self.batch.add_wheel(dx, dy);
            }
            _ => {}
        }
    }
}

/// The window rows `rows` as rectangles of the window's width `width`, for
/// `present_with_damage`.
fn damage_rects(rows: &[std::ops::Range<usize>], width: NonZeroU32) -> Vec<softbuffer::Rect> {
    rows.iter()
        .filter_map(|r| {
            let height = NonZeroU32::new(u32::try_from(r.len()).ok()?)?;
            Some(softbuffer::Rect {
                x: 0,
                y: u32::try_from(r.start).ok()?,
                width,
                height,
            })
        })
        .collect()
}
