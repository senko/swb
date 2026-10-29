// Where swb departs from Chromium on purpose in the cases of
// `cases.rs`: (case, query index, swb's value, why). The reasons are in
// ADR 0028, "Feature 7b notes".

/// The departures.
pub(crate) const SWB_VALUES: &[(&str, usize, &str, &str)] = &[
    (
        r"inline",
        1,
        r##"["#w [8 8 200 115]","#s1 [39.140625 9 163.375 17 | 8 29 125.390625 17]","#s2 [161.28125 26 36.703125 23]","#s3 [200.984375 29 0 17]","#b1 [8 49 48 17]","#i1 [46.21875 49 9.78125 17]","#br [56 49 0 17]","#ib [8 68 50 30]","#im [58 88 20 10]","#lnk [82.453125 84 85.375 17 | 8 104 152.984375 17]","#s4 [163.984375 106 17.78125 17]"]"##,
        r"Chromium gives an inline box without borders, padding and background (a culled inline) the rectangles of its children; swb gives one rectangle per line",
    ),
    (
        r"positioned",
        0,
        r##"["#rel 18 13 210 110","#abs 40 45 52 42","#inner 43 48 10 10","#abs2 179.890625 103.5 25.5 10","#fix 770 100 30 30","#fp 770 100 30 5","#st 8 118 784 20","#plain 8 138 784 20","#sp 8 139 8 17","#rel2 12 162 776 23","#stat 19 169 762 9","#deep 21 171 758 5"]"##,
        r"swb's layout does not snap each length to layout units (`right: 10%` of 206px is 20.59375px in Chromium; backlog of 7a2)",
    ),
    (
        r"positioned",
        1,
        r##"["#rel [18 13 210 110]","#abs [40 45 52 42]","#inner [43 48 10 10]","#abs2 [179.890625 103.5 25.5 10]","#fix [770 100 30 30]","#fp [770 100 30 5]","#st [8 118 784 20]","#plain [8 138 784 20]","#sp [8 139 8 17]","#rel2 [12 162 776 23]","#stat [19 169 762 9]","#deep [21 171 758 5]"]"##,
        r"swb's layout does not snap each length to layout units",
    ),
    (
        r"tables",
        3,
        r##"["#t 1 1 72 82","#cap 0 0 74 20","#tb 0 0 68 58","#tr1 0 0 68 28","#td1 1 1 15 26","#td2 1 1 47 26","#d 0 0 41 20","#tr2 0 0 68 28","#th1 1 1 66 26"]"##,
        r"Chromium's `clientWidth` and `clientHeight` of a table are those of its wrapper box with the caption and without the borders (74x84); swb's are the padding box of the table box",
    ),
    (
        r"svg",
        0,
        r##"["#s 13 13 206 106","#g 31 41 105 55","#r 31 41 50 30","#ci 116 76 20 20","#tx 0 0 0 0","#fo 0 0 0 0","#fd 0 0 0 0"]"##,
        r"swb has no boxes for SVG `text` and for `foreignObject` and its content",
    ),
    (
        r"svg",
        1,
        r##"["#s [13 13 206 106]","#g [31 41 105 55]","#r [31 41 50 30]","#ci [116 76 20 20]","#tx []","#fo []","#fd []"]"##,
        r"swb has no boxes for SVG `text` and for `foreignObject` and its content",
    ),
    (
        r"svg",
        2,
        r##"["#s 1 1 204 104","#g 0 0 0 0","#r 0 0 0 0","#ci 0 0 0 0","#tx 0 0 0 0","#fo 0 0 0 0","#fd 0 0 0 0"]"##,
        r"swb has no boxes for SVG `text` and for `foreignObject`; Chromium's `clientWidth` of SVG `text` is not its size (44x93)",
    ),
    (
        r"svg",
        3,
        r##"["#s 0 0 204 104","#g 0 0 0 0","#r 0 0 0 0","#ci 0 0 0 0","#tx 0 0 0 0","#fo 0 0 0 0","#fd 0 0 0 0"]"##,
        r"swb has no boxes for SVG `text` and for `foreignObject`",
    ),
    (
        r"svg",
        4,
        r#"["undefined","undefined",false,true,null,0,0]"#,
        r"swb has no box for the content of `foreignObject`",
    ),
    (
        r"svg",
        5,
        r##"["#fd null 0 0 0 0","#after BODY 8 129 784 20"]"##,
        r"swb has no box for the content of `foreignObject`",
    ),
    (
        r"root-short-quirks",
        0,
        r#"["HTML 0 0 800 36","BODY 0 0 800 600"]"#,
        r"swb's layout does not stretch the root and the body to the viewport in quirks mode (backlog of 7a2)",
    ),
    (
        r"root-short-quirks",
        2,
        r#"["HTML null 0 0 800 36","BODY null 0 0 784 20"]"#,
        r"swb's layout does not stretch the root and the body to the viewport in quirks mode (backlog of 7a2)",
    ),
    (
        r"not-rendered",
        0,
        r##"["HEAD 0 0 0 0","TITLE 0 0 0 0","#n 0 0 0 0","#np 0 0 0 0","#dc 0 0 0 0","#op 0 0 0 0","#ar 0 0 0 0","#empty 38 53 0 17","#vh 8 72 784 5"]"##,
        r"Chromium gives an `area` without an image the position of its `map`; swb gives it no box",
    ),
    (
        r"not-rendered",
        1,
        r##"["HEAD []","#n []","#np []","#dc []","#dcp [8 16 784 20]","#op []","#ar []","#empty [38 53 0 17]","#vh [8 72 784 5]"]"##,
        r"Chromium gives an `area` without an image the position of its `map`; swb gives it no box",
    ),
    (
        r"not-rendered",
        2,
        r##"["HEAD null 0 0 0 0","#n null 0 0 0 0","#np null 0 0 0 0","#dc null 0 0 0 0","#dcp BODY 8 16 784 20","#op null 0 0 0 0","#ar null 0 0 0 0","#empty BODY 38 53 0 17","#vh BODY 8 72 784 5"]"##,
        r"Chromium gives an `area` without an image the position of its `map`; swb gives it no box",
    ),
    (
        r"window-metrics",
        3,
        r#"["availWidth,availHeight,width,height,colorDepth,pixelDepth,availLeft,availTop,orientation,constructor","angle,type,constructor"]"#,
        r"swb's `Screen` has no `onchange` and `isExtended`, `ScreenOrientation` no `onchange`, `lock` and `unlock`",
    ),
    (
        r"flex-grid-float",
        0,
        r##"["#fx 8 8 306 46","#f1 11 21 87.65625 20","#f2 107.15625 21 175.328125 20","#f3 291 21 20 20","#gr 8 54 301 20","#g1 8 54 50 20","#g2 63 54 80.328125 20","#g3 148.328125 54 160.65625 20","#fl 8 74 60 30","#fp 8 74 200 80","#cl 8 154 784 0","#ul 8 158 784 40","#li1 38 158 754 20","#li2 38 178 754 20"]"##,
        r"swb's flex and grid layout does not snap the shares of free space to layout units",
    ),
    (
        r"flex-grid-float",
        1,
        r##"["#fx [8 8 306 46]","#f1 [11 21 87.65625 20]","#f2 [107.15625 21 175.328125 20]","#f3 [291 21 20 20]","#gr [8 54 301 20]","#g1 [8 54 50 20]","#g2 [63 54 80.328125 20]","#g3 [148.328125 54 160.65625 20]","#fl [8 74 60 30]","#fp [8 74 200 80]","#cl [8 154 784 0]","#ul [8 158 784 40]","#li1 [38 158 754 20]","#li2 [38 178 754 20]"]"##,
        r"swb's flex and grid layout does not snap the shares of free space to layout units",
    ),
    (
        r"block-in-inline",
        0,
        r##"["#w 8 8 300 60","#s 8 9 300 57","#d 8 28 300 20","#ib1 8 68 784 60","#ibs 20.453125 68 40 60","#pr 8 144 784 20","#rot 31.34375 139 10 30","#sk 61.410858154296875 146.5 35.45953369140625 15"]"##,
        r"swb's skew and scale matrix is computed in `f32` (Chromium in `f64`): the last digits differ",
    ),
    (
        r"block-in-inline",
        1,
        r##"["#w [8 8 300 60]","#s [8 9 45.375 17 | 8 28 300 20 | 8 49 32.015625 17]","#d [8 28 300 20]","#ib1 [8 68 784 60]","#ibs [20.453125 68 40 60]","#pr [8 144 784 20]","#rot [31.34375 139 10 30]","#sk [61.410858154296875 146.5 35.45953369140625 15]"]"##,
        r"swb's skew and scale matrix is computed in `f32` (Chromium in `f64`): the last digits differ",
    ),
    (
        r"nested-transforms",
        0,
        r##"["#o -8.30126953125 -8.30126953125 136.6025390625 136.6025390625","#i 11.028854370117188 -10.131397247314453 102.451904296875 102.451904296875","#c 41.02885437011719 -10.131397247314453 20.490379333496094 20.490379333496094","#sc 10 170 100 100","#sct 10 280 100 300","#sctc 10 280 10 10"]"##,
        r"swb's matrices are computed in `f32` (Chromium in `f64`): the last digits differ",
    ),
    (
        r"item4-img-size",
        0,
        r#"[33554428,0,"1410065408",0]"#,
        r"the test's fetcher serves no `data:` image, so the height has no natural size here (the probe with swb gives 33554428); swb's `naturalWidth` is 0",
    ),
    (
        r"window",
        35,
        r"[0,500]",
        r"`behavior: 'smooth'` scrolls at once in swb (Chromium animates)",
    ),
    (
        r"element",
        15,
        r"[50]",
        r"`behavior: 'smooth'` scrolls at once in swb (Chromium animates)",
    ),
    (
        r"into-view",
        28,
        r"[450,1500]",
        r"`behavior: 'smooth'` scrolls at once in swb (Chromium animates)",
    ),
    (
        r"huge",
        0,
        r"[33554432,33554432,33554432,33554432]",
        r"Chromium's largest box is 33554428px; swb's layout limit is 33554432px",
    ),
    (
        r"huge",
        2,
        r"[0,33553832,600,33554432]",
        r"Chromium's largest box is 33554428px; swb's layout limit is 33554432px",
    ),
    (
        r"window-metrics",
        7,
        r#"["function","function","function","function","undefined"]"#,
        r"swb has no `caretPositionFromPoint`",
    ),
    (
        r"domrect",
        15,
        r#"["scrollTop,scrollLeft,scrollWidth,scrollHeight,clientTop,clientLeft,clientWidth,clientHeight,getBoundingClientRect,getClientRects,scroll,scrollBy,scrollIntoView,scrollTo","innerText,offsetParent,offsetTop,offsetLeft,offsetWidth,offsetHeight,onscroll,onscrollend","scrollingElement,onscroll,onscrollend,elementFromPoint,elementsFromPoint"]"#,
        r"swb has no `scrollIntoViewIfNeeded`, `onscrollsnapchange`, `onscrollsnapchanging`, `caretPositionFromPoint`, `caretRangeFromPoint`, `exitPointerLock`; `innerText` comes before the `offset*` members (the order of the IDL files)",
    ),
];
