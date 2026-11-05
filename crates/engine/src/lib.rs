//! Page lifecycle: loading, the rendering pipeline, input, form controls,
//! navigation and history.
//!
//! The engine has no windowing code. The GUI shell and the headless runner
//! both drive a [`Page`].

mod boxes;
mod document;
mod edit;
mod focus;
mod forms;
mod frames;
mod history;
mod hit_test;
mod image_source;
mod input;
mod page;
mod resources;
mod scrollers;
mod selection;
mod web_fonts;

pub use boxes::{ElementBox, element_boxes};
pub use document::DocumentState;
pub use edit::TextEdit;
pub use frames::{
    DEFAULT_FRAME_VIEWPORT, FrameId, MAX_DEPTH, MAX_FONT_BYTES, MAX_FRAMES, MAX_IMAGE_BYTES,
    MAX_NAVIGATIONS, MAX_NODES, MAX_SOURCE_BYTES, MAX_STYLE_BYTES, NAVIGATION_WINDOW_MS, Sandbox,
};
pub use hit_test::HitResult;
pub use input::{Key, Modifiers, MouseButton};
pub use page::{
    ElementScroll, FlushCounters, LoadState, MAX_SCALE, MAX_SCREENSHOT_PIXELS, MAX_VIEWPORT_SIDE,
    Page, PageConfig, Pump, ScreenshotError, StageTimings, ViewportError, check_scale,
    check_viewport_size, device_size,
};
pub use selection::{Selection, TextPosition};
pub use swb_dom::NodeId;
pub use swb_layout::{Point, Rect, Size};
pub use swb_net::Url;
pub use swb_paint::Pixmap;
pub use swb_style::Cursor;
pub use swb_text::FontContext;
pub use swb_web::{
    ClockMode, ConsoleLevel, ConsoleMessage, DispatchTarget, Evaluation, EvaluationId,
    EvaluationStart, EventInterface, JsonForm, PageClock, ScriptRuntime, TrustedEvent,
};
