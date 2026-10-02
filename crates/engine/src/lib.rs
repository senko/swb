//! Page lifecycle: loading, the rendering pipeline, input, navigation and
//! history.
//!
//! The engine has no windowing code. The GUI shell and the headless runner
//! both drive a [`Page`].

mod boxes;
mod focus;
mod history;
mod hit_test;
mod input;
mod page;
mod resources;
mod selection;

pub use boxes::{ElementBox, element_boxes};
pub use hit_test::HitResult;
pub use input::{Key, Modifiers, MouseButton};
pub use page::{
    LoadState, MAX_SCREENSHOT_PIXELS, Page, PageConfig, ScreenshotError, StageTimings, device_size,
};
pub use selection::{Selection, TextPosition};
pub use swb_dom::NodeId;
pub use swb_layout::{Point, Rect, Size};
pub use swb_net::Url;
pub use swb_paint::Pixmap;
pub use swb_style::Cursor;
pub use swb_text::FontContext;
