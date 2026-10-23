//! `<canvas>` (<https://html.spec.whatwg.org/multipage/canvas.html>).
//!
//! With scripting enabled for the document, a canvas is a replaced
//! element whose natural size is the size of its bitmap: the `width` and
//! `height` attributes, 300×150 by default. No script draws on it yet, so
//! it paints nothing (its box still paints background and border), and its
//! fallback content is not rendered. With scripting disabled, a canvas is
//! not replaced: it represents its fallback content (HTML), as in Chromium
//! with JavaScript disabled. The box tree decides (`box_tree.rs`); this
//! module has the size.

use swb_dom::ElementData;

/// The size of the bitmap of `<canvas>` element `canvas` in CSS px
/// ([`swb_dom::canvas_size`]).
pub(crate) fn size(canvas: &ElementData) -> (f32, f32) {
    let (width, height) = swb_dom::canvas_size(canvas);
    (width as f32, height as f32)
}

#[cfg(test)]
mod tests {
    use crate::test_support::{layout_html, layout_html_scripting};
    use crate::{BoxContent, FragmentRef};

    #[test]
    fn size_from_attributes() {
        let html = "<!DOCTYPE html><body style='margin:0'><canvas id=c1></canvas>\
            <canvas id=c2 width=100 height=50></canvas><canvas id=c3 width=80></canvas>\
            <canvas id=c4 height=40></canvas><canvas id=c5 width=abc height=-5></canvas>\
            <canvas id=c6 width=0 height=20></canvas>\
            <canvas id=c7 width=2147483648 height=2147483647></canvas>";
        let layout = layout_html_scripting(html);
        let size = |id: &str| {
            let r = layout.rect(id);
            (r.width, r.height)
        };
        assert_eq!(size("c1"), (300.0, 150.0));
        assert_eq!(size("c2"), (100.0, 50.0));
        assert_eq!(size("c3"), (80.0, 150.0));
        assert_eq!(size("c4"), (300.0, 40.0));
        assert_eq!(size("c5"), (300.0, 150.0));
        assert_eq!(size("c6"), (0.0, 20.0));
        assert_eq!(size("c7").0, 300.0);
        // Lengths are clamped (`geom::clamp_length`).
        assert!(size("c7").1 >= 1_000_000.0);
    }

    #[test]
    fn replaced_with_scripting_and_painting_nothing() {
        let html = "<!DOCTYPE html><body style='margin:0;font:16px/20px sans-serif'>\
            <div id=d style='width:200px'><canvas id=c width=100 height=50 style='width:50%'>\
            fallback <b id=b>text</b></canvas></div>";
        let on = layout_html_scripting(html);
        let rect = on.rect("c");
        assert_eq!((rect.width, rect.height), (100.0, 50.0));
        assert!(!on.tree.element_boxes().contains_key(&on.node("b")));
        assert!(
            on.texts().is_empty(),
            "the fallback content is not rendered"
        );
        let mut contents = 0;
        on.tree.walk(|f, _| {
            if let FragmentRef::Box(b) = f
                && b.node == Some(on.node("c"))
            {
                assert_eq!(b.content, BoxContent::None);
                contents += 1;
            }
        });
        assert_eq!(contents, 1);

        // Scripting off: the fallback content is rendered, in a non-replaced
        // inline box.
        let off = layout_html(html);
        assert!(off.tree.element_boxes().contains_key(&off.node("b")));
        assert!(off.texts().iter().any(|(_, t)| t.contains("fallback")));
        assert_ne!(off.rect("c").width, 100.0);
    }

    #[test]
    fn huge_sizes_do_not_panic() {
        let html =
            "<!DOCTYPE html><canvas id=c width=2147483647 height=2147483647></canvas><p>x</p>";
        let layout = layout_html_scripting(html);
        let r = layout.rect("c");
        assert!(r.width.is_finite() && r.height.is_finite());
    }
}
