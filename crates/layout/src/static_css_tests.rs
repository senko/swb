//! Layout of M13 feature 2a (videojs.org): the width of an inline SVG with
//! `height: 100%` in the content sizes of a container with a definite
//! height (the logo), the individual transform properties, and the line
//! breaks of `text-wrap`. The boxes are those that Chromium 148 measured
//! in the cases of `tools/probes/css-videojs-2a.json` (`logo`,
//! `transforms-boxes`, `wrap-*`).

use crate::test_support::layout_html;

const HEAD: &str = "<!DOCTYPE html><style>body { margin: 0; font: 16px Arial }</style>";

#[test]
fn an_svg_with_percentage_height_is_as_wide_as_its_ratio_in_a_container_of_definite_height() {
    // Chromium: 225 x 40 in every case; before, the container of the first
    // three had a content width of 0.
    let l = layout_html(&format!(
        "{HEAD}<nav style='display:flex'>\
         <a id=a style='display:flex;height:40px;align-items:flex-end;gap:16px'>\
         <svg id=s viewBox='0 0 225 40' height='100%'></svg></a><b>x</b></nav>\
         <div style='float:left'><div id=d style='display:flex;height:40px'>\
         <svg id=v viewBox='0 0 225 40' style='height:100%'></svg></div></div>\
         <div style='clear:both;display:flex'><div id=e style='height:40px'>\
         <svg id=w viewBox='0 0 225 40' style='height:100%'></svg></div></div>"
    ));
    for id in ["a", "s", "d", "v", "e", "w"] {
        let r = l.rect(id);
        assert_eq!((r.width, r.height), (225.0, 40.0), "#{id}");
    }
}

#[test]
fn a_percentage_height_of_the_container_resolves_against_the_parent_base() {
    // Chromium (`pct-height-in-pct-height`): the item has `height: 50%` of
    // the flex container's 40px, so the SVG with `height: 100%` is 20px
    // high and 112.5px wide.
    let l = layout_html(&format!(
        "{HEAD}<div style='display:flex;height:40px'><div id=d style='height:50%'>\
         <svg id=s viewBox='0 0 225 40' style='height:100%'></svg></div></div>"
    ));
    for id in ["d", "s"] {
        let r = l.rect(id);
        assert_eq!((r.width, r.height), (112.5, 20.0), "#{id}");
    }
}

#[test]
fn individual_transforms_apply_before_transform() {
    // Chromium: the boxes of `transforms-boxes`: `translate: 20px 10px`
    // moves a box, `rotate: 90deg` of a 40 x 20 box at (90, 60) is 20 x 40
    // at (100, 50), `scale: 2 .5` is 80 x 10 at (130, 65), and
    // `rotate: x 90deg` squashes the height to 0.
    let l = layout_html(&format!(
        "{HEAD}<style>.b{{position:absolute;width:40px;height:20px;top:60px}}\
         #a{{left:10px;translate:20px 10px}} #b{{left:90px;rotate:90deg}}\
         #c{{left:150px;scale:2 .5}} #f{{left:10px;top:130px;rotate:x 90deg}}</style>\
         <div class=b id=a></div><div class=b id=b></div><div class=b id=c></div>\
         <div class=b id=f></div>"
    ));
    let size = |id: &str| {
        let r = l.rect(id);
        [r.x, r.y, r.width, r.height].map(|v| (v * 100.0).round() / 100.0)
    };
    assert_eq!(size("a"), [30.0, 70.0, 40.0, 20.0]);
    assert_eq!(size("b"), [100.0, 50.0, 20.0, 40.0]);
    assert_eq!(size("c"), [130.0, 65.0, 80.0, 10.0]);
    assert_eq!(size("f")[3], 0.0);
}

/// The lines of `<p>` in a box of `width` px, as the number of words on
/// each line.
fn lines(style: &str, width: u32, words: &str) -> Vec<usize> {
    let l = layout_html(&format!(
        "{HEAD}<p id=p style='margin:0;width:{width}px;{style}'>{words}</p>"
    ));
    let mut tops: Vec<(f32, usize)> = Vec::new();
    l.tree.walk(|f, _| {
        if let crate::FragmentRef::Text(t) = f {
            let y = t.rect.y.round();
            let n = t.text.split_whitespace().count();
            match tops.last_mut() {
                Some((top, count)) if *top == y => *count += n,
                _ => tops.push((y, n)),
            }
        }
    });
    tops.into_iter().map(|(_, n)| n).collect()
}

const TITLE: &str = "The Open Source Player for the Web";

#[test]
fn balance_matches_chromium_on_the_title() {
    // Chromium (`wrap-title-*`): width 260 gives "The Open Source|Player
    // for the Web" with `balance` and "...for the|Web" with `auto`; width
    // 120 gives three lines; at 280 everything fits on one line.
    assert_eq!(lines("text-wrap:balance", 260, TITLE), [3, 4]);
    assert_eq!(lines("text-wrap:auto", 260, TITLE), [6, 1]);
    assert_eq!(lines("text-wrap:balance", 120, TITLE), [2, 2, 3]);
    assert_eq!(lines("text-wrap:balance", 280, TITLE), [7]);
}

#[test]
fn pretty_moves_a_word_down_from_an_orphan() {
    // Chromium (`wrap-title-pretty`): width 240 gives "...for|the Web".
    assert_eq!(lines("text-wrap:auto", 240, TITLE), [6, 1]);
    assert_eq!(lines("text-wrap:pretty", 240, TITLE), [5, 2]);
    assert_eq!(lines("text-wrap:pretty", 200, TITLE), [5, 2]);
}

#[test]
fn a_nowrap_paragraph_is_not_balanced() {
    assert_eq!(lines("text-wrap:nowrap balance", 100, TITLE), [7]);
}
