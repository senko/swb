//! Tests of image source selection: `srcset` parsing, the choice among
//! candidates, `<picture>` sources and `w` descriptors, with expectations
//! measured in Chromium 148.

use std::fmt::Write as _;

use super::*;

fn candidates(srcset: &str) -> Vec<(&str, Descriptor)> {
    parse_srcset(srcset)
        .into_iter()
        .map(|c| (c.url, c.descriptor))
        .collect()
}

#[test]
fn parses_candidates_and_descriptors() {
    use Descriptor::{Density, None, Width};
    let cases: &[(&str, &[(&str, Descriptor)])] = &[
        ("", &[]),
        ("  ,, ", &[]),
        ("a.png", &[("a.png", None)]),
        ("a.png 2x", &[("a.png", Density(2.0))]),
        (
            "a.png 1.5x, b.png 3x",
            &[("a.png", Density(1.5)), ("b.png", Density(3.0))],
        ),
        // The URL ends at white space only.
        ("a.png,b.png 2x", &[("a.png,b.png", Density(2.0))]),
        ("a.png,,, b.png", &[("a.png", None), ("b.png", None)]),
        (
            "a.png 100w, b.png 200w",
            &[("a.png", Width(100)), ("b.png", Width(200))],
        ),
        ("a.png 100w 50h", &[("a.png", Width(100))]),
        (
            "  a.png   2x  ,  b.png  ",
            &[("a.png", Density(2.0)), ("b.png", None)],
        ),
        // A comma inside a URL is part of it; a trailing one ends it.
        ("data:a,b 2x", &[("data:a,b", Density(2.0))]),
        ("a.png, b.png", &[("a.png", None), ("b.png", None)]),
        // Valid floating-point numbers.
        (
            "a 0.5x, b .5x, c 1e1x, d 2E-1x, e 0x",
            &[
                ("a", Density(0.5)),
                ("b", Density(0.5)),
                ("c", Density(10.0)),
                ("d", Density(0.2)),
                ("e", Density(0.0)),
            ],
        ),
        // Parentheses keep white space and commas in a descriptor.
        ("a.png (x y), b.png", &[("b.png", None)]),
        ("a.png 2x (", &[]),
        (
            "é.png 2x, ü.png",
            &[("é.png", Density(2.0)), ("ü.png", None)],
        ),
    ];
    for &(srcset, expected) in cases {
        assert_eq!(candidates(srcset), expected, "{srcset:?}");
    }
}

#[test]
fn drops_invalid_candidates() {
    for srcset in [
        "a.png 2x 3x",
        "a.png 100w 2x",
        "a.png 2x 100w",
        "a.png 100w 200w",
        "a.png 50h",
        "a.png 2x 50h",
        "a.png 100w 50h 60h",
        "a.png 0w",
        "a.png -5w",
        "a.png +200w",
        "a.png 200W",
        "a.png 2X",
        "a.png -1x",
        "a.png 1.x",
        "a.png +1x",
        "a.png infx",
        "a.png 1e39x",
        "a.png 0h 100w",
        "a.png 99999999999w",
        "a.png foo",
        "a.png 2x foo",
    ] {
        assert_eq!(candidates(srcset), [], "{srcset:?}");
    }
    // The next candidate is still parsed.
    assert_eq!(
        candidates("a.png 2x 3x, b.png 2x"),
        [("b.png", Descriptor::Density(2.0))]
    );
}

/// The URL that [`choose`] picks from `srcset` (and `src`) at `dpr`, with
/// a source size of 150px.
fn pick(srcset: &str, src: Option<&str>, dpr: f32) -> Option<(String, f32)> {
    choose(&parse_srcset(srcset), src, || 150.0, dpr).map(|c| (c.url.to_owned(), c.density))
}

fn url_at(srcset: &str, src: Option<&str>, dpr: f32) -> String {
    pick(srcset, src, dpr)
        .map(|(url, _)| url)
        .unwrap_or_default()
}

/// A `srcset`, a `src`, and the expected choice at device pixel ratios.
type ChoiceCase = (
    &'static str,
    Option<&'static str>,
    &'static [(f32, &'static str)],
);

/// Choices measured with Chromium 148 at several device pixel ratios.
#[test]
fn chooses_as_chromium() {
    let cases: &[ChoiceCase] = &[
        (
            "r200 2x",
            Some("r100"),
            &[(1.0, "r100"), (1.05, "r200"), (1.25, "r200"), (3.0, "r200")],
        ),
        (
            "r100 1x, r200 2x, r300 3x",
            None,
            &[(1.0, "r100"), (2.0, "r200"), (2.5, "r300"), (4.0, "r300")],
        ),
        (
            "r150 1.5x, r300 3x",
            None,
            &[(1.0, "r150"), (1.5, "r150"), (2.0, "r300")],
        ),
        (
            "r100 1x, r150 1.4x, r200 2x",
            None,
            &[(1.0, "r100"), (1.05, "r150"), (1.4, "r150"), (1.45, "r200")],
        ),
        (
            "r100 1x, r150 1.1x, r200 2x",
            None,
            &[(1.1, "r150"), (1.2, "r200")],
        ),
        (
            "r100 1x, r150 1.4x, r200 2x, r300 2.5x, r400 4x",
            None,
            &[(1.25, "r150"), (2.5, "r300"), (3.0, "r400"), (4.0, "r400")],
        ),
        (
            "r100 0.5x, r150 0.9x",
            None,
            &[(0.5, "r100"), (0.75, "r150"), (2.0, "r150")],
        ),
        ("r100 2x, r150 3x", None, &[(1.0, "r100"), (2.4, "r150")]),
        // An explicit or implicit 1x candidate wins over `src`.
        ("r100, r200 2x", Some("r60"), &[(1.0, "r100")]),
        ("r100 1x, r200 1x", None, &[(1.0, "r100")]),
        ("", Some("r100"), &[(1.0, "r100"), (2.0, "r100")]),
        ("r100 2x 3x, r200 2x", None, &[(1.0, "r200")]),
    ];
    for &(srcset, src, choices) in cases {
        for &(dpr, expected) in choices {
            assert_eq!(url_at(srcset, src, dpr), expected, "{srcset:?} at {dpr}");
        }
    }
}

#[test]
fn width_descriptors_use_the_source_size() {
    let srcset = "r100 100w, r200 200w, r400 400w";
    // Densities at 150px: 0.67, 1.33, 2.67.
    assert_eq!(
        pick(srcset, None, 1.0),
        Some(("r200".into(), 200.0 / 150.0))
    );
    assert_eq!(
        pick(srcset, None, 2.0),
        Some(("r400".into(), 400.0 / 150.0))
    );
    // `src` (density 1) does not take part when a candidate has a width.
    assert_eq!(url_at("r100 100w, r200 200w", Some("r60"), 1.0), "r200");
    // Mixed descriptors: 100w at 150px is 0.67.
    assert_eq!(url_at("r100 100w, r200 2x", None, 1.0), "r200");
    // A source size of 0 makes the densities infinite: the first wins.
    let zero = choose(&parse_srcset(srcset), None, || 0.0, 1.0).unwrap();
    assert_eq!(zero.url, "r100");
    assert!(zero.density.is_infinite());
    // The source size is not computed without width descriptors.
    let chosen = choose(&parse_srcset("a 2x"), None, || panic!("not needed"), 1.0);
    assert_eq!(chosen.map(|c| c.url), Some("a"));
}

#[test]
fn no_candidate_no_image() {
    assert_eq!(pick("", None, 1.0), None);
    assert_eq!(pick("a.png 2x 3x", None, 1.0), None);
}

#[test]
fn source_types() {
    for t in [
        "",
        "  ",
        "image/png",
        "IMAGE/PNG",
        "image/png; foo=bar",
        "  image/gif  ",
        "image/svg+xml",
    ] {
        assert!(is_supported_type(t), "{t:?}");
    }
    for t in [
        "image/avif",
        "image/jxl",
        "video/webm",
        "image/png foo",
        "text/html",
    ] {
        assert!(!is_supported_type(t), "{t:?}");
    }
}

fn env(dpr: f32) -> MediaEnvironment {
    MediaEnvironment {
        viewport_width: 800.0,
        viewport_height: 600.0,
        device_pixel_ratio: dpr,
        ..MediaEnvironment::default()
    }
}

/// The file name of the chosen image, its density and whether a
/// `<source>` gives the dimensions, for each `<img>` with an `id`.
fn select_in(html: &str, dpr: f32) -> Vec<(String, String, f32, bool)> {
    let doc = swb_dom::parse_html(html);
    let base = Url::parse("https://example.com/dir/page.html").unwrap();
    select_images(&doc, &base, &env(dpr))
        .into_iter()
        .filter_map(|(node, selection)| {
            let id = doc.element(node)?.attr("id")?.to_owned();
            let (file, density) = selection.image.map_or((String::new(), 0.0), |image| {
                let file = image.url.path().rsplit('/').next().unwrap_or("").to_owned();
                (file, image.density)
            });
            Some((id, file, density, selection.dimension_source.is_some()))
        })
        .collect()
}

/// `<picture>` cases measured with Chromium 148 at 800x600.
#[test]
fn picture_sources_follow_chromium() {
    let html = r#"
        <picture><source media="(min-width: 500px)" srcset="r200.png"><img id="a" src="r100.png"></picture>
        <picture><source media="(max-width: 500px)" srcset="r200.png"><img id="b" src="r100.png"></picture>
        <picture><source type="image/avif" srcset="r200.png"><source type="image/webp" srcset="r300.png"><img id="c" src="r100.png"></picture>
        <picture><source type="image/png; foo=bar" srcset="r200.png"><img id="d" src="r100.png"></picture>
        <picture><source type="" srcset="r200.png"><img id="f" src="r100.png"></picture>
        <picture><source type="image/svg+xml" srcset="s.svg 2x"><img id="g" src="r100.png"></picture>
        <picture><source srcset=""><source src="r300.png"><source srcset="r150.png"><img id="h" src="r100.png"></picture>
        <picture><source media="(min-width: 500px)" srcset="r200.png" width="84" height="29"><img id="i" src="r100.png" width="25" height="25"></picture>
        <picture><source media="(min-width: 500px)" srcset="r200.png" width="84"><img id="j" src="r100.png"></picture>
        <picture><source media="(max-width: 500px)" srcset="r200.png" width="84" height="29"><img id="k" src="r100.png"></picture>
        <picture><source srcset="r200.png 100w, r400.png 400w" sizes="50px"><img id="l" src="r100.png"></picture>
        <picture><img id="m" src="r100.png"><source srcset="r200.png"></picture>
        <div><source srcset="r200.png"><img id="n" src="r100.png"></div>
        <picture><source srcset="r200.png 2x 3x"><source srcset="r300.png 3x"><img id="o" src="r100.png"></picture>
        <picture><source media="screen and (min-resolution: 2dppx)" srcset="r400.png"><source srcset="r200.png"><img id="p" src="r100.png" srcset="r60.png"></picture>
        <picture><source media="bogus" srcset="r400.png"><source media="" srcset="r300.png"><img id="q" src="r100.png"></picture>
        <picture><source type="video/webm" srcset="r400.png"><img id="u" src="r100.png"></picture>
        <picture><source srcset="r200.png" height="10"><img id="y" src="r100.png"></picture>
    "#;
    let s = |id: &str, file: &str, density: f32, dims: bool| {
        (id.to_owned(), file.to_owned(), density, dims)
    };
    // Chromium also decodes AVIF; swb skips that source (`c`).
    let at_1 = vec![
        s("a", "r200.png", 1.0, false),
        s("b", "r100.png", 1.0, false),
        s("c", "r300.png", 1.0, false),
        s("d", "r200.png", 1.0, false),
        s("f", "r200.png", 1.0, false),
        s("g", "s.svg", 2.0, false),
        s("h", "r150.png", 1.0, false),
        s("i", "r200.png", 1.0, true),
        s("j", "r200.png", 1.0, true),
        s("k", "r100.png", 1.0, false),
        s("l", "r200.png", 2.0, false),
        s("m", "r100.png", 1.0, false),
        s("n", "r100.png", 1.0, false),
        s("o", "r300.png", 3.0, false),
        s("p", "r200.png", 1.0, false),
        s("q", "r300.png", 1.0, false),
        s("u", "r100.png", 1.0, false),
        s("y", "r200.png", 1.0, true),
    ];
    assert_eq!(select_in(html, 1.0), at_1);
    let at_2 = select_in(html, 2.0);
    assert_eq!(at_2[14], s("p", "r400.png", 1.0, false));
    assert_eq!(at_2[10], s("l", "r200.png", 2.0, false));
}

/// `w` descriptors and `sizes`, measured with Chromium 148 at 800x600:
/// the chosen image and its natural width (image width / density).
#[test]
fn width_descriptors_follow_chromium() {
    let set = r#"srcset="r100.png 100w, r200.png 200w, r400.png 400w""#;
    let html = format!(
        r#"
        <img id="a" {set} sizes="150px">
        <img id="b" {set}>
        <img id="c" {set} sizes="(min-width: 700px) 100px, 300px">
        <img id="d" {set} sizes="(max-width: 700px) 100px, 300px">
        <img id="e" {set} sizes="calc(10vw + 2em)">
        <img id="f" src="r60.png" srcset="r100.png 100w, r200.png 200w" sizes="50px">
        <img id="g" srcset="r100.png 100w 50h, r200.png 200w" sizes="80px">
        <img id="h" srcset="r100.png 100w, r200.png 2x" sizes="80px">
        <img id="i" srcset="r200.png 0w, r100.png 50w" sizes="25px">
        <img id="j" {set} sizes="auto, 90px">
        "#
    );
    let natural = |dpr: f32| -> Vec<(String, String, f32)> {
        select_in(&html, dpr)
            .into_iter()
            .map(|(id, file, density, _)| {
                let width: f32 = file
                    .trim_start_matches('r')
                    .trim_end_matches(".png")
                    .parse()
                    .unwrap();
                (id, file, width / density)
            })
            .collect()
    };
    let n = |id: &str, file: &str, width: f32| (id.to_owned(), file.to_owned(), width);
    assert_eq!(
        natural(1.0),
        vec![
            n("a", "r200.png", 150.0),
            n("b", "r400.png", 800.0),
            n("c", "r100.png", 100.0),
            n("d", "r400.png", 300.0),
            n("e", "r200.png", 112.0),
            n("f", "r100.png", 50.0),
            n("g", "r100.png", 80.0),
            n("h", "r100.png", 80.0),
            n("i", "r100.png", 50.0),
            // `auto` without lazy loading is 100vw.
            n("j", "r400.png", 800.0),
        ]
    );
    let at_2 = natural(2.0);
    assert_eq!(at_2[0], n("a", "r400.png", 150.0));
    assert_eq!(at_2[2], n("c", "r200.png", 100.0));
    assert_eq!(at_2[7], n("h", "r200.png", 100.0));
}

/// `sizes="auto"` on lazy images: no selection without a width, then the
/// width of the box.
#[test]
fn lazy_auto_sizes_wait_for_the_width() {
    let set = r#"srcset="r100.png 100w, r200.png 200w, r400.png 400w""#;
    let html = format!(
        r#"
        <img id="a" {set} sizes="auto, 90px" loading="lazy">
        <img id="b" {set} sizes="AUTO" loading="LAZY">
        <img id="b2" {set} sizes=" auto, 90px" loading="lazy">
        <img id="c" {set} sizes="auto, 90px" loading="eager">
        <img id="d" {set} sizes="auto ,90px" loading="lazy">
        <img id="e" srcset="r100.png 1x, r200.png 2x" sizes="auto" loading="lazy">
        <img id="f" src="r100.png" sizes="auto" loading="lazy">
        <img id="g" {set} sizes="90px, auto" loading="lazy">
        <picture><source srcset="r300.png"><img id="h" {set} sizes="auto" loading="lazy"></picture>
        <picture><source srcset="r300.png" media="(max-width: 1px)"><img id="i" {set} sizes="auto" loading="lazy"></picture>
        "#
    );
    let doc = swb_dom::parse_html(&html);
    let base = Url::parse("https://example.com/dir/page.html").unwrap();
    let env = env(1.0);
    let selected = select_images(&doc, &base, &env);
    let waiting: Vec<_> = selected
        .iter()
        .map(|(node, s)| (doc.element(*node).unwrap().attr("id").unwrap(), s.auto))
        .collect();
    assert_eq!(
        waiting,
        [
            ("a", true),
            ("b", true),
            ("b2", false),
            ("c", false),
            ("d", false),
            ("e", false),
            ("f", false),
            ("g", false),
            ("h", false),
            ("i", true),
        ]
    );
    assert!(selected.iter().all(|(_, s)| s.auto == s.image.is_none()));
    let by_id = |id: &str| doc.element_by_id(id).unwrap();
    let file = |id: &str, width: Option<f32>| {
        let image = select_auto_image(&doc, &base, by_id(id), &env, width).unwrap();
        let name = image.url.path().rsplit('/').next().unwrap().to_owned();
        (name, image.density)
    };
    // The width gives the density of each candidate: the first one with a
    // density of at least 1.
    assert_eq!(
        file("a", Some(150.0)),
        ("r200.png".to_owned(), 200.0 / 150.0)
    );
    assert_eq!(file("a", Some(400.0)), ("r400.png".to_owned(), 1.0));
    assert_eq!(file("a", Some(50.0)), ("r100.png".to_owned(), 2.0));
    assert_eq!(
        file("b", Some(300.0)),
        ("r400.png".to_owned(), 400.0 / 300.0)
    );
    // With white space before `auto` (no user-agent rule, no containment),
    // the width is not used (100vw).
    assert_eq!(file("b2", Some(300.0)), ("r400.png".to_owned(), 0.5));
    // Without a width (no box, not rendered before): 100vw = 800px, so the
    // densest candidate.
    assert_eq!(file("a", None), ("r400.png".to_owned(), 0.5));
    // A width of 0 gives infinite densities: the first candidate.
    assert_eq!(file("a", Some(0.0)).0, "r100.png");
    // The width does not change a list that does not allow auto-sizes.
    assert_eq!(file("g", Some(300.0)).0, "r100.png");
}

#[test]
fn src_without_srcset() {
    let html =
        r#"<img id="a" src=" r100.png "><img id="b" src="  "><img id="c"><img id="d" srcset="">"#;
    let got = select_in(html, 2.0);
    assert_eq!(got[0].1, "r100.png");
    assert_eq!(got[0].2, 1.0);
    assert_eq!(got[1].1, "");
    assert_eq!(got[2].1, "");
    assert_eq!(got[3].1, "");
}

#[test]
fn many_images_in_one_picture() {
    // Every image sees the sources before it; the scan is linear.
    let mut html = String::from("<picture>");
    for i in 0..2000 {
        write!(
            html,
            r#"<img id="i{i}" src="r100.png"><source media="(max-width: 1px)" srcset="no.png">"#
        )
        .unwrap();
    }
    html.push_str(r#"<source srcset="r200.png"><img id="last" src="r100.png"></picture>"#);
    let got = select_in(&html, 1.0);
    assert_eq!(got.len(), 2001);
    assert!(got[..2000].iter().all(|(_, file, _, _)| file == "r100.png"));
    assert_eq!(got[2000].1, "r200.png");
}

#[test]
fn images_after_a_matching_source_share_it() {
    let html = r#"<picture><source srcset="no.png" type="image/avif"><img id="a" src="r100.png">
        <source srcset="r200.png" width="5"><img id="b" src="r100.png"><img id="c" src="r100.png">
        <source srcset="r300.png"><img id="d" src="r100.png"></picture>"#;
    let files: Vec<(String, bool)> = select_in(html, 1.0)
        .into_iter()
        .map(|(_, file, _, dims)| (file, dims))
        .collect();
    let f = |file: &str, dims: bool| (file.to_owned(), dims);
    assert_eq!(
        files,
        [
            f("r100.png", false),
            f("r200.png", true),
            f("r200.png", true),
            f("r200.png", true),
        ]
    );
}

#[test]
fn negative_zero_density_is_zero() {
    let parsed = candidates("a.png -0x");
    assert_eq!(parsed, [("a.png", Descriptor::Density(0.0))]);
    let Descriptor::Density(d) = parsed[0].1 else {
        unreachable!()
    };
    assert!(d.is_sign_positive());
}
