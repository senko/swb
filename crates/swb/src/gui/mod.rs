//! The windowed browser: a winit window, a softbuffer surface, the toolbar
//! and one page.

mod text_field;
mod toolbar;

use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::{Context, Result};
use swb_engine::{Page, PageConfig, Pixmap, Size, Url};
use swb_net::Fetcher;
use swb_paint::{ImageRef, ImageSource, RasterParams};
use swb_text::FontContext;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

use crate::url_input::parse_address;
use toolbar::{TOOLBAR_HEIGHT, Toolbar, ToolbarHit, ToolbarState};

/// Pixels scrolled per wheel "line".
const LINE_SCROLL: f32 = 48.0;

/// Events sent to the event loop from other threads.
#[derive(Debug, Clone, Copy)]
enum UserEvent {
    /// A network request completed.
    Network,
}

/// Runs the GUI until the window is closed.
pub(crate) fn run(
    fetcher: Arc<dyn Fetcher>,
    fonts: FontContext,
    start_url: Option<Url>,
) -> Result<()> {
    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .context("cannot create the event loop (is a display available?)")?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = event_loop.create_proxy();
    let mut app = App::new(fetcher, fonts, start_url, proxy);
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
}

struct NoImages;

impl ImageSource for NoImages {
    fn pixmap(&self, _image: &ImageRef) -> Option<&Pixmap> {
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
            network_threads: 6,
        };
        App {
            page: Page::new(config, fonts, Size::new(1024.0, 768.0), 1.0),
            start_url,
            gfx: None,
            toolbar: Toolbar::default(),
            modifiers: ModifiersState::empty(),
            cursor: PhysicalPosition::new(0.0, 0.0),
            selecting: false,
        }
    }

    fn scale(&self) -> f32 {
        self.gfx
            .as_ref()
            .map_or(1.0, |g| g.window.scale_factor() as f32)
    }

    /// Window size in CSS px.
    fn window_size(&self) -> Size {
        let Some(gfx) = &self.gfx else {
            return Size::new(1024.0, 768.0);
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
            Key::Named(NamedKey::Backspace) => field.backspace(),
            Key::Named(NamedKey::Delete) => field.delete(),
            Key::Named(NamedKey::ArrowLeft) => field.left(shift),
            Key::Named(NamedKey::ArrowRight) => field.right(shift),
            Key::Named(NamedKey::Home) => field.home(shift),
            Key::Named(NamedKey::End) => field.end(shift),
            Key::Character(c) if ctrl && c.eq_ignore_ascii_case("a") => field.select_all(),
            _ if !ctrl => {
                if let Some(text) = &event.text {
                    field.insert(text);
                }
            }
            _ => {}
        }
    }

    /// A key press while the page has the focus: scrolling, back, stop.
    fn handle_page_key(&mut self, event: &KeyEvent) {
        let shift = self.modifiers.shift_key();
        let viewport = self.page.viewport();
        let page_step = (viewport.height * 0.875).max(LINE_SCROLL);
        let (dx, dy) = match &event.logical_key {
            Key::Named(NamedKey::ArrowDown) => (0.0, 40.0),
            Key::Named(NamedKey::ArrowUp) => (0.0, -40.0),
            Key::Named(NamedKey::ArrowRight) => (40.0, 0.0),
            Key::Named(NamedKey::ArrowLeft) => (-40.0, 0.0),
            Key::Named(NamedKey::PageDown) => (0.0, page_step),
            Key::Named(NamedKey::PageUp) => (0.0, -page_step),
            Key::Named(NamedKey::Space) => (0.0, if shift { -page_step } else { page_step }),
            // Scrolling clamps to the content.
            Key::Named(NamedKey::Home) => (0.0, -1e9),
            Key::Named(NamedKey::End) => (0.0, 1e9),
            Key::Named(NamedKey::Backspace) => {
                self.page.go_back();
                self.sync_address();
                return;
            }
            Key::Named(NamedKey::Escape) => {
                self.page.stop();
                self.sync_address();
                return;
            }
            _ => return,
        };
        self.page.scroll_by(dx, dy);
    }

    fn handle_mouse_button(&mut self, button: MouseButton, state: ElementState) {
        let scale = self.scale();
        let x = self.cursor.x as f32 / scale;
        let y = self.cursor.y as f32 / scale;
        match (button, state) {
            (MouseButton::Left, ElementState::Pressed) => {
                if y < TOOLBAR_HEIGHT {
                    let width = self.window_size().width;
                    match Toolbar::hit(x, y, width) {
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
                } else {
                    self.toolbar.focused = false;
                    self.page.click(x, y - TOOLBAR_HEIGHT);
                }
                self.sync_address();
                self.request_redraw();
            }
            (MouseButton::Left, ElementState::Released) => {
                self.selecting = false;
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
        let over_link = if y >= TOOLBAR_HEIGHT {
            self.page.mouse_move(x, y - TOOLBAR_HEIGHT)
        } else {
            self.page.mouse_leave()
        };
        if let Some(gfx) = &self.gfx {
            let icon = if y < TOOLBAR_HEIGHT {
                match Toolbar::hit(x, y, self.window_size().width) {
                    Some(ToolbarHit::Address(_)) => CursorIcon::Text,
                    Some(_) => CursorIcon::Pointer,
                    None => CursorIcon::Default,
                }
            } else if self.page.hovered_link().is_some() {
                CursorIcon::Pointer
            } else {
                CursorIcon::Default
            };
            gfx.window.set_cursor(icon);
        }
        if over_link {
            self.request_redraw();
        }
    }
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

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Network => {
                if self.page.process_network() {
                    self.sync_address();
                    self.request_redraw();
                }
            }
        }
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
                self.page.scroll_by(dx, dy);
                self.request_redraw();
            }
            _ => {}
        }
    }
}
