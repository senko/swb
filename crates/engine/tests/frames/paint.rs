//! What frames paint (ADR 0031 section 4, M9 feature 10b): the `pixels` rows
//! of the cases of `tools/probes/web-frames.json` with Chromium's values, the
//! repaint when a frame document changes, the web fonts of frame documents,
//! and hit testing on a frame.

use std::sync::Arc;

use swb_engine::{FrameId, NodeId, Pixmap, Size};
use swb_layout::FragmentRef;

use super::common::{node, on_page_thread, repo_root, value};
use super::{MAIN, Res, Sim, case_html, finish_loading, frame, js_value, new_page, results, url};

/// The largest difference of a channel that counts as equal (the probe tool
/// uses 2 too).
const TOLERANCE: i32 = 2;

/// The cases with `pixels` rows that run here. `paint-past-budget` does not:
/// its document of 900,000 boxes takes minutes in a test build, and swb
/// departs on purpose (the frame paints nothing; Chromium paints it blue).
/// `just probe --with-swb` and the hostile case
/// `frame-past-the-item-budget` run it.
const PIXEL_CASES: &[&str] = &[
    "transparency",
    "javascript-off",
    "paint-content",
    "paint-effects",
    "paint-scroll-fragment",
    "paint-nested",
    "paint-srcdoc-image",
    "paint-fixed",
    "paint-fonts",
    "paint-hidden",
    "paint-backdrop",
    "paint-stacking",
    "paint-color-scheme",
    "paint-fractional",
    "paint-sticky",
    "paint-many",
    "paint-full-page",
];

/// The JSON of the case `name` in the probe file.
fn case_json(name: &str) -> serde_json::Value {
    let probe = super::probe_file();
    probe["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no case {name}"))
        .clone()
}

/// The resources and the files of a case, by URL path.
fn case_resources(case: &serde_json::Value) -> Vec<(String, Res)> {
    let mut all = Vec::new();
    if let Some(resources) = case["resources"].as_object() {
        for (path, resource) in resources {
            let mut res = Res::html(resource["text"].as_str().unwrap());
            if resource["content_type"] == "image/svg+xml" {
                res.content_type = "image/svg+xml";
            }
            all.push((format!("/case/{path}"), res));
        }
    }
    if let Some(files) = case["files"].as_object() {
        for (path, source) in files {
            let bytes = std::fs::read(repo_root().join(source.as_str().unwrap())).unwrap();
            let res = Res {
                body: bytes,
                content_type: "font/ttf",
                headers: Vec::new(),
                delay_ms: 0,
            };
            all.push((format!("/case/{path}"), res));
        }
    }
    all
}

/// A loaded page of the case `name` of the probe file, in the file's
/// viewport and with its JavaScript setting.
fn open_case(name: &str) -> swb_engine::Page {
    let case = case_json(name);
    let mut all: Vec<(String, Res)> = vec![("/case/case.html".into(), Res::html(&case_html(name)))];
    all.extend(case_resources(&case));
    let borrowed: Vec<(&str, Res)> = all.iter().map(|(p, r)| (p.as_str(), r.clone())).collect();
    let sim = Sim::new(&borrowed);
    let mut page = new_page(sim, 4, Size::new(800.0, 600.0));
    page.set_javascript(case["javascript"].as_bool().unwrap_or(true));
    page.navigate(url(&format!("{MAIN}/case/case.html")));
    finish_loading(&mut page, std::time::Duration::from_secs(60));
    page
}

/// swb's color at a device pixel; outside the screenshot white, as in the
/// probe tool.
fn color_at(shot: &Pixmap, x: f64, y: f64) -> [i32; 3] {
    let at = |v: f64, end: u32| u32::try_from(v.round() as i64).ok().filter(|&v| v < end);
    let pixel = at(x, shot.width())
        .zip(at(y, shot.height()))
        .and_then(|(x, y)| shot.pixel(x, y));
    pixel.map_or([255, 255, 255], |p| {
        let c = p.demultiply();
        [c.red().into(), c.green().into(), c.blue().into()]
    })
}

/// The `pixels` rows of the case `name` that differ from Chromium's.
fn differences(name: &str) -> Vec<String> {
    let mut page = open_case(name);
    let shot = page.screenshot(true).unwrap();
    let mut wrong = Vec::new();
    for row in results(name).iter().filter(|r| r["kind"] == "pixels") {
        let area = row["area"].as_array().unwrap();
        let (x0, y0) = (area[0].as_f64().unwrap(), area[1].as_f64().unwrap());
        let points = row["points"].as_array().unwrap();
        let pixels = row["pixels"].as_array().unwrap();
        for (point, want) in points.iter().zip(pixels) {
            let (px, py) = (point[0].as_f64().unwrap(), point[1].as_f64().unwrap());
            let got = color_at(&shot, x0 + px, y0 + py);
            let close =
                (0..3).all(|i| (got[i] - want[i].as_i64().unwrap() as i32).abs() <= TOLERANCE);
            if !close {
                let label = row["label"].as_str().unwrap_or_default();
                wrong.push(format!(
                    "{name} {label} at {point}: {got:?}, Chromium {want}"
                ));
            }
        }
    }
    wrong
}

#[test]
fn frame_pixels_match_chromium() {
    on_page_thread(|| {
        let wrong: Vec<String> = PIXEL_CASES.iter().flat_map(|c| differences(c)).collect();
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    });
}

/// The syntax of `color-scheme`, `element.style` and `getComputedStyle`.
#[test]
fn color_scheme_values_match_chromium() {
    on_page_thread(|| {
        let name = "paint-color-scheme-syntax";
        let mut page = open_case(name);
        let results = results(name);
        let row = results.iter().find(|r| r["kind"] == "js").unwrap();
        let got = value(&mut page, row["label"].as_str().unwrap());
        let got: serde_json::Value = serde_json::from_str(&got).unwrap();
        assert_eq!(got, row["value"]);
    });
}

/// The width of the border box of the first fragment of `node` in `doc`.
fn box_width(doc: &swb_engine::DocumentState, node: NodeId) -> f32 {
    let mut width = None;
    doc.fragments().unwrap().walk(|fragment, _| {
        if let FragmentRef::Box(b) = fragment
            && b.node == Some(node)
            && width.is_none()
        {
            width = Some(b.border_rect.width);
        }
    });
    width.unwrap()
}

/// The width of the first `span` of frame `id`'s document.
fn span_width(page: &swb_engine::Page, id: FrameId) -> f32 {
    let doc = page.frame_document(id).unwrap();
    box_width(doc, doc.query_selector_all("span").unwrap()[0])
}

/// `paint-fonts`: each document has its own faces of the family `F` and `G`,
/// also next to a parent and a sibling with the same family names.
#[test]
fn web_fonts_stay_in_their_documents() {
    on_page_thread(|| {
        let name = "paint-fonts";
        let want: Vec<f64> = js_value(name, 0)
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let mut page = open_case(name);
        page.update_layout();
        let top = |page: &swb_engine::Page, id: &str| {
            box_width(page.frame_document(FrameId::TOP).unwrap(), node(page, id))
        };
        let got = [
            top(&page, "p"),
            top(&page, "g"),
            span_width(&page, frame(&page, "a")),
            span_width(&page, frame(&page, "b")),
            span_width(&page, frame(&page, "c")),
        ];
        for (got, want) in got.iter().zip(&want) {
            assert!((f64::from(*got) - want).abs() < 1.0, "{got:?} {want:?}");
        }
    });
}

/// A page with one frame, which loads `resources` of its own.
fn open_with(top: &str, resources: &[(&str, Res)]) -> (swb_engine::Page, Arc<Sim>) {
    super::open(top, resources, false)
}

/// The bytes of a region of a viewport screenshot.
fn region(shot: &Pixmap, y0: u32, y1: u32) -> Vec<u8> {
    let width = shot.width() as usize * 4;
    shot.data()
        .chunks_exact(width)
        .skip(y0 as usize)
        .take((y1 - y0) as usize)
        .flatten()
        .copied()
        .collect()
}

/// A frame's text paints the glyphs of the frame's own web font while the
/// parent's face of the same family is in place, and the parent's text keeps
/// its face: each region equals the same text alone in a top-level document.
/// (`paint-fonts` paints its text in the background color, so it checks only
/// widths.) With JavaScript off this also checks that the wait for the load
/// does not end at a `srcdoc` navigation, before layout has found the fonts.
#[test]
fn text_in_a_frame_paints_the_glyphs_of_its_own_web_font() {
    on_page_thread(|| {
        let all = case_resources(&case_json("paint-fonts"));
        let fonts: Vec<(&str, Res)> = all.iter().map(|(p, r)| (p.as_str(), r.clone())).collect();
        let text = "font:40px F;line-height:60px;height:60px;overflow:hidden";
        let face =
            |file: &str| format!("<style>@font-face{{font-family:F;src:url({file})}}</style>");
        let body = format!("<style>body{{margin:0}}</style><div style='{text}'>mmmm Wg</div>");
        let inner = format!("{}{body}", face("lm.ttf"));
        let framed = format!(
            "<!doctype html>{}<style>body{{margin:0}} iframe{{position:absolute;left:0;top:0;\
             width:800px;height:60px;border:0}} div{{position:absolute;left:0;top:100px;{text}}}\
             </style><iframe srcdoc=\"{inner}\"></iframe><div>mmmm Wg</div>",
            face("dv.ttf")
        );
        let alone = |file: &str, top: u32| {
            format!(
                "<!doctype html>{}<style>body{{margin:0}} div{{position:absolute;left:0;\
                 top:{top}px;{text}}}</style><div>mmmm Wg</div>",
                face(file)
            )
        };
        for javascript in [false, true] {
            let shot = |html: &str| {
                let (mut page, _) = super::open(html, &fonts, javascript);
                page.screenshot(false).unwrap()
            };
            let framed = shot(&framed);
            let frame_alone = shot(&alone("lm.ttf", 0));
            let parent_alone = shot(&alone("dv.ttf", 100));
            assert!(
                region(&framed, 0, 60) == region(&frame_alone, 0, 60),
                "frame, JavaScript {javascript}"
            );
            assert!(
                region(&framed, 100, 160) == region(&parent_alone, 100, 160),
                "parent, JavaScript {javascript}"
            );
            // The two fonts differ, so the comparison can fail.
            let same_font = region(&frame_alone, 0, 60) == region(&parent_alone, 100, 160);
            assert!(!same_font);
        }
    });
}

/// The color of a viewport screenshot at CSS px.
fn viewport_color(page: &mut swb_engine::Page, x: f64, y: f64) -> [i32; 3] {
    color_at(&page.screenshot(false).unwrap(), x, y)
}

const BLUE: [i32; 3] = [0, 0, 255];
const GREEN: [i32; 3] = [0, 255, 0];
const WHITE: [i32; 3] = [255, 255, 255];

#[test]
fn the_first_paint_shows_the_frame() {
    on_page_thread(|| {
        let html = "<!doctype html><style>body{margin:0} iframe{border:0;display:block;\
                    width:200px;height:100px}</style><iframe srcdoc=\"<style>html{background:\
                    rgb(0,0,255)}</style>\"></iframe>";
        let (mut page, _) = open_with(html, &[]);
        let mut target = Pixmap::new(800, 600).unwrap();
        page.render(&mut target);
        assert_eq!(color_at(&target, 150.0, 50.0), BLUE);
        assert_eq!(color_at(&target, 250.0, 50.0), WHITE);
    });
}

#[test]
fn screenshots_at_other_scales_and_scroll_offsets_show_frames() {
    on_page_thread(|| {
        let html = "<!doctype html><style>body{margin:0;height:3000px} iframe{position:absolute;\
                    left:10px;top:1000px;border:0;width:100px;height:100px}</style><iframe \
                    srcdoc=\"<style>html{background:rgb(0,0,255)}</style>\"></iframe>";
        let (mut page, _) = open_with(html, &[]);
        // Below the viewport, nothing shows.
        assert_eq!(viewport_color(&mut page, 50.0, 50.0), WHITE);
        // The full page shows it.
        let full = page.screenshot(true).unwrap();
        assert_eq!(color_at(&full, 50.0, 1050.0), BLUE);
        // At scale 2, in device pixels.
        page.set_viewport(Size::new(800.0, 600.0), 2.0);
        let full = page.screenshot(true).unwrap();
        assert_eq!(color_at(&full, 100.0, 2100.0), BLUE);
        assert_eq!(color_at(&full, 15.0, 2100.0), WHITE);
        page.set_viewport(Size::new(800.0, 600.0), 1.0);
        // Scrolled to it.
        page.scroll_to(swb_engine::Point::new(0.0, 900.0));
        assert_eq!(viewport_color(&mut page, 50.0, 150.0), BLUE);
        assert_eq!(viewport_color(&mut page, 150.0, 150.0), WHITE);
    });
}

/// The parent's display list stays when a frame's document changes: a build
/// counts as one run of the page's document.
#[test]
fn an_image_that_loads_in_a_frame_repaints_only_the_frame() {
    on_page_thread(|| {
        let html = "<!doctype html><style>body{margin:0} iframe{border:0;display:block;\
                    width:100px;height:100px}</style><iframe id=f src=f.html></iframe>";
        let svg = "<svg xmlns='http://www.w3.org/2000/svg' width='40' height='40'><rect \
                   width='40' height='40' fill='rgb(0,255,0)'/></svg>";
        let mut image = Res::html(svg);
        image.content_type = "image/svg+xml";
        image.delay_ms = 300;
        let frame_html = Res::html("<!doctype html><style>body{margin:0}</style><img src=i.svg>");
        let all = [
            ("/case/case.html", Res::html(html)),
            ("/case/f.html", frame_html),
            ("/case/i.svg", image),
        ];
        let sim = Sim::new(&all);
        let mut page = new_page(sim, 4, Size::new(800.0, 600.0));
        page.navigate(url(&format!("{MAIN}/case/case.html")));
        // Until the frame's document has laid out and asked for the image.
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let id = loop {
            page.process_network();
            page.update_layout();
            let id = page.frame_ids().get(1).copied();
            if let Some(id) = id
                && page
                    .frame_document(id)
                    .is_some_and(|d| d.url().path() == "/case/f.html" && d.is_loading())
            {
                break id;
            }
            assert!(std::time::Instant::now() < end, "the image did not start");
            std::thread::sleep(std::time::Duration::from_millis(2));
        };
        assert_eq!(viewport_color(&mut page, 20.0, 20.0), WHITE);
        let lists = page.flush_counters().display_list_runs;
        let frame_lists = page
            .frame_document(id)
            .unwrap()
            .flush_counters()
            .display_list_runs;
        // The image arrives: the pump reports a change, as the GUI's frame
        // loop needs for a redraw.
        let mut changed = false;
        while page.frame_document(id).unwrap().is_loading() {
            assert!(std::time::Instant::now() < end, "the image did not load");
            changed |= page.pump().changed;
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(changed, "the pump reports the image");
        assert_eq!(viewport_color(&mut page, 20.0, 20.0), GREEN);
        assert_eq!(page.flush_counters().display_list_runs, lists);
        assert_eq!(
            page.frame_document(id)
                .unwrap()
                .flush_counters()
                .display_list_runs,
            frame_lists + 1
        );
    });
}

#[test]
fn a_style_sheet_that_loads_in_a_frame_repaints() {
    on_page_thread(|| {
        let html = "<!doctype html><style>body{margin:0} iframe{border:0;display:block;\
                    width:100px;height:100px}</style><iframe src=f.html></iframe>";
        let mut sheet = Res::html("html{background:rgb(0,255,0)}");
        sheet.content_type = "text/css";
        let (mut page, _) = open_with(
            html,
            &[
                (
                    "/case/f.html",
                    Res::html("<!doctype html><link rel=stylesheet href=s.css>"),
                ),
                ("/case/s.css", sheet),
            ],
        );
        assert_eq!(viewport_color(&mut page, 20.0, 20.0), GREEN);
    });
}

/// A smaller viewport makes the frame smaller, and the frame's media queries
/// see its own viewport.
#[test]
fn a_viewport_change_repaints_the_frame() {
    on_page_thread(|| {
        let html = "<!doctype html><style>body{margin:0} iframe{border:0;display:block;\
                    width:50%;height:100px}</style><iframe srcdoc=\"<style>html{background:\
                    rgb(0,0,255)} @media (max-width:300px){html{background:rgb(0,255,0)}}</style>\
                    \"></iframe>";
        let (mut page, _) = open_with(html, &[]);
        assert_eq!(viewport_color(&mut page, 350.0, 50.0), BLUE);
        page.set_viewport(Size::new(400.0, 600.0), 1.0);
        assert_eq!(viewport_color(&mut page, 150.0, 50.0), GREEN);
        assert_eq!(viewport_color(&mut page, 250.0, 50.0), WHITE);
    });
}

/// Feature 10b does not go into frames for hit tests: the `iframe` element.
#[test]
fn a_hit_on_a_frame_finds_its_element() {
    on_page_thread(|| {
        let html = "<!doctype html><style>body{margin:0} iframe{border:0;display:block;\
                    width:100px;height:100px}</style><iframe id=f srcdoc=\"<style>body{margin:0}\
                    </style><div style='height:100px'>x</div>\"></iframe>";
        let (mut page, _) = open_with(html, &[]);
        page.screenshot(false).unwrap();
        let iframe = node(&page, "f");
        assert_eq!(page.hit_test(50.0, 50.0).map(|h| h.node), Some(iframe));
    });
}

/// A frame that the parent's layout shows later gets its display list then.
#[test]
fn a_hidden_frame_paints_when_it_gets_a_box() {
    on_page_thread(|| {
        let html = "<!doctype html><style>body{margin:0} iframe{border:0;display:none;\
                    width:100px;height:100px}</style><iframe id=f srcdoc=\"<style>html{background:\
                    rgb(0,0,255)}</style>\"></iframe>";
        let mut page = {
            let sim = Sim::new(&[("/case/case.html", Res::html(html))]);
            let mut page = new_page(sim, 4, Size::new(800.0, 600.0));
            page.set_javascript(true);
            page.navigate(url(&format!("{MAIN}/case/case.html")));
            finish_loading(&mut page, std::time::Duration::from_secs(20));
            page
        };
        assert_eq!(viewport_color(&mut page, 50.0, 50.0), WHITE);
        value(
            &mut page,
            "document.querySelector('#f').style.display = 'block'; 1",
        );
        assert_eq!(viewport_color(&mut page, 50.0, 50.0), BLUE);
    });
}
