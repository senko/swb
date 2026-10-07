//! Absolute, fixed and sticky positioning and transforms in pages: hit
//! testing (paint order, clips, scroll offsets), the painted element boxes,
//! the scrollable area and rendered pixels. The pages are files in a
//! temporary directory; nothing uses the network.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{Page, Point, Size};
use swb_net::NetworkFetcher;

mod common;
use common::Site;

/// Loads `body` (the content of a no-quirks document whose body has no
/// margin) in an 800×600 viewport.
fn page(test: &str, body: &str) -> (Site, Page) {
    let site = Site::new(test);
    let url = site.page(
        "page.html",
        &format!("<!DOCTYPE html><body style='margin:0; font: 16px/20px sans-serif'>{body}"),
    );
    let mut page = common::new_page(Arc::new(NetworkFetcher::new()), 2, Size::new(800.0, 600.0));
    page.navigate(url);
    common::finish_loading(&mut page, Duration::from_secs(20));
    (site, page)
}

/// The ID of the element at (x, y) in viewport coordinates, or of its
/// nearest ancestor with an ID.
fn id_at(page: &mut Page, x: f32, y: f32) -> Option<String> {
    let node = page.hit_test(x, y)?.node;
    let doc = page.document()?;
    std::iter::once(node)
        .chain(doc.ancestors(node))
        .find_map(|n| doc.element(n)?.attr("id").map(str::to_owned))
}

/// The color of the pixel at (x, y) of a screenshot of the viewport.
fn pixel(page: &mut Page, x: u32, y: u32) -> (u8, u8, u8) {
    let shot = page.screenshot(false).unwrap();
    let p = shot.pixel(x, y).unwrap().demultiply();
    (p.red(), p.green(), p.blue())
}

const RED: (u8, u8, u8) = (255, 0, 0);
const WHITE: (u8, u8, u8) = (255, 255, 255);

#[test]
fn fixed_boxes_stay_in_the_viewport_when_scrolling() {
    let (_site, mut page) = page(
        "fixed-scroll",
        "<div id=fixed style='position:fixed; top:0; left:0; width:100px; height:50px; background:red'></div>\
         <div id=long style='height:3000px'></div>",
    );
    assert_eq!(id_at(&mut page, 10.0, 10.0).as_deref(), Some("fixed"));
    page.scroll_to(Point::new(0.0, 1000.0));
    assert_eq!(id_at(&mut page, 10.0, 10.0).as_deref(), Some("fixed"));
    assert_eq!(id_at(&mut page, 10.0, 60.0).as_deref(), Some("long"));
    assert_eq!(pixel(&mut page, 10, 10), RED);
    assert_eq!(pixel(&mut page, 10, 60), WHITE);
    let fixed = common::node(&page, "fixed");
    let rect = page.element_box(fixed).unwrap();
    assert_eq!((rect.x, rect.y), (0.0, 1000.0));
}

#[test]
fn focusing_a_fixed_box_does_not_scroll() {
    let (_site, mut page) = page(
        "fixed-focus",
        "<div style='position:fixed; top:0'><a id=link href='#x'>link</a></div>\
         <div id=footer style='position:fixed; bottom:-10px; height:30px'><a id=low href='#y'>low</a></div>\
         <div style='height:3000px'></div>",
    );
    for start in [0.0, 1000.0] {
        page.scroll_to(Point::new(0.0, start));
        // Also a box partly outside the viewport: a scroll would not move
        // it (Chromium does not scroll).
        for id in ["link", "low"] {
            let node = common::node(&page, id);
            page.scroll_into_view(node);
            assert_eq!(page.scroll_position(), Point::new(0.0, start), "{id}");
        }
        // Fragment navigation to a fixed box (Chromium stays).
        let mut target = page.url().unwrap().clone();
        target.set_fragment(Some("footer"));
        page.navigate(target);
        common::finish_loading(&mut page, Duration::from_secs(20));
        assert_eq!(page.scroll_position(), Point::new(0.0, start), "#footer");
    }
}

#[test]
fn fixed_boxes_do_not_extend_the_page() {
    let (_site, mut page) = page(
        "fixed-extent",
        "<div style='position:fixed; top:5000px; width:10px; height:10px'></div>\
         <div style='position:absolute; top:2000px; width:10px; height:10px'></div>",
    );
    assert_eq!(page.content_size(), Size::new(800.0, 2010.0));
}

#[test]
fn fixed_boxes_are_not_clipped_by_their_ancestors() {
    let (_site, mut page) = page(
        "fixed-clip",
        "<div style='position:relative; overflow:hidden; width:100px; height:100px'>\
           <div id=fixed style='position:fixed; top:200px; left:200px; width:50px; height:50px'></div>\
         </div>",
    );
    assert_eq!(id_at(&mut page, 210.0, 210.0).as_deref(), Some("fixed"));
}

#[test]
fn fixed_boxes_escape_the_clips_of_stacking_contexts() {
    // The clipping ancestor forms a stacking context (z-index, opacity,
    // sticky); Chromium paints the fixed boxes outside its clip.
    for (n, context) in [
        "position:relative; z-index:1",
        "opacity:0.5",
        "position:sticky; top:0",
    ]
    .iter()
    .enumerate()
    {
        let (_site, mut page) = page(
            &format!("fixed-context-{n}"),
            &format!(
                "<div style='{context}; overflow:hidden; width:50px; height:50px'>\
                   <div id=fixed style='position:fixed; top:200px; left:200px; width:50px; \
                                        height:50px; background:red'></div></div>\
                 <div style='height:3000px; width:10px'></div>"
            ),
        );
        // Also scrolled: the content fixed to the viewport in the opacity
        // layer and in the sticky group moves with the scroll offset.
        for scroll in [0.0, 1000.0] {
            page.scroll_to(Point::new(0.0, scroll));
            assert_eq!(
                id_at(&mut page, 210.0, 210.0).as_deref(),
                Some("fixed"),
                "{context} at {scroll}"
            );
            let (r, g, b) = pixel(&mut page, 210, 210);
            assert!(
                r == 255 && g < 200 && b < 200,
                "{context} at {scroll}: {:?}",
                (r, g, b)
            );
        }
    }
}

#[test]
fn clip_rect_clips_fixed_descendants() {
    let (_site, mut page) = page(
        "clip-fixed",
        "<div style='position:absolute; width:100px; height:100px; clip:rect(0, 20px, 20px, 0)'>\
           <div id=fixed style='position:fixed; top:0; left:0; width:100px; height:100px'></div></div>",
    );
    assert_eq!(id_at(&mut page, 10.0, 10.0).as_deref(), Some("fixed"));
    assert_eq!(id_at(&mut page, 50.0, 50.0), None);
}

#[test]
fn nested_sticky_boxes_move_with_their_sticky_container() {
    let (_site, mut page) = page(
        "sticky-nested",
        "<div style='height:2000px'>\
           <div id=outer style='position:sticky; top:0; height:300px'>\
             <div style='height:50px'></div>\
             <div id=inner style='position:sticky; top:0; height:20px'></div>\
           </div></div>",
    );
    page.scroll_to(Point::new(0.0, 500.0));
    let outer = common::node(&page, "outer");
    let inner = common::node(&page, "inner");
    // The outer box sticks at 500; the inner box stays 50 px below its top
    // (Chromium: 550).
    assert_eq!(page.element_box(outer).unwrap().y, 500.0);
    assert_eq!(page.element_box(inner).unwrap().y, 550.0);
}

#[test]
fn clip_rect_does_not_bring_back_outer_overflow_clips_for_fixed_boxes() {
    // Only the clip rectangle applies to the fixed box, not the overflow
    // clip around the clipping box (Chromium paints the fixed box).
    let (_site, mut page) = page(
        "clip-overflow-fixed",
        "<div style='overflow:hidden; width:50px; height:50px'>\
           <div style='position:absolute; width:300px; height:300px; clip:rect(0, 500px, 500px, 0)'>\
             <div id=fixed style='position:fixed; top:100px; left:100px; width:50px; height:50px'></div>\
           </div></div>",
    );
    assert_eq!(id_at(&mut page, 110.0, 110.0).as_deref(), Some("fixed"));
}

#[test]
fn three_nested_sticky_boxes_move_with_all_their_ancestors() {
    let (_site, mut page) = page(
        "sticky-three",
        "<div style='height:3000px'><div style='height:100px'></div>\
           <div id=s1 style='position:sticky; top:0; height:800px'><div style='height:50px'></div>\
             <div id=s2 style='position:sticky; top:0; height:400px'><div style='height:30px'></div>\
               <div id=s3 style='position:sticky; top:0; height:20px'></div>\
             </div></div></div>",
    );
    page.scroll_to(Point::new(0.0, 400.0));
    // Chromium: 400, 450, 480.
    for (id, y) in [("s1", 400.0), ("s2", 450.0), ("s3", 480.0)] {
        let node = common::node(&page, id);
        assert_eq!(page.element_box(node).unwrap().y, y, "#{id}");
    }
}

#[test]
fn sticky_boxes_inside_a_fixed_box_ignore_sticky_ancestors_outside_it() {
    let (_site, mut page) = page(
        "sticky-fixed-sticky",
        "<div style='height:3000px'><div style='height:100px'></div>\
           <div style='position:sticky; top:0; height:500px'>\
             <div style='position:fixed; top:0; left:300px; width:200px; height:400px'>\
               <div style='height:40px'></div>\
               <div id=inner style='position:sticky; top:100px; height:20px'></div>\
             </div></div></div>",
    );
    page.scroll_to(Point::new(0.0, 300.0));
    // Chromium: 100 px below the top of the viewport.
    let inner = common::node(&page, "inner");
    assert_eq!(page.element_box(inner).unwrap().y, 400.0);
}

#[test]
fn many_large_transform_layers_are_bounded() {
    // Each rotated box needs a layer as large as the viewport; the frame
    // budget stops them after a while. The last box is opaque blue.
    let boxes = "<div style='position:absolute; inset:0; transform:rotate(1deg) scale(1.5); \
                 background:rgba(255,0,0,0.01)'></div>"
        .repeat(2000);
    let last = "<div style='position:absolute; inset:0; transform:rotate(1deg) scale(1.5); \
                background:blue'></div>";
    let (_site, mut page) = page(
        "transform-budget",
        &format!("<div style='height:100%'>{boxes}{last}</div>"),
    );
    let started = std::time::Instant::now();
    let (red, green, _) = pixel(&mut page, 400, 300);
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "{:?}",
        started.elapsed()
    );
    // The budget draws only the first layers: some red, and not the blue
    // box (without the budget, the center is blue).
    assert_eq!(red, 255);
    assert!(green < 250, "{green}");
}

#[test]
fn absolute_boxes_are_clipped_by_their_containing_block_chain_only() {
    let (_site, mut page) = page(
        "abs-clip",
        "<div style='position:relative; height:100px'>\
           <div style='overflow:hidden; width:50px; height:50px'>\
             <div id=escapes style='position:absolute; left:100px; top:0; width:50px; height:50px'></div>\
           </div>\
         </div>\
         <div style='position:relative; overflow:hidden; width:50px; height:50px'>\
           <div id=clipped style='position:absolute; left:100px; top:0; width:50px; height:50px'></div>\
         </div>\
         <div style='position:relative; height:100px'>\
           <div style='opacity:0.5; overflow:hidden; width:50px; height:50px'>\
             <div id=through-opacity style='position:absolute; left:100px; top:0; width:50px; height:50px'></div>\
           </div>\
         </div>",
    );
    assert_eq!(id_at(&mut page, 110.0, 10.0).as_deref(), Some("escapes"));
    assert_eq!(id_at(&mut page, 110.0, 110.0), None);
    // The clip of a stacking context between the box and its containing
    // block does not apply either.
    assert_eq!(
        id_at(&mut page, 110.0, 160.0).as_deref(),
        Some("through-opacity")
    );
}

#[test]
fn sticky_boxes_stick_while_their_container_is_visible() {
    let (_site, mut page) = page(
        "sticky",
        "<div id=container style='height:1000px'>\
           <div id=sticky style='position:sticky; top:10px; height:50px; background:red'></div>\
         </div>\
         <div id=after style='height:3000px'></div>",
    );
    page.scroll_to(Point::new(0.0, 500.0));
    assert_eq!(id_at(&mut page, 10.0, 20.0).as_deref(), Some("sticky"));
    assert_eq!(id_at(&mut page, 10.0, 70.0).as_deref(), Some("container"));
    assert_eq!(pixel(&mut page, 10, 20), RED);
    let sticky = common::node(&page, "sticky");
    assert_eq!(page.element_box(sticky).unwrap().y, 510.0);
    // At the end of its container it scrolls away with it.
    page.scroll_to(Point::new(0.0, 1200.0));
    assert_eq!(page.element_box(sticky).unwrap().y, 950.0);
    assert_eq!(id_at(&mut page, 10.0, 20.0).as_deref(), Some("after"));
}

#[test]
fn transformed_boxes_are_hit_where_they_are_painted() {
    let (_site, mut page) = page(
        "transform-hit",
        "<div id=moved style='width:100px; height:20px; transform:translate(200px, 0); background:red'></div>\
         <div id=rotated style='margin-top:100px; width:200px; height:20px; transform:rotate(90deg); background:red'></div>",
    );
    assert_eq!(id_at(&mut page, 210.0, 10.0).as_deref(), Some("moved"));
    assert_eq!(id_at(&mut page, 10.0, 10.0), None);
    assert_eq!(pixel(&mut page, 210, 10), RED);
    assert_eq!(pixel(&mut page, 10, 10), WHITE);
    // The rotated box is 20 px wide and 200 px high around its center
    // (100, 130).
    assert_eq!(id_at(&mut page, 100.0, 40.0).as_deref(), Some("rotated"));
    assert_eq!(id_at(&mut page, 20.0, 130.0), None);
    assert_eq!(pixel(&mut page, 100, 40), RED);
    assert_eq!(pixel(&mut page, 20, 130), WHITE);
}

#[test]
fn rotated_boxes_are_painted_at_the_device_scale() {
    let (_site, mut page) = page(
        "transform-hidpi",
        "<div style='margin-top:100px; width:200px; height:20px; transform:rotate(90deg); background:red'></div>",
    );
    page.set_viewport(Size::new(800.0, 600.0), 2.0);
    // At scale 2 the box covers device x 180..220 and y 20..420.
    assert_eq!(pixel(&mut page, 200, 60), RED);
    assert_eq!(pixel(&mut page, 200, 400), RED);
    assert_eq!(pixel(&mut page, 170, 200), WHITE);
    assert_eq!(pixel(&mut page, 200, 440), WHITE);
}

#[test]
fn scaled_boxes_are_painted_at_their_size() {
    let (_site, mut page) = page(
        "transform-scale",
        "<div style='margin:100px; width:20px; height:20px; transform:scale(3); background:red'></div>",
    );
    // 20 px around the center (110, 110) scaled by 3: 80 to 140.
    assert_eq!(pixel(&mut page, 82, 82), RED);
    assert_eq!(pixel(&mut page, 138, 138), RED);
    assert_eq!(pixel(&mut page, 78, 110), WHITE);
}

#[test]
fn translations_are_exact() {
    let (_site, mut page) = page(
        "transform-exact",
        "<div style='position:relative; height:100px'>\
           <div style='position:absolute; top:50%; left:10px; width:18px; height:18px; \
                       transform:translateY(-50%); background:red'></div></div>",
    );
    // From y = 41 to 59: the edges are sharp.
    assert_eq!(pixel(&mut page, 15, 41), RED);
    assert_eq!(pixel(&mut page, 15, 58), RED);
    assert_eq!(pixel(&mut page, 15, 40), WHITE);
    assert_eq!(pixel(&mut page, 15, 59), WHITE);
}

#[test]
fn z_index_auto_forms_no_stacking_context() {
    let (_site, mut page) = page(
        "z-auto",
        "<div style='position:relative; height:0'>\
           <div id=high style='position:absolute; z-index:10; width:100px; height:100px'></div>\
         </div>\
         <div id=middle style='position:relative; z-index:5; width:100px; height:100px'></div>",
    );
    // The z-index 10 box takes part in the root stacking context.
    assert_eq!(id_at(&mut page, 10.0, 10.0).as_deref(), Some("high"));
}

#[test]
fn positioned_boxes_with_equal_z_index_paint_in_tree_order() {
    let (_site, mut page) = page(
        "tree-order",
        "<div style='position:relative'>\
           <div id=first style='position:absolute; top:0; width:100px; height:100px'></div>\
           <div id=second style='position:relative; width:100px; height:100px'></div>\
         </div>",
    );
    assert_eq!(id_at(&mut page, 10.0, 10.0).as_deref(), Some("second"));
}

#[test]
fn deeply_nested_transforms_are_bounded() {
    // Each level is a layer; levels deeper than the limit are not drawn:
    // the red box at the bottom is missing, the boxes of the first levels
    // are there.
    let open = "<div style='transform:rotate(0.01deg); padding:1px; background:rgb(200,200,255)'>"
        .repeat(300);
    let (_site, mut page) = page(
        "transform-deep",
        &format!("{open}<div style='width:100px; height:100px; background:red'></div>"),
    );
    let started = std::time::Instant::now();
    assert_eq!(pixel(&mut page, 350, 350), (200, 200, 255));
    assert_eq!(pixel(&mut page, 2, 2), (200, 200, 255));
    let _ = id_at(&mut page, 400.0, 10.0);
    assert!(started.elapsed() < Duration::from_secs(20));
}

#[test]
fn huge_and_singular_transforms_do_not_break_painting() {
    let (_site, mut page) = page(
        "transform-hostile",
        "<div id=huge style='width:10px; height:10px; transform:scale(1e30) rotate(10deg); background:red'></div>\
         <div id=flat style='width:100px; height:100px; transform:scale(0)'></div>\
         <div id=wide style='width:10px; height:10px; transform:matrix(1e38, 0, 0, 1e-38, 0, 0) skewX(89.9deg)'></div>\
         <div id=thin style='width:100px; height:100px; transform-origin:0 0; transform:scale(1e30, 1e-6); background:red'></div>\
         <div id=tall style='width:100px; height:100px; transform-origin:0 0; transform:scale(1e-6, 1e30); background:red'></div>\
         <div id=back style='height:100px'></div>",
    );
    // A transform that cannot be inverted is not hit.
    assert_ne!(id_at(&mut page, 50.0, 60.0).as_deref(), Some("flat"));
    let _ = pixel(&mut page, 5, 5);
    for id in ["huge", "flat", "wide", "thin", "tall"] {
        let node = common::node(&page, id);
        let r = page.element_box(node).unwrap();
        assert!([r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite()));
    }
}

#[test]
fn clip_rect_clips_absolute_boxes() {
    let (_site, mut page) = page(
        "clip-rect",
        "<div id=back style='height:200px'>\
           <div id=clipped style='position:absolute; top:0; left:0; width:100px; height:100px; \
                                  clip:rect(0, 20px, 20px, 0); background:red'></div></div>",
    );
    assert_eq!(id_at(&mut page, 10.0, 10.0).as_deref(), Some("clipped"));
    assert_eq!(id_at(&mut page, 50.0, 50.0).as_deref(), Some("back"));
    assert_eq!(pixel(&mut page, 10, 10), RED);
    assert_eq!(pixel(&mut page, 50, 50), WHITE);
}

// ----- Masks with transforms, fixed and sticky boxes -----
//
// The expected colors were measured in screenshots of the same pages with
// Chromium 148; edges are left out (anti-aliasing differs).

/// Asserts that the pixel at (x, y) of `shot` is `expected`, within 2 per
/// channel.
fn assert_color(shot: &swb_engine::Pixmap, x: u32, y: u32, expected: (u8, u8, u8)) {
    let p = shot.pixel(x, y).unwrap().demultiply();
    let actual = (p.red(), p.green(), p.blue());
    let close = |a: u8, b: u8| a.abs_diff(b) <= 2;
    assert!(
        close(actual.0, expected.0) && close(actual.1, expected.1) && close(actual.2, expected.2),
        "({x}, {y}): {actual:?}, expected {expected:?}"
    );
}

#[test]
fn masked_boxes_with_transforms() {
    let (_site, mut page) = page(
        "mask-transform",
        r#"<style>div { box-sizing: border-box } .a { position: absolute; width: 160px; height: 100px }</style>
        <div class=a style="left: 40px; top: 60px; background: blue; transform: rotate(30deg);
          mask-image: linear-gradient(to right, black 50%, transparent 50%)"></div>
        <div class=a style="left: 300px; top: 40px; background: #ddd;
          mask-image: linear-gradient(black 60%, transparent 60%)">
          <div style="margin: 20px 40px; width: 60px; height: 60px; background: green; transform: scale(1.5) rotate(15deg)"></div></div>
        <div class=a style="left: 560px; top: 40px; background: #fc0; transform: rotate(-20deg)">
          <div style="margin: 10px; width: 80px; height: 80px; background: purple;
            mask-image: linear-gradient(to bottom right, black 50%, transparent 50%)"></div></div>
        <div class=a style="left: 40px; top: 260px; background: red; opacity: 0.5;
          transform: skewX(-20deg); mask-image: linear-gradient(to bottom, black 50%, transparent 50%)"></div>
        <div class=a style="left: 300px; top: 260px; width: 100px; background: #aaa;
          mask: linear-gradient(black, black) no-clip">
          <div style="width: 60px; height: 40px; background: teal; transform: translate(80px, 30px)"></div></div>
        <div class=a style="left: 560px; top: 260px; width: 100px; height: 100px; transform: scale(1.5); transform-origin: 0 0">
          <div style="width: 100px; height: 100px; background: navy;
            mask-image: linear-gradient(45deg, black 50%, transparent 50%)"></div></div>
        <div class=a style="left: 40px; top: 440px; width: 200px; height: 120px; background: #eee;
          mask-image: linear-gradient(to right, black 70%, transparent 70%)">
          <div style="position: absolute; left: 100px; top: 20px; width: 80px; height: 80px; background: maroon; transform: rotate(45deg)"></div></div>"#,
    );
    let shot = page.screenshot(false).unwrap();
    let expected = [
        // The mask turns with the box.
        ((85, 90), (0, 0, 255)),
        ((154, 130), WHITE),
        // A transformed child in a mask group.
        ((370, 85), (0, 128, 0)),
        ((370, 125), WHITE),
        ((310, 50), (221, 221, 221)),
        // A mask group in a transformed box.
        ((586, 88), (128, 0, 128)),
        ((644, 115), (255, 204, 0)),
        // Opacity, mask and skew.
        ((129, 285), (255, 126, 126)),
        ((120, 335), WHITE),
        // `no-clip` covers the transformed child.
        ((430, 310), (0, 128, 128)),
        ((350, 300), (170, 170, 170)),
        // The mask in the scaled box's coordinates.
        ((580, 390), (0, 0, 128)),
        ((690, 280), WHITE),
        // A transformed absolute child in a mask group.
        ((165, 500), (128, 0, 0)),
        ((200, 500), WHITE),
        ((60, 460), (238, 238, 238)),
    ];
    for ((x, y), color) in expected {
        assert_color(&shot, x, y, color);
    }
}

#[test]
fn fixed_and_sticky_boxes_in_masked_boxes() {
    let (_site, mut page) = page(
        "mask-fixed",
        r#"<style>body { height: 3000px } div { box-sizing: border-box }
          .m { position: absolute; width: 200px; height: 150px; background: #ccc }
          .f { position: fixed; width: 120px; height: 80px }</style>
        <div class=m style="left: 20px; top: 20px; mask-image: linear-gradient(black 50%, rgba(0,0,0,0.5) 50%)">
          <div class=f style="left: 140px; top: 60px; background: red"></div></div>
        <div class=m style="left: 300px; top: 20px; overflow: hidden; mask-image: linear-gradient(to right, black 75%, transparent 75%)">
          <div class=f style="left: 250px; top: 0px; width: 300px; background: green"></div></div>
        <div class=m style="left: 560px; top: 20px; height: 400px; mask-image: linear-gradient(black 30%, rgba(0,0,0,0.3) 30%)">
          <div style="height: 100px"></div>
          <div style="position: sticky; top: 60px; height: 60px; background: blue"></div></div>
        <div style="position: absolute; left: 20px; top: 250px; width: 150px; height: 100px; overflow: hidden; background: #eee">
          <div class=m style="position: static; mask-image: linear-gradient(to right, black 50%, transparent 50%)">
            <div class=f style="left: 60px; top: 300px; opacity: 0.5; background: purple"></div></div></div>
        <div class=m style="left: 300px; top: 250px; mask-image: linear-gradient(to right, black 50%, rgba(0,0,0,0.5) 50%)">
          <div class=f style="left: 320px; top: 280px; background: orange; transform: rotate(20deg)"></div></div>
        <div class=f style="left: 560px; top: 450px; width: 200px; height: 120px; background: #9cf; overflow: auto;
          mask-image: linear-gradient(to right, black 60%, transparent 60%)">
          <div style="height: 30px"></div>
          <div style="position: sticky; top: 0; height: 30px; background: navy"></div>
          <div style="height: 300px"></div></div>"#,
    );
    let at_top = [
        // The mask hides the fixed box outside the masked box.
        ((180, 80), RED),
        ((180, 120), (255, 127, 127)),
        ((240, 80), WHITE),
        // The fixed box escapes the overflow clip, not the mask.
        ((400, 50), (0, 128, 0)),
        ((480, 50), WHITE),
        ((270, 50), WHITE),
        ((400, 100), (204, 204, 204)),
        // A sticky box in a masked box.
        ((660, 130), (0, 0, 255)),
        ((660, 160), (178, 178, 255)),
        // A fixed box with opacity in a masked box inside an overflow
        // clip: the clip applies to the mask.
        ((90, 330), (166, 102, 166)),
        ((90, 370), WHITE),
        ((150, 330), (238, 238, 238)),
        // A rotated fixed box in a masked box.
        ((380, 320), (255, 165, 0)),
        ((420, 330), (255, 210, 127)),
        // A masked fixed box with a sticky child.
        ((600, 495), (0, 0, 128)),
        ((720, 495), WHITE),
    ];
    let shot = page.screenshot(false).unwrap();
    for ((x, y), color) in at_top {
        assert_color(&shot, x, y, color);
    }
    // Fixed boxes stay, masked boxes move with the page.
    page.scroll_to(Point::new(0.0, 200.0));
    let scrolled = [
        ((180, 80), WHITE),
        ((400, 50), (229, 229, 229)),
        ((660, 90), (178, 178, 255)),
        ((660, 130), (240, 240, 240)),
        ((380, 320), WHITE),
        ((600, 495), (0, 0, 128)),
        ((720, 495), WHITE),
    ];
    let shot = page.screenshot(false).unwrap();
    for ((x, y), color) in scrolled {
        assert_color(&shot, x, y, color);
    }
}

/// A full-page screenshot of a scrolled page: as in Chromium (Playwright
/// `full_page`), it starts at the top of the page, and fixed and sticky
/// boxes are where they are at the current scroll offset (measured at 700
/// px: the header at 700, the sticky box at 740, the footer at 1270).
#[test]
fn full_page_screenshots_of_a_scrolled_page() {
    let (_site, mut page) = page(
        "full-page-scrolled",
        "<div style='position:fixed; top:0; left:0; right:0; height:40px; background:#c00'></div>\
         <div style='position:fixed; bottom:0; left:0; width:300px; height:30px; background:#00c'></div>\
         <div style='height:50vh; background:#eee; margin-top:40px'></div>\
         <div style='position:sticky; top:40px; height:30px; background:#0c0'></div>\
         <div style='height:1400px'></div>",
    );
    page.scroll_to(Point::new(0.0, 700.0));
    let shot = page.screenshot(true).unwrap();
    assert_eq!((shot.width(), shot.height()), (800, 1770));
    assert_color(&shot, 100, 5, WHITE);
    assert_color(&shot, 100, 705, (204, 0, 0));
    assert_color(&shot, 100, 745, (0, 204, 0));
    assert_color(&shot, 100, 1275, (0, 0, 204));
    // The page keeps its scroll offset.
    assert_eq!(page.scroll_position(), Point::new(0.0, 700.0));
}

/// A full-page screenshot of a 400x200 viewport at scale 2 of a page
/// 15,000 px high: two strips of 15,000 rows that meet at y = 7,500 px. As
/// in Chromium, the layout keeps the viewport and fixed and sticky boxes
/// are where they are at scroll offset 0; the boxes that cross the strip
/// boundary are the same on both sides of it.
#[test]
fn full_page_screenshots_with_fixed_and_sticky_boxes_across_strips() {
    let site = Site::new("strips-fixed-sticky");
    let url = site.page(
        "page.html",
        r#"<!DOCTYPE html><style>body { margin: 0; height: 15000px } div { box-sizing: border-box }
          .a { position: absolute } .f { position: fixed }</style>
        <div class=f style="left: 0; top: 7450px; width: 50px; height: 100px; background: red"></div>
        <div class=a style="left: 50px; top: 0; width: 50px; height: 15000px">
          <div style="height: 100px"></div>
          <div style="position: sticky; top: 7450px; height: 100px; background: green"></div></div>
        <div class=a style="left: 110px; top: 7420px; width: 40px; height: 160px; background: blue; transform: rotate(10deg)"></div>
        <div class=a style="left: 160px; top: 7300px; width: 100px; height: 400px; background: #ccc;
          mask-image: linear-gradient(black 60%, rgba(0,0,0,0.5) 60%)">
          <div class=f style="left: 170px; top: 7470px; width: 30px; height: 60px; background: purple"></div>
          <div class=a style="left: 50px; top: 170px; width: 30px; height: 60px; background: teal; transform: rotate(-15deg)"></div>
          <div style="height: 150px"></div>
          <div style="position: sticky; top: 7480px; margin-left: 85px; width: 15px; height: 40px; background: navy"></div></div>
        <div class=a style="left: 270px; top: 7400px; width: 60px; height: 50px; overflow: hidden; background: #eee">
          <div style="opacity: 0.5">
            <div class=f style="left: 270px; top: 7470px; width: 60px; height: 60px; background: orange"></div></div></div>
        <div class=f style="left: 340px; bottom: 0; width: 60px; height: 50px; background: maroon"></div>
        <div class=a style="left: 340px; top: 0; width: 60px; height: 15000px">
          <div style="height: 14000px"></div>
          <div style="position: sticky; bottom: 0; height: 40px; background: olive"></div></div>"#,
    );
    let viewport = Size::new(400.0, 200.0);
    let mut page = common::new_page(Arc::new(NetworkFetcher::new()), 2, viewport);
    page.set_viewport(viewport, 2.0);
    page.navigate(url);
    common::finish_loading(&mut page, Duration::from_secs(30));
    let shot = page.screenshot(true).unwrap();
    assert_eq!((shot.width(), shot.height()), (800, 30_000));
    let boxes = [
        // Fixed and sticky boxes, a rotated box.
        (50, RED),
        (150, (0, 128, 0)),
        (260, (0, 0, 255)),
        // A masked box with a fixed box, a rotated box and a sticky box.
        (330, (204, 204, 204)),
        (370, (128, 0, 128)),
        (450, (0, 128, 128)),
        (505, (0, 0, 128)),
        // A fixed box in an opacity group escapes an overflow clip.
        (600, (255, 210, 127)),
    ];
    for (x, color) in boxes {
        for y in [14_990, 14_999, 15_000, 15_010] {
            assert_color(&shot, x, y, color);
        }
    }
    // Relative to the viewport at scroll offset 0: a fixed box at the
    // bottom, and a sticky box that sticks to the bottom over it.
    assert_color(&shot, 740, 310, (128, 0, 0));
    assert_color(&shot, 740, 380, (128, 128, 0));
    assert_color(&shot, 740, 29_990, WHITE);
}

// ----- Scroll containers -----
//
// The positions follow `tests/layout/scroll-positioned.html` and
// `scroll-sticky.html`, which compare them with Chromium.

#[test]
fn sticky_fixed_and_transformed_boxes_in_scroll_containers() {
    let (_site, mut page) = page(
        "positioned-scrollers",
        "<div id=s style='overflow:auto; width:200px; height:100px; padding:10px; background:#eee'>\
           <div style='height:20px'></div>\
           <div id=k style='position:sticky; top:0; height:20px; background:blue'></div>\
           <div id=f style='position:fixed; left:300px; top:300px; width:50px; height:50px; background:red'></div>\
           <div style='transform:rotate(90deg); width:40px; height:20px; margin-left:100px'>\
             <div id=t style='height:20px; background:lime'></div>\
             <div id=a style='position:absolute; left:0; top:40px; width:10px; height:10px; background:teal'></div></div>\
           <div style='height:300px'></div></div>\
         <div style='height:2000px'></div>",
    );
    let s = common::node(&page, "s");
    page.scroll_element_to(s, Point::new(0.0, 50.0));
    // The sticky box sticks to the scrollport without its padding.
    assert_eq!(common::rect(&mut page, "k").y, 10.0);
    assert_eq!(id_at(&mut page, 50.0, 20.0).as_deref(), Some("k"));
    assert_eq!(pixel(&mut page, 50, 20), (0, 0, 255));
    // The fixed box does not move with the scroll container and is not
    // clipped by it.
    assert_eq!(common::rect(&mut page, "f").y, 300.0);
    assert_eq!(id_at(&mut page, 320.0, 320.0).as_deref(), Some("f"));
    assert_eq!(pixel(&mut page, 320, 320), RED);
    // The transformed box and the absolutely positioned box in it (its
    // containing block) move with the content: the box is at (110, 50) in
    // the content, rotated about the center (130, 60); the absolutely
    // positioned box is 40 px below its top left corner, rotated with it.
    let round = |r: swb_engine::Rect| (r.x.round(), r.y.round(), r.width.round(), r.height.round());
    assert_eq!(
        round(common::rect(&mut page, "t")),
        (120.0, -10.0, 20.0, 40.0)
    );
    assert_eq!(
        round(common::rect(&mut page, "a")),
        (90.0, -10.0, 10.0, 10.0)
    );
    assert_eq!(id_at(&mut page, 130.0, 20.0).as_deref(), Some("t"));
    assert_eq!(pixel(&mut page, 130, 20), (0, 255, 0));
    // A viewport scroll moves the fixed box in document coordinates, and
    // the scroll container with its sticky box on the screen.
    page.scroll_to(Point::new(0.0, 5.0));
    assert_eq!(common::rect(&mut page, "k").y, 10.0);
    assert_eq!(common::rect(&mut page, "f").y, 305.0);
    assert_eq!(pixel(&mut page, 320, 320), RED);
    assert_eq!(pixel(&mut page, 50, 10), (0, 0, 255));
    assert_eq!(id_at(&mut page, 50.0, 10.0).as_deref(), Some("k"));
}

/// A transform on an inline form control applies (it is an atomic
/// inline): the control contains its fixed descendants, which move with
/// the scroll container around it, and the wheel over them scrolls that
/// container (the scroll chain follows layout).
#[test]
fn a_fixed_box_in_an_inline_transformed_control_scrolls_with_its_scroll_container() {
    let (_site, mut page) = page(
        "fixed-in-control",
        "<div id=s style='overflow:auto; height:100px; width:300px'><div style='height:20px'></div>\
         <button style='display:inline; transform:translateX(5px)'>btn<span id=f style='position:fixed;\
         top:0; left:0; width:10px; height:10px; display:block; background:red'></span></button>\
         <div style='height:300px'></div></div>",
    );
    let before = common::rect(&mut page, "f");
    let center = (before.x + 5.0, before.y + 5.0);
    assert_eq!(id_at(&mut page, center.0, center.1).as_deref(), Some("f"));
    assert!(page.wheel(center.0, center.1, 0.0, 40.0));
    let s = common::node(&page, "s");
    let offset = page.element_scroll(s).unwrap().offset;
    assert_eq!(offset, Point::new(0.0, 40.0));
    assert_eq!(common::rect(&mut page, "f").y, before.y - 40.0);
}

/// Scroll into view and the clip of scroll containers use the painted
/// boxes and scrollports: in a stuck sticky scroll container and in a
/// fixed one, a visible link stays, and a hidden one scrolls only its
/// scroll container.
#[test]
fn scroll_into_view_in_sticky_and_fixed_scroll_containers() {
    let (_site, mut page) = page(
        "reveal-painted",
        "<div style='height:1000px'></div>\
         <div id=st style='position:sticky; top:0; overflow:auto; height:100px; width:200px'>\
           <a id=near href='#'>near</a><div style='height:300px'></div><a id=far href='#'>far</a></div>\
         <div style='height:3000px'></div>\
         <div id=fx style='position:fixed; left:300px; top:100px; overflow:auto; height:100px; width:200px'>\
           <a id=fnear href='#'>near</a><div style='height:300px'></div><a id=ffar href='#'>far</a></div>",
    );
    page.scroll_to(Point::new(0.0, 1500.0));
    let scroll = Point::new(0.0, 1500.0);
    let offset = |page: &mut Page, id: &str| {
        let n = common::node(page, id);
        page.element_scroll(n).unwrap().offset
    };
    // The center of a link is inside the clip of its scroll containers.
    let visible = |page: &mut Page, id: &str| {
        let rect = common::rect(page, id);
        let n = common::node(page, id);
        let clip = page.scroll_clip(n).unwrap().unwrap();
        clip.contains(Point::new(
            rect.x + rect.width / 2.0,
            rect.y + rect.height / 2.0,
        ))
    };
    for (near, far, scroller) in [("near", "far", "st"), ("fnear", "ffar", "fx")] {
        assert!(visible(&mut page, near), "{near}");
        let n = common::node(&page, near);
        page.scroll_into_view(n);
        assert_eq!(page.scroll_position(), scroll, "{near}");
        assert_eq!(offset(&mut page, scroller), Point::default(), "{near}");
        assert!(!visible(&mut page, far), "{far}");
        let n = common::node(&page, far);
        page.scroll_into_view(n);
        assert_eq!(page.scroll_position(), scroll, "{far}");
        assert!(offset(&mut page, scroller).y > 0.0, "{far}");
        assert!(visible(&mut page, far), "{far}");
    }
}
