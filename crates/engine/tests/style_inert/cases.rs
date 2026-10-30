// The cases of `tools/probes/web-style-inert.json` with the values that Chromium 148
// gave (`just probe --json tools/probes/web-style-inert.json`, three runs, all equal);
// where swb departs on purpose, the test overrides the value. Generated.

use crate::Case;

/// Page 1 of the cases.
pub(crate) const PAGE_1: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 0 } #c { width: 400px; height: 300px; font-size: 20px; position: relative; color: rgb(0, 0, 255) } #p { will-change: transform; touch-action: pan-y pan-x; perspective: 10em; perspective-origin: left 10px; } #k { perspective: inherit; will-change: inherit; touch-action: inherit; perspective-origin: inherit } .w { will-change: scroll-position, opacity; touch-action: none; perspective: 5px; perspective-origin: right bottom } .w2 { will-change: initial; touch-action: unset; perspective: initial; perspective-origin: unset }</style></head><body><div id=c><div id=p><div id=k></div></div><div id=w class=w><div id=w2 class=w2></div></div><div id=d style='height: 100px'></div></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return e.constructor.name + ': ' + e.message; } }
function P(p, vs) { return vs.map(v => { const e = document.createElement('div'); e.style.setProperty(p, v); return v + ' => [' + e.style.getPropertyValue(p) + '] {' + e.getAttribute('style') + '}'; }); }
function C(p, vs) { const c = document.getElementById('c'); return vs.map(v => { const e = document.createElement('div'); e.style.setProperty(p, v, 'important'); e.style.height = '100px'; c.appendChild(e); const r = getComputedStyle(e).getPropertyValue(p); e.remove(); return v + ' => ' + r; }); }
function G(sel, ps) { const s = getComputedStyle(document.querySelector(sel)); return ps.map(p => p + ': ' + s.getPropertyValue(p)); }
</script></body></html>";

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "declared-transition",
        html: PAGE_1,
        queries: &[(
            r#"E(() => P("transition", ["none", "all", "opacity 1s", "opacity 1s ease", "opacity 1s ease 2s", "opacity 1s ease 2s allow-discrete", "opacity 1s, width 2s", "all 0.3s", "all 300ms", "1s", "1s 2s", "ease", "width", "WIDTH 1s", "Width", "foo 1s", "none 1s", "none, width", "all, none", "opacity 1s linear, width 2s steps(4)", "initial", "inherit", "unset", "revert", "opacity .5s", "opacity 0s", "opacity 0.0s", "0s", "1s all", "ease-in-out 1s", "width 1s cubic-bezier(0.1,0.7,1,0.1)", "width 1s steps(4, jump-start)", "width 1s steps(4, start)", "width 1s steps(4, end)", "width 1s steps(4)", "width 1s step-start", "width 1s step-end", "opacity -1s", "opacity 1s -1s", "opacity 1s 1s 1s", "opacity, width", "opacity,width", "all 1s var(--x)", "normal", "allow-discrete", "opacity allow-discrete", "transform 1s, , width", "--x 1s", "-webkit-transform 1s", "Transform 1S", "inherit 1s", "opacity 0s 2s", "opacity 300MS 1S", "ease ease", "a b", "opacity 1s,"]))"#,
            r#"["none => [none] {transition: none;}", "all => [all] {transition: all;}", "opacity 1s => [opacity 1s] {transition: opacity 1s;}", "opacity 1s ease => [opacity 1s] {transition: opacity 1s;}", "opacity 1s ease 2s => [opacity 1s 2s] {transition: opacity 1s 2s;}", "opacity 1s ease 2s allow-discrete => [opacity 1s 2s allow-discrete] {transition: opacity 1s 2s allow-discrete;}", "opacity 1s, width 2s => [opacity 1s, width 2s] {transition: opacity 1s, width 2s;}", "all 0.3s => [0.3s] {transition: 0.3s;}", "all 300ms => [300ms] {transition: 300ms;}", "1s => [1s] {transition: 1s;}", "1s 2s => [1s 2s] {transition: 1s 2s;}", "ease => [all] {transition: all;}", "width => [width] {transition: width;}", "WIDTH 1s => [width 1s] {transition: width 1s;}", "Width => [width] {transition: width;}", "foo 1s => [foo 1s] {transition: foo 1s;}", "none 1s => [none 1s] {transition: none 1s;}", "none, width => [] {null}", "all, none => [] {null}", "opacity 1s linear, width 2s steps(4) => [opacity 1s linear, width 2s steps(4)] {transition: opacity 1s linear, width 2s steps(4);}", "initial => [initial] {transition: initial;}", "inherit => [inherit] {transition: inherit;}", "unset => [unset] {transition: unset;}", "revert => [revert] {transition: revert;}", "opacity .5s => [opacity 0.5s] {transition: opacity 0.5s;}", "opacity 0s => [opacity] {transition: opacity;}", "opacity 0.0s => [opacity] {transition: opacity;}", "0s => [all] {transition: all;}", "1s all => [1s] {transition: 1s;}", "ease-in-out 1s => [1s ease-in-out] {transition: 1s ease-in-out;}", "width 1s cubic-bezier(0.1,0.7,1,0.1) => [width 1s cubic-bezier(0.1, 0.7, 1, 0.1)] {transition: width 1s cubic-bezier(0.1, 0.7, 1, 0.1);}", "width 1s steps(4, jump-start) => [width 1s steps(4, jump-start)] {transition: width 1s steps(4, jump-start);}", "width 1s steps(4, start) => [width 1s steps(4, start)] {transition: width 1s steps(4, start);}", "width 1s steps(4, end) => [width 1s steps(4)] {transition: width 1s steps(4);}", "width 1s steps(4) => [width 1s steps(4)] {transition: width 1s steps(4);}", "width 1s step-start => [width 1s step-start] {transition: width 1s step-start;}", "width 1s step-end => [width 1s step-end] {transition: width 1s step-end;}", "opacity -1s => [opacity -1s] {transition: opacity -1s;}", "opacity 1s -1s => [opacity 1s -1s] {transition: opacity 1s -1s;}", "opacity 1s 1s 1s => [] {null}", "opacity, width => [opacity, width] {transition: opacity, width;}", "opacity,width => [opacity, width] {transition: opacity, width;}", "all 1s var(--x) => [all 1s var(--x)] {transition: all 1s var(--x);}", "normal => [all] {transition: all;}", "allow-discrete => [allow-discrete] {transition: allow-discrete;}", "opacity allow-discrete => [opacity allow-discrete] {transition: opacity allow-discrete;}", "transform 1s, , width => [] {null}", "--x 1s => [--x 1s] {transition: --x 1s;}", "-webkit-transform 1s => [-webkit-transform 1s] {transition: -webkit-transform 1s;}", "Transform 1S => [transform 1s] {transition: transform 1s;}", "inherit 1s => [] {null}", "opacity 0s 2s => [opacity 2s] {transition: opacity 2s;}", "opacity 300MS 1S => [opacity 300ms 1s] {transition: opacity 300ms 1s;}", "ease ease => [ease] {transition: ease;}", "a b => [] {null}", "opacity 1s, => [] {null}"]"#,
        )],
    },
    Case {
        name: "declared-transition-longhands",
        html: PAGE_1,
        queries: &[
            (
                r#"E(() => P("transition-property", ["none", "all", "opacity", "opacity, width", "OPACITY", "Width, HEIGHT", "foo", "all, width", "none, width", "width, all", "width, none", "-webkit-transform", "transform, -webkit-transform", "--x", "inherit", "initial", "unset", "revert", "1s", "default", "auto", "ease", "opacity width", "color,background-color", "margin", "scale", "rotate", "translate", "Margin-Left", "border-radius", "font-size", "z-index", "box-shadow", "BOX-SHADOW"]))"#,
                r#"["none => [none] {transition-property: none;}", "all => [all] {transition-property: all;}", "opacity => [opacity] {transition-property: opacity;}", "opacity, width => [opacity, width] {transition-property: opacity, width;}", "OPACITY => [opacity] {transition-property: opacity;}", "Width, HEIGHT => [width, height] {transition-property: width, height;}", "foo => [foo] {transition-property: foo;}", "all, width => [all, width] {transition-property: all, width;}", "none, width => [] {null}", "width, all => [width, all] {transition-property: width, all;}", "width, none => [] {null}", "-webkit-transform => [-webkit-transform] {transition-property: -webkit-transform;}", "transform, -webkit-transform => [transform, -webkit-transform] {transition-property: transform, -webkit-transform;}", "--x => [--x] {transition-property: --x;}", "inherit => [inherit] {transition-property: inherit;}", "initial => [initial] {transition-property: initial;}", "unset => [unset] {transition-property: unset;}", "revert => [revert] {transition-property: revert;}", "1s => [] {null}", "default => [] {null}", "auto => [auto] {transition-property: auto;}", "ease => [ease] {transition-property: ease;}", "opacity width => [] {null}", "color,background-color => [color, background-color] {transition-property: color, background-color;}", "margin => [margin] {transition-property: margin;}", "scale => [scale] {transition-property: scale;}", "rotate => [rotate] {transition-property: rotate;}", "translate => [translate] {transition-property: translate;}", "Margin-Left => [margin-left] {transition-property: margin-left;}", "border-radius => [border-radius] {transition-property: border-radius;}", "font-size => [font-size] {transition-property: font-size;}", "z-index => [z-index] {transition-property: z-index;}", "box-shadow => [box-shadow] {transition-property: box-shadow;}", "BOX-SHADOW => [box-shadow] {transition-property: box-shadow;}"]"#,
            ),
            (
                r#"E(() => P("transition-duration", ["0s", "0ms", "1s", "300ms", "0.3s", ".3s", "0", "-1s", "1", "1s, 2s", "1S", "300MS", "1e1s", "0.0s", "+1s", "1s 2s", "auto", "initial", "inherit", "1m", "1min", "1.50s", "100.5ms", "0.1234567s", "12345678s", "1e10s"]))"#,
                r#"["0s => [0s] {transition-duration: 0s;}", "0ms => [0ms] {transition-duration: 0ms;}", "1s => [1s] {transition-duration: 1s;}", "300ms => [300ms] {transition-duration: 300ms;}", "0.3s => [0.3s] {transition-duration: 0.3s;}", ".3s => [0.3s] {transition-duration: 0.3s;}", "0 => [] {null}", "-1s => [] {null}", "1 => [] {null}", "1s, 2s => [1s, 2s] {transition-duration: 1s, 2s;}", "1S => [1s] {transition-duration: 1s;}", "300MS => [300ms] {transition-duration: 300ms;}", "1e1s => [10s] {transition-duration: 10s;}", "0.0s => [0s] {transition-duration: 0s;}", "+1s => [1s] {transition-duration: 1s;}", "1s 2s => [] {null}", "auto => [] {null}", "initial => [initial] {transition-duration: initial;}", "inherit => [inherit] {transition-duration: inherit;}", "1m => [] {null}", "1min => [] {null}", "1.50s => [1.5s] {transition-duration: 1.5s;}", "100.5ms => [100.5ms] {transition-duration: 100.5ms;}", "0.1234567s => [0.123457s] {transition-duration: 0.123457s;}", "12345678s => [1.23457e+07s] {transition-duration: 1.23457e+07s;}", "1e10s => [1e+10s] {transition-duration: 1e+10s;}"]"#,
            ),
            (
                r#"E(() => P("transition-delay", ["0s", "-1s", "1s", "-300ms", "1s, 2s", "0", "1", "initial", "-0.5S"]))"#,
                r#"["0s => [0s] {transition-delay: 0s;}", "-1s => [-1s] {transition-delay: -1s;}", "1s => [1s] {transition-delay: 1s;}", "-300ms => [-300ms] {transition-delay: -300ms;}", "1s, 2s => [1s, 2s] {transition-delay: 1s, 2s;}", "0 => [] {null}", "1 => [] {null}", "initial => [initial] {transition-delay: initial;}", "-0.5S => [-0.5s] {transition-delay: -0.5s;}"]"#,
            ),
            (
                r#"E(() => P("transition-timing-function", ["ease", "EASE", "linear", "ease-in", "ease-out", "ease-in-out", "step-start", "step-end", "steps(4)", "steps(4, end)", "steps(4, start)", "steps(4, jump-start)", "steps(4, jump-end)", "steps(4, jump-none)", "steps(4, jump-both)", "steps(1, jump-none)", "steps(0)", "steps(0, jump-none)", "steps(0, start)", "steps(-1)", "steps(1.5)", "steps(4,end)", "steps( 4 , end )", "cubic-bezier(0.1, 0.7, 1, 0.1)", "cubic-bezier(0.1,0.7,1,0.1)", "cubic-bezier(0,0,1,1)", "cubic-bezier(.25,.1,.25,1)", "cubic-bezier(1.5, -2, 3, 4)", "cubic-bezier(2, 0.5, 0.5, 1)", "cubic-bezier(0, 0.5, -1, 1)", "cubic-bezier(0,0,1)", "cubic-bezier(0,0,1,1,1)", "cubic-bezier(a,0,1,1)", "ease, linear", "ease,linear", "initial", "inherit", "foo", "cubic-bezier(0.1, 0.7, 1.0, 0.1)", "cubic-bezier(0.10, 0.70, 1, 0.1)", "cubic-bezier(1e-1, 0.7, 1, 0.1)", "cubic-bezier(1, 1, 1, 1)", "cubic-bezier(0.333333333, 0, 0.666666666, 1)", "steps(4, END)", "EASE-IN", "step-start, step-end"]))"#,
                r#"["ease => [ease] {transition-timing-function: ease;}", "EASE => [ease] {transition-timing-function: ease;}", "linear => [linear] {transition-timing-function: linear;}", "ease-in => [ease-in] {transition-timing-function: ease-in;}", "ease-out => [ease-out] {transition-timing-function: ease-out;}", "ease-in-out => [ease-in-out] {transition-timing-function: ease-in-out;}", "step-start => [step-start] {transition-timing-function: step-start;}", "step-end => [step-end] {transition-timing-function: step-end;}", "steps(4) => [steps(4)] {transition-timing-function: steps(4);}", "steps(4, end) => [steps(4)] {transition-timing-function: steps(4);}", "steps(4, start) => [steps(4, start)] {transition-timing-function: steps(4, start);}", "steps(4, jump-start) => [steps(4, jump-start)] {transition-timing-function: steps(4, jump-start);}", "steps(4, jump-end) => [steps(4)] {transition-timing-function: steps(4);}", "steps(4, jump-none) => [steps(4, jump-none)] {transition-timing-function: steps(4, jump-none);}", "steps(4, jump-both) => [steps(4, jump-both)] {transition-timing-function: steps(4, jump-both);}", "steps(1, jump-none) => [] {null}", "steps(0) => [] {null}", "steps(0, jump-none) => [] {null}", "steps(0, start) => [] {null}", "steps(-1) => [] {null}", "steps(1.5) => [] {null}", "steps(4,end) => [steps(4)] {transition-timing-function: steps(4);}", "steps( 4 , end ) => [steps(4)] {transition-timing-function: steps(4);}", "cubic-bezier(0.1, 0.7, 1, 0.1) => [cubic-bezier(0.1, 0.7, 1, 0.1)] {transition-timing-function: cubic-bezier(0.1, 0.7, 1, 0.1);}", "cubic-bezier(0.1,0.7,1,0.1) => [cubic-bezier(0.1, 0.7, 1, 0.1)] {transition-timing-function: cubic-bezier(0.1, 0.7, 1, 0.1);}", "cubic-bezier(0,0,1,1) => [cubic-bezier(0, 0, 1, 1)] {transition-timing-function: cubic-bezier(0, 0, 1, 1);}", "cubic-bezier(.25,.1,.25,1) => [cubic-bezier(0.25, 0.1, 0.25, 1)] {transition-timing-function: cubic-bezier(0.25, 0.1, 0.25, 1);}", "cubic-bezier(1.5, -2, 3, 4) => [] {null}", "cubic-bezier(2, 0.5, 0.5, 1) => [] {null}", "cubic-bezier(0, 0.5, -1, 1) => [] {null}", "cubic-bezier(0,0,1) => [] {null}", "cubic-bezier(0,0,1,1,1) => [] {null}", "cubic-bezier(a,0,1,1) => [] {null}", "ease, linear => [ease, linear] {transition-timing-function: ease, linear;}", "ease,linear => [ease, linear] {transition-timing-function: ease, linear;}", "initial => [initial] {transition-timing-function: initial;}", "inherit => [inherit] {transition-timing-function: inherit;}", "foo => [] {null}", "cubic-bezier(0.1, 0.7, 1.0, 0.1) => [cubic-bezier(0.1, 0.7, 1, 0.1)] {transition-timing-function: cubic-bezier(0.1, 0.7, 1, 0.1);}", "cubic-bezier(0.10, 0.70, 1, 0.1) => [cubic-bezier(0.1, 0.7, 1, 0.1)] {transition-timing-function: cubic-bezier(0.1, 0.7, 1, 0.1);}", "cubic-bezier(1e-1, 0.7, 1, 0.1) => [cubic-bezier(0.1, 0.7, 1, 0.1)] {transition-timing-function: cubic-bezier(0.1, 0.7, 1, 0.1);}", "cubic-bezier(1, 1, 1, 1) => [cubic-bezier(1, 1, 1, 1)] {transition-timing-function: cubic-bezier(1, 1, 1, 1);}", "cubic-bezier(0.333333333, 0, 0.666666666, 1) => [cubic-bezier(0.333333, 0, 0.666667, 1)] {transition-timing-function: cubic-bezier(0.333333, 0, 0.666667, 1);}", "steps(4, END) => [steps(4)] {transition-timing-function: steps(4);}", "EASE-IN => [ease-in] {transition-timing-function: ease-in;}", "step-start, step-end => [step-start, step-end] {transition-timing-function: step-start, step-end;}"]"#,
            ),
            (
                r#"E(() => P("transition-behavior", ["normal", "allow-discrete", "normal, allow-discrete", "ALLOW-DISCRETE", "foo", "initial", "inherit", "normal normal", "allow-discrete, allow-discrete"]))"#,
                r#"["normal => [normal] {transition-behavior: normal;}", "allow-discrete => [allow-discrete] {transition-behavior: allow-discrete;}", "normal, allow-discrete => [normal, allow-discrete] {transition-behavior: normal, allow-discrete;}", "ALLOW-DISCRETE => [allow-discrete] {transition-behavior: allow-discrete;}", "foo => [] {null}", "initial => [initial] {transition-behavior: initial;}", "inherit => [inherit] {transition-behavior: inherit;}", "normal normal => [] {null}", "allow-discrete, allow-discrete => [allow-discrete, allow-discrete] {transition-behavior: allow-discrete, allow-discrete;}"]"#,
            ),
        ],
    },
    Case {
        name: "declared-webkit",
        html: PAGE_1,
        queries: &[
            (
                r#"E(() => P("-webkit-transition", ["opacity 1s", "all 1s ease 2s", "none", "opacity 1s ease 2s allow-discrete"]))"#,
                r#"["opacity 1s => [opacity 1s] {transition: opacity 1s;}", "all 1s ease 2s => [1s 2s] {transition: 1s 2s;}", "none => [none] {transition: none;}", "opacity 1s ease 2s allow-discrete => [opacity 1s 2s allow-discrete] {transition: opacity 1s 2s allow-discrete;}"]"#,
            ),
            (
                r#"E(() => P("-webkit-transition-property", ["opacity", "all", "none", "Width"]))"#,
                r#"["opacity => [opacity] {transition-property: opacity;}", "all => [all] {transition-property: all;}", "none => [none] {transition-property: none;}", "Width => [width] {transition-property: width;}"]"#,
            ),
            (
                r#"E(() => P("-webkit-transition-duration", ["1s", "300ms"]))"#,
                r#"["1s => [1s] {transition-duration: 1s;}", "300ms => [300ms] {transition-duration: 300ms;}"]"#,
            ),
            (
                r#"E(() => P("-webkit-transition-delay", ["1s", "300ms"]))"#,
                r#"["1s => [1s] {transition-delay: 1s;}", "300ms => [300ms] {transition-delay: 300ms;}"]"#,
            ),
            (
                r#"E(() => P("-webkit-transition-timing-function", ["ease", "steps(4, end)"]))"#,
                r#"["ease => [ease] {transition-timing-function: ease;}", "steps(4, end) => [steps(4)] {transition-timing-function: steps(4);}"]"#,
            ),
            (
                r#"E(() => P("-webkit-transition-behavior", ["allow-discrete"]))"#,
                r#"["allow-discrete => [] {null}"]"#,
            ),
            (
                r#"E(() => P("-webkit-perspective", ["100px", "none", "10em"]))"#,
                r#"["100px => [100px] {perspective: 100px;}", "none => [none] {perspective: none;}", "10em => [10em] {perspective: 10em;}"]"#,
            ),
            (
                r#"E(() => P("-webkit-perspective-origin", ["10px 20px", "left top"]))"#,
                r#"["10px 20px => [10px 20px] {perspective-origin: 10px 20px;}", "left top => [left top] {perspective-origin: left top;}"]"#,
            ),
        ],
    },
    Case {
        name: "declared-others",
        html: PAGE_1,
        queries: &[
            (
                r#"E(() => P("will-change", ["auto", "transform", "opacity", "transform, opacity", "scroll-position", "contents", "TRANSFORM", "Transform", "foo", "auto, transform", "transform, auto", "will-change", "none", "initial", "inherit", "unset", "revert", "default", "all", "inherit, transform", "transform opacity", "-webkit-transform", "left", "width,height", "transform,,opacity", "--x", "contents, scroll-position", "initial, transform", "revert-layer", "top", "SCROLL-POSITION"]))"#,
                r#"["auto => [auto] {will-change: auto;}", "transform => [transform] {will-change: transform;}", "opacity => [opacity] {will-change: opacity;}", "transform, opacity => [transform, opacity] {will-change: transform, opacity;}", "scroll-position => [scroll-position] {will-change: scroll-position;}", "contents => [contents] {will-change: contents;}", "TRANSFORM => [TRANSFORM] {will-change: TRANSFORM;}", "Transform => [Transform] {will-change: Transform;}", "foo => [foo] {will-change: foo;}", "auto, transform => [] {null}", "transform, auto => [] {null}", "will-change => [] {null}", "none => [] {null}", "initial => [initial] {will-change: initial;}", "inherit => [inherit] {will-change: inherit;}", "unset => [unset] {will-change: unset;}", "revert => [revert] {will-change: revert;}", "default => [] {null}", "all => [] {null}", "inherit, transform => [] {null}", "transform opacity => [] {null}", "-webkit-transform => [-webkit-transform] {will-change: -webkit-transform;}", "left => [left] {will-change: left;}", "width,height => [width, height] {will-change: width, height;}", "transform,,opacity => [] {null}", "--x => [--x] {will-change: --x;}", "contents, scroll-position => [contents, scroll-position] {will-change: contents, scroll-position;}", "initial, transform => [] {null}", "revert-layer => [revert-layer] {will-change: revert-layer;}", "top => [top] {will-change: top;}", "SCROLL-POSITION => [scroll-position] {will-change: scroll-position;}"]"#,
            ),
            (
                r#"E(() => P("touch-action", ["auto", "none", "manipulation", "pan-x", "pan-y", "pan-x pan-y", "pan-y pan-x", "pinch-zoom", "pan-x pinch-zoom", "pinch-zoom pan-x", "pan-x pan-y pinch-zoom", "pan-left", "pan-right", "pan-up", "pan-down", "pan-left pan-up", "pan-up pan-left", "pan-left pan-x", "pan-x pan-x", "none pan-x", "manipulation pan-x", "auto pan-x", "foo", "AUTO", "Pan-X", "pan-x pan-y pan-x", "pan-y pinch-zoom pan-x", "pan-left pan-up pinch-zoom", "pinch-zoom pan-left", "pan-down pan-right", "initial", "inherit", "unset", "pan-x, pan-y", "pan-x pinch-zoom pan-y"]))"#,
                r#"["auto => [auto] {touch-action: auto;}", "none => [none] {touch-action: none;}", "manipulation => [manipulation] {touch-action: manipulation;}", "pan-x => [pan-x] {touch-action: pan-x;}", "pan-y => [pan-y] {touch-action: pan-y;}", "pan-x pan-y => [pan-x pan-y] {touch-action: pan-x pan-y;}", "pan-y pan-x => [pan-x pan-y] {touch-action: pan-x pan-y;}", "pinch-zoom => [pinch-zoom] {touch-action: pinch-zoom;}", "pan-x pinch-zoom => [pan-x pinch-zoom] {touch-action: pan-x pinch-zoom;}", "pinch-zoom pan-x => [pan-x pinch-zoom] {touch-action: pan-x pinch-zoom;}", "pan-x pan-y pinch-zoom => [pan-x pan-y pinch-zoom] {touch-action: pan-x pan-y pinch-zoom;}", "pan-left => [pan-left] {touch-action: pan-left;}", "pan-right => [pan-right] {touch-action: pan-right;}", "pan-up => [pan-up] {touch-action: pan-up;}", "pan-down => [pan-down] {touch-action: pan-down;}", "pan-left pan-up => [pan-left pan-up] {touch-action: pan-left pan-up;}", "pan-up pan-left => [pan-left pan-up] {touch-action: pan-left pan-up;}", "pan-left pan-x => [] {null}", "pan-x pan-x => [] {null}", "none pan-x => [] {null}", "manipulation pan-x => [] {null}", "auto pan-x => [] {null}", "foo => [] {null}", "AUTO => [auto] {touch-action: auto;}", "Pan-X => [pan-x] {touch-action: pan-x;}", "pan-x pan-y pan-x => [] {null}", "pan-y pinch-zoom pan-x => [pan-x pan-y pinch-zoom] {touch-action: pan-x pan-y pinch-zoom;}", "pan-left pan-up pinch-zoom => [pan-left pan-up pinch-zoom] {touch-action: pan-left pan-up pinch-zoom;}", "pinch-zoom pan-left => [pan-left pinch-zoom] {touch-action: pan-left pinch-zoom;}", "pan-down pan-right => [pan-right pan-down] {touch-action: pan-right pan-down;}", "initial => [initial] {touch-action: initial;}", "inherit => [inherit] {touch-action: inherit;}", "unset => [unset] {touch-action: unset;}", "pan-x, pan-y => [] {null}", "pan-x pinch-zoom pan-y => [pan-x pan-y pinch-zoom] {touch-action: pan-x pan-y pinch-zoom;}"]"#,
            ),
            (
                r#"E(() => P("perspective", ["none", "100px", "0", "0px", "-1px", "10em", "1.5em", "10%", "auto", "50", "1e2px", "calc(10px + 5px)", "calc(1em + 2px)", "initial", "inherit", "unset", "1in", "1cm", "100PX", "10vw", "0.5px", "+5px", ".5em", "1rem", "min(1px, 2px)", "3rem", "none 1px", "10px 20px", "fit-content", "thin"]))"#,
                r#"["none => [none] {perspective: none;}", "100px => [100px] {perspective: 100px;}", "0 => [0px] {perspective: 0px;}", "0px => [0px] {perspective: 0px;}", "-1px => [] {null}", "10em => [10em] {perspective: 10em;}", "1.5em => [1.5em] {perspective: 1.5em;}", "10% => [] {null}", "auto => [] {null}", "50 => [] {null}", "1e2px => [100px] {perspective: 100px;}", "calc(10px + 5px) => [calc(15px)] {perspective: calc(15px);}", "calc(1em + 2px) => [calc(1em + 2px)] {perspective: calc(1em + 2px);}", "initial => [initial] {perspective: initial;}", "inherit => [inherit] {perspective: inherit;}", "unset => [unset] {perspective: unset;}", "1in => [1in] {perspective: 1in;}", "1cm => [1cm] {perspective: 1cm;}", "100PX => [100px] {perspective: 100px;}", "10vw => [10vw] {perspective: 10vw;}", "0.5px => [0.5px] {perspective: 0.5px;}", "+5px => [5px] {perspective: 5px;}", ".5em => [0.5em] {perspective: 0.5em;}", "1rem => [1rem] {perspective: 1rem;}", "min(1px, 2px) => [calc(1px)] {perspective: calc(1px);}", "3rem => [3rem] {perspective: 3rem;}", "none 1px => [] {null}", "10px 20px => [] {null}", "fit-content => [] {null}", "thin => [] {null}"]"#,
            ),
            (
                r#"E(() => P("perspective-origin", ["center", "50% 50%", "left top", "top left", "left", "top", "right bottom", "10px 20px", "10px", "25%", "center center", "left 10px top 20px", "10px top", "top 10px", "bottom right", "foo", "50%", "1em 2em", "0", "0 0", "left center", "center left", "center top", "top center", "right", "bottom", "10% 20px", "calc(10px + 1%)", "initial", "inherit", "50% 50% 50%", "left 10px", "right 10px bottom 20px", "center 10px", "10px center"]))"#,
                r#"["center => [center center] {perspective-origin: center center;}", "50% 50% => [50% 50%] {perspective-origin: 50% 50%;}", "left top => [left top] {perspective-origin: left top;}", "top left => [left top] {perspective-origin: left top;}", "left => [left center] {perspective-origin: left center;}", "top => [center top] {perspective-origin: center top;}", "right bottom => [right bottom] {perspective-origin: right bottom;}", "10px 20px => [10px 20px] {perspective-origin: 10px 20px;}", "10px => [10px center] {perspective-origin: 10px center;}", "25% => [25% center] {perspective-origin: 25% center;}", "center center => [center center] {perspective-origin: center center;}", "left 10px top 20px => [left 10px top 20px] {perspective-origin: left 10px top 20px;}", "10px top => [10px top] {perspective-origin: 10px top;}", "top 10px => [] {null}", "bottom right => [right bottom] {perspective-origin: right bottom;}", "foo => [] {null}", "50% => [50% center] {perspective-origin: 50% center;}", "1em 2em => [1em 2em] {perspective-origin: 1em 2em;}", "0 => [0px center] {perspective-origin: 0px center;}", "0 0 => [0px 0px] {perspective-origin: 0px 0px;}", "left center => [left center] {perspective-origin: left center;}", "center left => [left center] {perspective-origin: left center;}", "center top => [center top] {perspective-origin: center top;}", "top center => [center top] {perspective-origin: center top;}", "right => [right center] {perspective-origin: right center;}", "bottom => [center bottom] {perspective-origin: center bottom;}", "10% 20px => [10% 20px] {perspective-origin: 10% 20px;}", "calc(10px + 1%) => [calc(1% + 10px) center] {perspective-origin: calc(1% + 10px) center;}", "initial => [initial] {perspective-origin: initial;}", "inherit => [inherit] {perspective-origin: inherit;}", "50% 50% 50% => [] {null}", "left 10px => [left 10px] {perspective-origin: left 10px;}", "right 10px bottom 20px => [right 10px bottom 20px] {perspective-origin: right 10px bottom 20px;}", "center 10px => [center 10px] {perspective-origin: center 10px;}", "10px center => [10px center] {perspective-origin: 10px center;}"]"#,
            ),
        ],
    },
    Case {
        name: "block",
        html: PAGE_1,
        queries: &[
            (
                r"E(() => { const e = document.createElement('div'); e.style.transitionProperty = 'a, b'; e.style.transitionDuration = '1s'; const r = [e.style.transition, e.style.cssText]; e.style.transitionDelay = '2s'; e.style.transitionTimingFunction = 'linear'; r.push(e.style.transition); e.style.transitionBehavior = 'allow-discrete'; r.push(e.style.transition, e.style.cssText); e.style.transitionProperty = 'a'; r.push(e.style.transition); e.style.transitionDuration = '1s, 2s'; r.push(e.style.transition); e.style.transitionProperty = 'a, b'; e.style.transitionDelay = '2s, 3s'; e.style.transitionTimingFunction = 'linear, ease'; e.style.transitionBehavior = 'allow-discrete, normal'; r.push(e.style.transition); return r; })",
                r#"["", "transition-property: a, b; transition-duration: 1s;", "", "a 1s linear 2s allow-discrete, b", "transition: a 1s linear 2s allow-discrete, b;", "a 1s linear 2s allow-discrete", "a 1s linear 2s allow-discrete, 2s", "a 1s linear 2s allow-discrete, b 2s 3s"]"#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); e.style.transition = 'opacity 1s ease-in 2s, width 3s'; return [e.style.transitionProperty, e.style.transitionDuration, e.style.transitionDelay, e.style.transitionTimingFunction, e.style.transitionBehavior, e.style.cssText, e.style.length, e.style[0], e.style[1], e.style[2], e.style[3], e.style[4], e.style[5]]; })",
                r#"["opacity, width", "1s, 3s", "2s, 0s", "ease-in, ease", "normal, normal", "transition: opacity 1s ease-in 2s, width 3s;", 5, "transition-behavior", "transition-duration", "transition-timing-function", "transition-delay", "transition-property", null]"#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); e.style.webkitTransition = 'opacity 1s'; return [e.style.transition, e.style.webkitTransition, e.style.webkitTransitionProperty, e.style.webkitTransitionDuration, e.style.cssText, e.style.length]; })",
                r#"["opacity 1s", "opacity 1s", "opacity", "1s", "transition: opacity 1s;", 5]"#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); e.style.transitionDuration = '300ms'; e.style.willChange = 'transform'; e.style.touchAction = 'none'; e.style.perspective = '100px'; e.style.perspectiveOrigin = 'left'; e.style.transform = 'translate(1px, 2px)'; e.style.left = '5px'; return [e.style.cssText, e.getAttribute('style'), e.style.length, e.style.transitionDuration, e.style.willChange, e.style.touchAction, e.style.perspective, e.style.perspectiveOrigin, e.style.getPropertyValue('transition-duration'), e.style.item(0)]; })",
                r#"["transition-duration: 300ms; will-change: transform; touch-action: none; perspective: 100px; perspective-origin: left center; transform: translate(1px, 2px); left: 5px;", "transition-duration: 300ms; will-change: transform; touch-action: none; perspective: 100px; perspective-origin: left center; transform: translate(1px, 2px); left: 5px;", 7, "300ms", "transform", "none", "100px", "left center", "300ms", "transition-duration"]"#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); e.style.cssText = 'transition: opacity 1s; will-change: transform; -webkit-transition: width 2s'; return [e.style.cssText, e.style.length]; })",
                r#"["will-change: transform; transition: width 2s;", 6]"#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); e.style.transition = 'opacity 1s'; e.style.removeProperty('transition-duration'); return [e.style.transition, e.style.cssText, e.style.length, e.style.transitionProperty]; })",
                r#"["", "transition-behavior: normal; transition-timing-function: ease; transition-delay: 0s; transition-property: opacity;", 4, "opacity"]"#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); e.style.transition = 'opacity 1s'; e.style.transition = ''; return [e.style.cssText, e.style.length, e.getAttribute('style')]; })",
                r#"["", 0, ""]"#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); e.style.transition = 'none'; return [e.style.transition, e.style.transitionProperty, e.style.transitionDuration, e.style.cssText]; })",
                r#"["none", "none", "0s", "transition: none;"]"#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); e.style.setProperty('transition', 'opacity 1s', 'important'); return [e.style.cssText, e.style.getPropertyPriority('transition'), e.style.getPropertyPriority('transition-duration')]; })",
                r#"["transition: opacity 1s !important;", "important", "important"]"#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); e.style.transition = 'var(--x)'; return [e.style.transition, e.style.transitionDuration, e.style.cssText]; })",
                r#"["var(--x)", "", "transition: var(--x);"]"#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); e.style.transition = 'inherit'; e.style.willChange = 'inherit'; return [e.style.transition, e.style.transitionDelay, e.style.willChange, e.style.cssText]; })",
                r#"["inherit", "inherit", "inherit", "transition: inherit; will-change: inherit;"]"#,
            ),
            (
                r"E(() => [CSS.supports('transition', 'opacity 1s'), CSS.supports('transition: opacity 1s'), CSS.supports('will-change', 'transform'), CSS.supports('touch-action', 'none'), CSS.supports('perspective', '10px'), CSS.supports('transition', 'foo bar baz'), CSS.supports('-webkit-transition', 'opacity 1s'), CSS.supports('transition-behavior', 'allow-discrete'), CSS.supports('perspective-origin', 'left'), CSS.supports('will-change', 'none'), CSS.supports('touch-action', 'pan-x pan-left')])",
                r"[true, true, true, true, true, false, true, true, true, false, false]",
            ),
            (
                r"E(() => { const d = document.createElement('div'); const s = d.style; return ['transition', 'transitionProperty', 'transitionDuration', 'transitionDelay', 'transitionTimingFunction', 'transitionBehavior', 'webkitTransition', 'webkitTransitionProperty', 'webkitTransitionDuration', 'webkitTransitionDelay', 'webkitTransitionTimingFunction', 'webkitTransitionBehavior', 'WebkitTransition', 'MozTransition', 'OTransition', 'msTransition', 'willChange', 'webkitWillChange', 'touchAction', 'msTouchAction', 'perspective', 'perspectiveOrigin', 'webkitPerspective', 'webkitPerspectiveOrigin', 'MozPerspective', 'transition-duration', 'will-change', '-webkit-transition', 'cssText'].map(n => n + '=' + (n in s)).join(' '); })",
                r#""transition=true transitionProperty=true transitionDuration=true transitionDelay=true transitionTimingFunction=true transitionBehavior=true webkitTransition=true webkitTransitionProperty=true webkitTransitionDuration=true webkitTransitionDelay=true webkitTransitionTimingFunction=true webkitTransitionBehavior=false WebkitTransition=true MozTransition=false OTransition=false msTransition=false willChange=true webkitWillChange=false touchAction=true msTouchAction=false perspective=true perspectiveOrigin=true webkitPerspective=true webkitPerspectiveOrigin=true MozPerspective=false transition-duration=true will-change=true -webkit-transition=true cssText=true""#,
            ),
            (
                r"E(() => { const s = getComputedStyle(document.body); return [typeof s.transition, typeof s.transitionDuration, typeof s.webkitTransitionDuration, typeof s.willChange, typeof s.touchAction, typeof s.perspective, typeof s.perspectiveOrigin, 'transition' in s, 'webkitTransition' in s, 'MozTransition' in s, 'willChange' in s].join(); })",
                r#""string,string,string,string,string,string,string,true,true,false,true""#,
            ),
        ],
    },
    Case {
        name: "computed",
        html: PAGE_1,
        queries: &[
            (
                r"E(() => C('transition-duration', ['300ms', '1s', '0.5s', '1500ms', '0ms', '0s', '.25s', '100.5ms', '1e1s']))",
                r#"["300ms => 0.3s", "1s => 1s", "0.5s => 0.5s", "1500ms => 1.5s", "0ms => 0s", "0s => 0s", ".25s => 0.25s", "100.5ms => 0.1005s", "1e1s => 10s"]"#,
            ),
            (
                r"E(() => C('transition-delay', ['300ms', '-1s', '0.5s', '1500ms', '0ms', '2s, 3s']))",
                r#"["300ms => 0.3s", "-1s => -1s", "0.5s => 0.5s", "1500ms => 1.5s", "0ms => 0s", "2s, 3s => 2s, 3s"]"#,
            ),
            (
                r"E(() => C('transition-timing-function', ['ease', 'steps(4, end)', 'steps(4, jump-end)', 'steps(4, start)', 'cubic-bezier(.1,.2,.3,.4)', 'linear', 'ease, linear', 'step-start', 'step-end', 'steps(3, jump-both)']))",
                r#"["ease => ease", "steps(4, end) => steps(4)", "steps(4, jump-end) => steps(4)", "steps(4, start) => steps(4, start)", "cubic-bezier(.1,.2,.3,.4) => cubic-bezier(0.1, 0.2, 0.3, 0.4)", "linear => linear", "ease, linear => ease, linear", "step-start => steps(1, start)", "step-end => steps(1)", "steps(3, jump-both) => steps(3, jump-both)"]"#,
            ),
            (
                r"E(() => C('transition-property', ['all', 'none', 'Width', 'opacity, FOO', '-webkit-transform']))",
                r#"["all => all", "none => none", "Width => width", "opacity, FOO => opacity, FOO", "-webkit-transform => -webkit-transform"]"#,
            ),
            (
                r"E(() => C('transition-behavior', ['normal', 'allow-discrete', 'allow-discrete, normal']))",
                r#"["normal => normal", "allow-discrete => allow-discrete", "allow-discrete, normal => allow-discrete, normal"]"#,
            ),
            (
                r"E(() => C('transition', ['opacity 1s', 'all', 'opacity 1s ease 2s allow-discrete, width 3s', 'none', 'all 1s', '1s steps(2)']))",
                r#"["opacity 1s => opacity 1s", "all => all", "opacity 1s ease 2s allow-discrete, width 3s => opacity 1s 2s allow-discrete, width 3s", "none => none", "all 1s => 1s", "1s steps(2) => 1s steps(2)"]"#,
            ),
            (
                r"E(() => C('perspective', ['none', '100px', '10em', '1.5em', '0', '1in', '10vw', 'calc(1em + 2px)', '1rem', '1cm', '0.5px', '33.333333px']))",
                r#"["none => none", "100px => 100px", "10em => 200px", "1.5em => 30px", "0 => 0px", "1in => 96px", "10vw => 80px", "calc(1em + 2px) => 22px", "1rem => 16px", "1cm => 37.7953px", "0.5px => 0.5px", "33.333333px => 33.3333px"]"#,
            ),
            (
                r"E(() => C('perspective-origin', ['center', '50% 50%', 'left top', 'left', 'top', 'right bottom', '10px 20px', '10px', '25%', 'left 10px top 20px', '1em 2em', '0', 'calc(10px + 1%) 5px', 'bottom right', 'right 10px bottom 20px', '10% 20px']))",
                r#"["center => 200px 50px", "50% 50% => 200px 50px", "left top => 0px 0px", "left => 0px 50px", "top => 200px 0px", "right bottom => 400px 100px", "10px 20px => 10px 20px", "10px => 10px 50px", "25% => 100px 50px", "left 10px top 20px => 10px 20px", "1em 2em => 20px 40px", "0 => 0px 50px", "calc(10px + 1%) 5px => 14px 5px", "bottom right => 400px 100px", "right 10px bottom 20px => 390px 80px", "10% 20px => 40px 20px"]"#,
            ),
            (
                r"E(() => C('will-change', ['auto', 'transform', 'transform, opacity', 'TRANSFORM', 'contents, scroll-position']))",
                r#"["auto => auto", "transform => transform", "transform, opacity => transform, opacity", "TRANSFORM => TRANSFORM", "contents, scroll-position => contents, scroll-position"]"#,
            ),
            (
                r"E(() => C('touch-action', ['auto', 'pan-y pan-x', 'pinch-zoom pan-x', 'none', 'manipulation', 'pan-left pan-up']))",
                r#"["auto => auto", "pan-y pan-x => pan-x pan-y", "pinch-zoom pan-x => pan-x pinch-zoom", "none => none", "manipulation => manipulation", "pan-left pan-up => pan-left pan-up"]"#,
            ),
            (
                r"E(() => G('#p', ['will-change', 'touch-action', 'perspective', 'perspective-origin', '-webkit-perspective', '-webkit-perspective-origin']))",
                r#"["will-change: transform", "touch-action: pan-x pan-y", "perspective: 200px", "perspective-origin: 0px 10px", "-webkit-perspective: 200px", "-webkit-perspective-origin: 0px 10px"]"#,
            ),
            (
                r"E(() => G('#k', ['will-change', 'touch-action', 'perspective', 'perspective-origin']))",
                r#"["will-change: transform", "touch-action: pan-x pan-y", "perspective: 200px", "perspective-origin: 0px 10px"]"#,
            ),
            (
                r"E(() => G('#w', ['will-change', 'touch-action', 'perspective', 'perspective-origin']))",
                r#"["will-change: scroll-position, opacity", "touch-action: none", "perspective: 5px", "perspective-origin: 400px 0px"]"#,
            ),
            (
                r"E(() => G('#w2', ['will-change', 'touch-action', 'perspective', 'perspective-origin']))",
                r#"["will-change: auto", "touch-action: auto", "perspective: none", "perspective-origin: 200px 0px"]"#,
            ),
            (
                r"E(() => G('#c', ['will-change', 'touch-action', 'perspective', 'perspective-origin']))",
                r#"["will-change: auto", "touch-action: auto", "perspective: none", "perspective-origin: 200px 150px"]"#,
            ),
            (
                r"E(() => { const s = getComputedStyle(document.getElementById('d')); return [s.willChange, s.touchAction, s.perspective, s.perspectiveOrigin, s.webkitPerspective, s.webkitPerspectiveOrigin]; })",
                r#"["auto", "auto", "none", "200px 50px", "none", "200px 50px"]"#,
            ),
        ],
    },
];
