//! Page lifecycle: loading, the rendering pipeline, input, navigation and
//! history.
//!
//! The engine has no windowing code. The GUI shell and the headless runner
//! both drive a [`Page`].

mod boxes;
mod history;
mod hit_test;
mod page;
mod resources;

pub use boxes::{ElementBox, element_boxes};
pub use hit_test::HitResult;
pub use page::{LoadState, Page, PageConfig};
pub use swb_layout::{Point, Rect, Size};
pub use swb_net::Url;
pub use swb_paint::Pixmap;
pub use swb_text::FontContext;
