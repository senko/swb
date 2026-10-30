//! `getComputedStyle` (M8 feature 7a2, ADR 0028 sections 9 and 10): every
//! case of the Chromium probe `tools/probes/web-computed-style.json` (the
//! values of each property group as Chromium resolves and serializes them,
//! the values that layout resolves, shorthands, pseudo-elements, elements
//! that are not rendered or not in the document, the object and its
//! errors, and style changes seen through a live declaration), loaded with
//! the bundled test fonts in an 800x600 viewport as the probe does; and the
//! flush counters of style-only and layout queries. Scripts run on a thread
//! with a 16 MiB stack, as in swb.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

#[path = "computed_style/cases.rs"]
mod cases;
mod common;

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{Evaluation, JsonForm, Page, Size, Url};
use swb_net::{Fetcher, Headers, NetError, Request, Response};

use common::new_page;

/// A probe case with Chromium's values: (query, value).
pub(crate) struct Case {
    pub(crate) name: &'static str,
    pub(crate) html: &'static str,
    pub(crate) queries: &'static [(&'static str, &'static str)],
}

/// Where swb departs from Chromium on purpose: (case, query index, swb's
/// value, why).
const SWB_VALUES: &[(&str, usize, &str, &str)] = &[
    (
        "values-longhand-colors",
        0,
        r##"["red => rgb(255, 0, 0)","RED => rgb(255, 0, 0)","#f00 => rgb(255, 0, 0)","#FF0000 => rgb(255, 0, 0)","#f008 => rgba(255, 0, 0, 0.533)","#ff000080 => rgba(255, 0, 0, 0.5)","rgb(255,0,0) => rgb(255, 0, 0)","rgb(255 0 0 / 50%) => rgba(255, 0, 0, 0.5)","rgba(0,0,0,0.5) => rgba(0, 0, 0, 0.5)","rgba(0,0,0,0) => rgba(0, 0, 0, 0)","transparent => rgba(0, 0, 0, 0)","currentColor => rgb(0, 0, 255)","CurrentColor => rgb(0, 0, 255)","hsl(120, 100%, 50%) => rgb(0, 255, 0)","hsla(120,100%,50%,0.3) => rgba(0, 255, 0, 0.3)","rgb(10.5, 20, 30) => rgb(11, 20, 30)","rgba(0,0,0,0.123456) => rgba(0, 0, 0, 0.12)","rgba(0,0,0,0.3) => rgba(0, 0, 0, 0.3)","rgba(1,2,3,0.25) => rgba(1, 2, 3, 0.25)","rgba(1,2,3,.1) => rgba(1, 2, 3, 0.1)","rgb(300,-5,0) => rgb(255, 0, 0)","rgb(50%,0%,10%) => rgb(128, 0, 26)","foo => rgb(0, 0, 255)","#ggg => rgb(0, 0, 255)","Canvas => rgb(255, 255, 255)","canvastext => rgb(0, 0, 0)","-webkit-link => rgb(0, 0, 238)","lab(50% 20 30) => rgb(161, 105, 69)","oklch(0.5 0.1 120) => rgb(92, 107, 33)","color(srgb 1 0 0) => rgb(255, 0, 0)","color-mix(in srgb, red, blue) => rgb(128, 0, 128)","hwb(120 0% 0%) => rgb(0, 255, 0)","rgb(0 0 0 / 0.5) => rgba(0, 0, 0, 0.5)","#ABCDEF => rgb(171, 205, 239)","aliceblue => rgb(240, 248, 255)","rgba(255,0,0,1) => rgb(255, 0, 0)","rgb(1,2,3,0.5) => rgba(1, 2, 3, 0.5)","  red   => rgb(255, 0, 0)","red !important => rgb(0, 0, 255)"]"##,
        "swb computes `lab()`, `oklch()`, `color()` and `color-mix()` to 8-bit sRGB",
    ),
    (
        "values-longhand-lengths",
        0,
        r#"["10px => 10px","10PX => 10px","0 => 0px","0.5em => 10px",".5em => 10px","10.0px => 10px","1e2px => 100px","50% => 200px","auto => 400px","calc(10px + 5px) => 15px","calc(100% - 10px) => 390px","calc(10px*2) => 20px","calc( 50% + 2em ) => 240px","calc(100%/3) => 133.328px","-10px => 400px","10vw => 80px","3rem => 48px","1in => 96px","1cm => 37.7812px","12pt => 16px","1mm => 3.76562px","1q => 0.9375px","1pc => 16px","2ex => 20px","2ch => 20px","min-content => 400px","fit-content => 400px","max-content => 400px","10 => 400px","1.23456789px => 1.23438px","0.0001px => 0px","1e-7px => 0px","123456789px => 3.35544e+07px","1234567.5px => 1.23457e+06px","0.1px => 0.09375px","33.333333px => 33.3281px","+5px => 5px","-0px => 0px","0.000001px => 0px","1e21px => 3.35544e+07px","fit-content(10px) => 400px","10px 20px => 400px","stretch => 400px","-webkit-fill-available => 400px","min(10px, 5%) => 10px","max(1px,2px) => 2px","clamp(1px, 5%, 10px) => 10px"]"#,
        "swb's `ex` is half an `em`; swb's layout sizes a block with `width: min-content`, `max-content` or `fit-content` as `auto` (backlog)",
    ),
    (
        "values-longhand-keywords",
        0,
        r#"["block => block","BLOCK => block","inline-block => inline-block","flex => flex","grid => grid","none => none","contents => contents","table-cell => table-cell","list-item => list-item","inline flex => inline-flex","block flow => block","inline-flex => inline-flex","flow-root => flow-root","inline-grid => inline-grid","table => table","-webkit-box => flex","ruby => inline","math => block"]"#,
        "swb's parser: `-webkit-box` is `flex`, no `ruby` and `math` display types",
    ),
    (
        "values-longhand-keywords",
        1,
        r#"["absolute => absolute","sticky => sticky","-webkit-sticky => sticky","fixed => fixed","static => static","relative => relative"]"#,
        "swb parses `-webkit-sticky` as `sticky`",
    ),
    (
        "values-longhand-keywords",
        2,
        r#"["left => left","none => none","inline-start => left"]"#,
        "swb computes `float: inline-start` to `left`",
    ),
    (
        "values-longhand-keywords",
        8,
        r#"["pointer => pointer","default => default","url(a.png), auto => auto","url(a.png) 1 2, pointer => pointer","grab => grab","zoom-in => zoom-in"]"#,
        "swb's `cursor` keeps the keyword only",
    ),
    (
        "values-longhand-keywords",
        20,
        r#"["disc => disc","DECIMAL => decimal","none => none","'-' => none","\"*\" => none","lower-roman => lower-roman","foo => decimal"]"#,
        "swb's `list-style-type` has no strings and no counter style names",
    ),
    (
        "values-longhand-keywords",
        33,
        r#"["center => center","space-between => space-between","flex-start => flex-start","safe center => center","normal => normal","left => left"]"#,
        "swb's alignment has no `safe`",
    ),
    (
        "values-longhand-keywords",
        34,
        r#"["center => center","baseline => baseline","first baseline => baseline","last baseline => baseline","stretch => stretch","normal => normal"]"#,
        "swb computes `last baseline` to `baseline`",
    ),
    (
        "values-longhand-keywords",
        37,
        r#"["legacy center => center","center legacy => center","start => start"]"#,
        "swb computes `legacy center` to `center`",
    ),
    (
        "values-longhand-keywords",
        42,
        r#"["italic => italic","oblique => italic","oblique 10deg => italic","normal => normal"]"#,
        "swb's `font-style` has no oblique angle",
    ),
    (
        "values-longhand-keywords",
        48,
        r#"["text => border-box","padding-box => padding-box"]"#,
        "swb has no `background-clip: text`",
    ),
    (
        "values-longhand-keywords",
        53,
        r#"["alpha => luminance"]"#,
        "swb does not compute `mask-type`",
    ),
    (
        "values-longhand-numbers",
        7,
        r#"["4 => 4","10px => 8","0 => 0"]"#,
        "swb's `tab-size` takes numbers only",
    ),
    (
        "values-longhand-numbers",
        13,
        r#"["auto => auto","16 / 9 => 1.77778 / 1","16/9 => 1.77778 / 1","2 => 2 / 1","auto 1/1 => auto 1 / 1","1 / 1 auto => auto 1 / 1","0.5 => 0.5 / 1"]"#,
        "swb keeps the aspect ratio as one number",
    ),
    (
        "values-longhand-fonts",
        0,
        r#"["Arial => Arial","\"Times New Roman\" => \"Times New Roman\"","sans-serif => sans-serif","'a b' => \"a b\"","Arial  ,  Helvetica => Arial, Helvetica","Helvetica Neue => \"Helvetica Neue\"","\"Arial\" => Arial","SANS-SERIF => sans-serif","'sans-serif' => \"sans-serif\"","system-ui => system-ui","-apple-system => system-ui","BlinkMacSystemFont, 'Segoe UI', Roboto => system-ui, \"Segoe UI\", Roboto","a\\,b => \"a,b\"","\"a\\\"b\" => \"a\\\"b\"","1abc => \"Times New Roman\"","serif, monospace => serif, monospace"]"#,
        "swb parses `-apple-system` and `BlinkMacSystemFont` as `system-ui`",
    ),
    (
        "values-longhand-transforms",
        0,
        r#"["none => none","translate(10px, 20px) => matrix(1, 0, 0, 1, 10, 20)","translate(10px,20px) => matrix(1, 0, 0, 1, 10, 20)","translateX(10px) => matrix(1, 0, 0, 1, 10, 0)","translate(10px) => matrix(1, 0, 0, 1, 10, 0)","rotate(45deg) => matrix(0.707107, 0.707107, -0.707107, 0.707107, 0, 0)","scale(2) => matrix(2, 0, 0, 2, 0, 0)","scale(1.5, 2) => matrix(1.5, 0, 0, 2, 0, 0)","matrix(1,0,0,1,0,0) => matrix(1, 0, 0, 1, 0, 0)","translate3d(1px,2px,3px) => matrix(1, 0, 0, 1, 1, 2)","translate3d(10px, 20px, 0) => matrix(1, 0, 0, 1, 10, 20)","rotate(0.5turn) => matrix(-1, 0, 0, -1, 0, 0)","skewX(10deg) => matrix(1, 0, 0.176327, 1, 0, 0)","translate(0) => matrix(1, 0, 0, 1, 0, 0)","translateX(0) => matrix(1, 0, 0, 1, 0, 0)","translate(10px) rotate(5deg) => matrix(0.996195, 0.0871557, -0.0871557, 0.996195, 10, 0)","TRANSLATE(1PX) => matrix(1, 0, 0, 1, 1, 0)","rotate(0) => matrix(1, 0, 0, 1, 0, 0)","scale3d(1,2,3) => matrix(1, 0, 0, 2, 0, 0)","matrix3d(1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1) => none","perspective(100px) => none","translateY(-50%) => matrix(1, 0, 0, 1, 0, 0)","rotateX(10deg) => none","rotateZ(1rad) => matrix(0.540302, 0.841471, -0.841471, 0.540302, 0, 0)","skew(10deg, 20deg) => matrix(1, 0.36397, 0.176327, 1, 0, 0)","skew(10deg) => matrix(1, 0, 0.176327, 1, 0, 0)","scaleX(-1) => matrix(-1, 0, 0, 1, 0, 0)","translate(calc(10px + 5px)) => matrix(1, 0, 0, 1, 15, 0)","rotate(90grad) => matrix(0.156434, 0.987688, -0.987688, 0.156434, 0, 0)"]"#,
        "swb has no 3D transforms",
    ),
    (
        "values-longhand-transforms",
        2,
        r#"["center => 200px 0px","left top => 0px 0px","top left => 0px 0px","10px 20px => 10px 20px","50% 50% 0 => 200px 0px","50% 50% => 200px 0px","right => 400px 0px","bottom => 200px 0px","0 0 => 0px 0px","center center => 200px 0px","left 10px => 0px 10px","10px 20px 5px => 10px 20px"]"#,
        "swb's `transform-origin` has no z",
    ),
    (
        "values-longhand-images",
        0,
        r#"["url(a.png) => url(\"file:///a.png\")","url(\"a.png\") => url(\"file:///a.png\")","url('a b.png') => url(\"file:///a%20b.png\")","none => none","linear-gradient(red, blue) => linear-gradient(rgb(255, 0, 0), rgb(0, 0, 255))","linear-gradient(to right, red 0%, blue 100%) => linear-gradient(to right, rgb(255, 0, 0) 0%, rgb(0, 0, 255) 100%)","linear-gradient(45deg,red,blue) => linear-gradient(45deg, rgb(255, 0, 0), rgb(0, 0, 255))","radial-gradient(circle, red, blue) => none","url(a.png), none => url(\"file:///a.png\"), none","url(data:image/png;base64,AA==) => url(\"data:image/png;base64,AA==\")","url() => none","image-set(url(a.png) 1x) => url(\"file:///a.png\")","-webkit-linear-gradient(top, red, blue) => linear-gradient(rgb(255, 0, 0), rgb(0, 0, 255))","repeating-linear-gradient(red 0, blue 10px) => repeating-linear-gradient(rgb(255, 0, 0) 0px, rgb(0, 0, 255) 10px)","conic-gradient(red, blue) => none","linear-gradient(rgba(0,0,0,.5), #fff) => linear-gradient(rgba(0, 0, 0, 0.5), rgb(255, 255, 255))"]"#,
        "swb has linear gradients and `url()` only (no radial, conic, `image-set()`, `-webkit-` gradient syntax, empty `url()`)",
    ),
    (
        "values-longhand-images",
        3,
        r#"["\"a\" => \"a\"","none => normal","normal => normal","counter(x) => counter(x)","attr(title) => \"\"","\"a\" \"b\" => \"ab\"","url(a.png) => url(\"file:///a.png\")","counters(x, '.') => counters(x, \".\")","open-quote => open-quote","'' => \"\"","\"\\\"\" => \"\\\"\"","\"a\" attr(x) counter(y, upper-roman) => \"a\" counter(y, upper-roman)","\"x\" / \"alt\" => \"x\""]"#,
        "swb drops the alternative text of `content`",
    ),
    (
        "values-longhand-images",
        6,
        r##"["none => none","inset(10px) => none","circle(50%) => none","url(#a) => url(\"#a\")","polygon(0 0, 10px 0, 0 10px) => none","inset(1px 2px round 3px) => none"]"##,
        "swb's `clip-path` has no basic shapes",
    ),
    (
        "values-longhand-images",
        8,
        r##"["red => rgb(255, 0, 0)","none => none","url(#g) => url(\"#g\")","url(#g) blue => url(\"#g\") rgb(0, 0, 255)","context-fill => rgb(0, 0, 0)","currentcolor => rgb(0, 0, 255)"]"##,
        "swb has no `context-fill`",
    ),
    (
        "values-longhand-grid",
        0,
        r#"["1fr 1fr => 1fr 1fr","repeat(3, 1fr) => repeat(3, 1fr)","100px auto => 100px auto","none => none","[a] 10px [b c] 1fr => [a] 10px [b c] 1fr","minmax(100px, 1fr) 2fr => minmax(100px, 1fr) 2fr","repeat(auto-fill, minmax(100px, 1fr)) => repeat(auto-fill, minmax(100px, 1fr))","subgrid => none","fit-content(100px) => fit-content(100px)","repeat(2, [a] 10px) => repeat(2, [a] 10px)","masonry => none"]"#,
        "swb has no `subgrid`",
    ),
    (
        "values-longhand-custom",
        0,
        r#"["10px => 10px","  a  b   => a b","red => red","{a:b} => {a:b}"," => ","   => ","  10px  /* c */   => 10px","var(--y) => ","initial => ","INHERIT => ","a!important => "]"#,
        "swb collapses white space runs in custom property values (as in `element.style`)",
    ),
    (
        "values-shorthands",
        20,
        r#"["12px Arial => 12px Arial","bold 12px/1.5 Arial, sans-serif => 700 12px / 18px Arial, sans-serif","italic small-caps bold 12px Arial => italic small-caps 700 12px Arial","12px/20px sans-serif => 12px / 20px sans-serif","caption => 13px system-ui","menu => 13px system-ui","small-caption => 13px system-ui","message-box => 13px system-ui","icon => 13px system-ui","status-bar => 13px system-ui","normal 400 16px/normal serif => 16px serif","condensed 12px a => condensed 12px a","italic 12px a => italic 12px a","16px => 20px \"Times New Roman\"","12px \"Times New Roman\" => 12px \"Times New Roman\"","oblique 10deg 12px a => italic 12px a","bold italic 12px a => italic 700 12px a","1.2em/1.2 a => 24px / 28.8px a","x-large a => 24px a","100 12px a => 100 12px a","small-caps 12px a => small-caps 12px a","normal normal 12px a => 12px a"]"#,
        "swb's system fonts are 13px `system-ui`; no oblique angle",
    ),
    (
        "values-shorthands",
        21,
        r#"["small-caps => small-caps","normal => normal","none => normal","all-small-caps => all-small-caps","common-ligatures => normal"]"#,
        "swb's `font-variant` sets `font-variant-caps` only",
    ),
    (
        "values-shorthands",
        23,
        r#"["underline => underline","underline red dotted => underline dotted rgb(255, 0, 0)","none => none","red => rgb(255, 0, 0)","underline overline => underline overline","line-through 2px => line-through","underline solid => underline"]"#,
        "swb has no `text-decoration-thickness`",
    ),
    (
        "values-shorthands",
        44,
        r#"["text => border-box"]"#,
        "swb has no `-webkit-mask-clip: text`",
    ),
    (
        "values-shorthands",
        45,
        r#"["source-over => add"]"#,
        "`-webkit-mask-composite` is an alias of `mask-composite` in swb",
    ),
    (
        "values-shorthands",
        56,
        r#"["condensed => 75%"]"#,
        "swb parses `font-width` as an alias of `font-stretch`",
    ),
    (
        "values-shorthands",
        57,
        r#"["2px => auto"]"#,
        "swb does not compute `text-decoration-thickness` (its initial value)",
    ),
    (
        "values-shorthands",
        58,
        r#"["2px => auto"]"#,
        "swb does not compute `text-underline-offset` (its initial value)",
    ),
    (
        "grid",
        0,
        r#"["grid-template-columns: 1fr 2fr 30px","grid-template-rows: auto 20px","grid-auto-rows: auto","gap: 10px","row-gap: 10px","grid-template: auto 20px / 1fr 2fr 30px","grid: auto 20px / 1fr 2fr 30px / none / row / auto / auto"]"#,
        "the track sizes of grid containers stay as specified (Chromium gives the used sizes; swb's fragments do not keep them)",
    ),
    (
        "grid",
        1,
        r#"["grid-template-columns: repeat(2, 50px)","grid-template-rows: none","grid-auto-rows: 15px"]"#,
        "the track sizes of grid containers stay as specified",
    ),
    (
        "grid",
        3,
        r#"["grid-template-columns: [x] 40px [y] 1fr","grid-template-areas: \"a b\"","grid-area: auto"]"#,
        "the track sizes of grid containers stay as specified",
    ),
    (
        "shorthands",
        2,
        r#"["margin: 0px","padding: 0px","border: ","border-top: 1px solid rgb(255, 0, 0)","border-right: 0px none rgb(0, 0, 0)","border-width: 1px 0px 0px","border-style: solid none none","border-color: rgb(255, 0, 0) rgb(0, 0, 0) rgb(0, 0, 0)","border-radius: 0px","outline: rgb(0, 0, 0) none 3px","overflow: clip","list-style: outside none none","flex-flow: row nowrap","gap: normal","place-items: normal","inset: auto","background: rgba(0, 0, 0, 0) none repeat scroll calc(100% - 5px) 90% / auto padding-box border-box","background-position: calc(100% - 5px) 90%","columns: auto","grid-area: auto","grid-row: auto","grid-column: auto","text-decoration: none","flex: 0 1 auto","border-spacing: 2px 3px","place-content: center","place-self: end","contain-intrinsic-size: 10px 20px","mask: none","font: 16px \"Times New Roman\"","font-variant: normal","border-block: ","border-inline: 0px none rgb(0, 0, 0)","margin-block: 0px","margin-inline: 0px","padding-inline: 0px","inset-block: auto","border-block-start: 1px solid rgb(255, 0, 0)","border-inline-width: 0px","border-image: none","grid: none / none / none / row / auto / auto","grid-template: none","mask-position: 0% 0%","-webkit-mask-position: 0% 0%","overflow-x: clip"]"#,
        "swb's user-agent sheet has no `border-color: gray` for tables (backlog)",
    ),
    (
        "pseudo",
        7,
        r#"["content: normal","color: rgb(1, 2, 3)","width: auto","height: auto","display: inline","padding-left: 0px","left: auto","position: static"]"#,
        "`::marker`: the style only; Chromium gives `display: inline-block` and the size of the marker box",
    ),
    (
        "pseudo",
        8,
        r#"["content: normal","color: rgb(0, 128, 0)","width: auto","height: auto","display: inline","padding-left: 0px","left: auto","position: static"]"#,
        "`::marker`: Chromium gives `display: inline-block`",
    ),
    (
        "pseudo",
        9,
        r#"["content: normal","color: rgb(0, 128, 0)","width: auto","height: auto","display: inline","padding-left: 0px","left: auto","position: static"]"#,
        "swb does not style `::first-line`: the pseudo-element inherits from the element",
    ),
    (
        "pseudo",
        10,
        r#"["content: normal","color: rgb(0, 128, 0)","width: auto","height: auto","display: inline","padding-left: 0px","left: auto","position: static"]"#,
        "swb does not style `::selection`: the pseudo-element inherits from the element",
    ),
    (
        "pseudo",
        11,
        r#"["content: normal","color: rgb(10, 11, 12)","width: auto","height: auto","display: inline","padding-left: 0px","left: auto","position: static"]"#,
        "`::placeholder`: the style only; Chromium gives `display: block` and the size of the placeholder box",
    ),
    (
        "pseudo",
        12,
        r#"["::foo =>   0","before => rgb(255, 0, 0) \"ab\" 216","::before  =>   0"," ::before => rgb(0, 128, 0) normal 216",":after => rgb(0, 0, 255) none 216","::-webkit-scrollbar => rgb(0, 128, 0) normal 216",":hover =>   0","::before::after =>   0","::first-letter => rgb(0, 128, 0) normal 216",":first-line => rgb(0, 128, 0) normal 216","::backdrop => rgb(0, 128, 0) normal 216","::part(x) => rgb(0, 128, 0) normal 216","::slotted(p) => rgb(0, 128, 0) normal 216","::details-content => rgb(0, 128, 0) normal 216"]"#,
        "a declaration lists swb's 207 properties (Chromium 456); `::first-line` is not styled",
    ),
    (
        "quirks",
        1,
        r#"["width: 784px","height: 10px","margin-top: 8px"]"#,
        "swb's layout does not stretch the body to the viewport in quirks mode (backlog)",
    ),
    (
        "extra",
        13,
        r#"["align-content,align-items,align-self",true,"-webkit-border-horizontal-spacing,-webkit-border-vertical-spacing,-webkit-mask-position-x",true,true,true,214,-1]"#,
        "a declaration lists swb's properties (Chromium also `accent-color`, `-webkit-border-image`, `mask-position`, ...)",
    ),
    (
        "fixed-lengths-em",
        1,
        r#"["padding-top: 26.3906px","padding-left: 26.3906px","margin-top: 8.79688px","margin-left: 17.5938px","margin-right: 17.5938px","width: 712px"]"#,
        "swb's layout does not snap percentage margins and paddings to layout units before it computes the width (Chromium: 712.031px)",
    ),
];

/// The URL of the case pages; the probe's `BASE/` stands for its
/// directory.
const PAGE: &str = "file:///case.html";
const BASE: &str = "file:///";

/// Serves one page; other URLs get a 404.
struct OnePage(&'static str);

impl Fetcher for OnePage {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        let (status, body) = if request.url.as_str() == PAGE {
            (200, self.0.as_bytes().to_vec())
        } else {
            (404, b"not found".to_vec())
        };
        let headers: Headers = [("content-type", "text/html")].into_iter().collect();
        Ok(Response {
            url: request.url.clone(),
            status,
            headers,
            body,
            redirected: false,
        })
    }
}

/// A page with JavaScript on that shows `html`, loaded.
fn open(html: &'static str) -> Page {
    let mut page = new_page(Arc::new(OnePage(html)), 2, Size::new(800.0, 600.0));
    page.set_javascript(true);
    page.navigate(Url::parse(PAGE).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(20)));
    page
}

/// The value of an evaluation as the probe prints it.
fn answer(page: &mut Page, expression: &str) -> serde_json::Value {
    match page.evaluate(expression, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => serde_json::from_str(&text).unwrap(),
        Some(Evaluation::Error(text)) => serde_json::Value::String(format!("error: {text}")),
        None => serde_json::Value::String("error: no script runtime".to_owned()),
    }
}

/// Runs a case: loads it and compares each answer with Chromium's.
fn run_case(case: &'static Case) -> Vec<String> {
    let mut page = open(case.html);
    let mut failures = Vec::new();
    for (index, &(query, chromium)) in case.queries.iter().enumerate() {
        let want = SWB_VALUES
            .iter()
            .find(|v| v.0 == case.name && v.1 == index)
            .map_or(chromium, |v| v.2)
            .replace("BASE/", BASE);
        let want: serde_json::Value = if want.starts_with("error: ") {
            serde_json::Value::String(want)
        } else {
            serde_json::from_str(&want).unwrap()
        };
        let got = answer(&mut page, query);
        if got != want {
            failures.push(format!(
                "{} #{index}: {query}\n  expected: {want}\n  actual:   {got}",
                case.name
            ));
        }
    }
    failures
}

/// Runs `f` on a thread with the stack of swb's page thread.
fn on_page_thread<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap()
}

#[test]
fn probe_cases() {
    let runs: Vec<_> = cases::CASES
        .iter()
        .map(|case| {
            std::thread::Builder::new()
                .stack_size(16 << 20)
                .spawn(move || run_case(case))
                .unwrap()
        })
        .collect();
    let failures: Vec<String> = runs
        .into_iter()
        .flat_map(|run| run.join().unwrap())
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A style-only value is computed for the element and its ancestors after
/// a change (no full restyle, no layout); a value that layout resolves
/// forces one style and one layout run, and a second read after no change
/// forces nothing more.
#[test]
fn queries_flush_only_what_they_need() {
    on_page_thread(|| {
        let mut page = open(
            "<!DOCTYPE html><style>.x { color: rgb(1, 2, 3) }</style><div id=d style='width: 50%'>a</div>",
        );
        page.update_layout();
        let before = page.flush_counters();
        let color = answer(&mut page, "d.className = 'x'; getComputedStyle(d).color");
        assert_eq!(color, "rgb(1, 2, 3)");
        let after_color = page.flush_counters();
        assert_eq!(after_color.forced_style_runs, before.forced_style_runs);
        assert_eq!(after_color.forced_layout_runs, before.forced_layout_runs);
        assert!(after_color.element_style_runs > before.element_style_runs);
        let width = answer(
            &mut page,
            "d.style.width = '25%'; const s = getComputedStyle(d); [s.width, s.width, s.marginLeft]",
        );
        assert_eq!(width, serde_json::json!(["196px", "196px", "0px"]));
        let after_width = page.flush_counters();
        assert_eq!(
            after_width.forced_layout_runs,
            before.forced_layout_runs + 1
        );
        assert_eq!(after_width.forced_style_runs, before.forced_style_runs + 1);
    });
}

/// The page's font context goes with the document state for each task and
/// comes back: text measured in a task and after it uses the same fonts.
#[test]
fn layout_in_a_task_uses_the_page_fonts() {
    on_page_thread(|| {
        let mut page = open(
            "<!DOCTYPE html><span id=s style='display: inline-block; font: 20px monospace'>abcd</span>",
        );
        let in_task = answer(&mut page, "parseFloat(getComputedStyle(s).width)");
        page.update_layout();
        let node = common::node(&page, "s");
        let after = page.element_box(node).unwrap();
        let in_task = in_task.as_f64().unwrap();
        assert!(in_task > 40.0, "{in_task}");
        assert!(
            (in_task - f64::from(after.width)).abs() < 0.02,
            "{in_task} {after:?}"
        );
    });
}
