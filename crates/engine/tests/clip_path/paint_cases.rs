// The cases of `tools/probes/clip-path.json` with the values that Chromium 148 gave
// (`just probe tools/probes/clip-path.json`, three runs, all equal); where swb departs on
// purpose, the test overrides the value. Generated.

use crate::PaintCase;

pub(crate) const CASES: &[PaintCase] = &[
    PaintCase {
        name: "cp-paint-1",
        html: r#"<!DOCTYPE html><html><head><title>t</title><style>
body{margin:0;background:#fff}
.w{position:absolute;width:200px;height:150px}
.t{box-sizing:content-box;width:80px;height:40px;padding:10px;border:10px solid #000;margin:20px 30px;background:#888;background-clip:padding-box}
.k{position:absolute;left:-30px;top:-10px;width:30px;height:30px;background:#444}
</style></head><body><div class=w id=w0 style="left:5px;top:5px"><div class=t id=t0 style="clip-path:none"></div></div><div class=w id=w1 style="left:215px;top:5px"><div class=t id=t1 style="clip-path:inset(10px)"></div></div><div class=w id=w2 style="left:425px;top:5px"><div class=t id=t2 style="clip-path:inset(10px 20px 30px 40px)"></div></div><div class=w id=w3 style="left:635px;top:5px"><div class=t id=t3 style="clip-path:inset(10% 20%)"></div></div><div class=w id=w4 style="left:5px;top:165px"><div class=t id=t4 style="clip-path:inset(10px round 20px)"></div></div><div class=w id=w5 style="left:215px;top:165px"><div class=t id=t5 style="clip-path:inset(0 round 10px 20px 30px 40px / 5px 10px 15px 20px)"></div></div><div class=w id=w6 style="left:425px;top:165px"><div class=t id=t6 style="clip-path:inset(0 round 50%)"></div></div><div class=w id=w7 style="left:635px;top:165px"><div class=t id=t7 style="clip-path:inset(-10px)"></div></div><div class=w id=w8 style="left:5px;top:325px"><div class=t id=t8 style="clip-path:inset(50%)"></div></div><div class=w id=w9 style="left:215px;top:325px"><div class=t id=t9 style="clip-path:inset(100%)"></div></div><div class=w id=w10 style="left:425px;top:325px"><div class=t id=t10 style="clip-path:inset(calc(10px + 5%))"></div></div><div class=w id=w11 style="left:635px;top:325px"><div class=t id=t11 style="clip-path:inset(60px 0 0 0)"></div></div><div class=w id=w12 style="left:5px;top:485px"><div class=t id=t12 style="clip-path:circle()"></div></div><div class=w id=w13 style="left:215px;top:485px"><div class=t id=t13 style="clip-path:circle(30px)"></div></div><div class=w id=w14 style="left:425px;top:485px"><div class=t id=t14 style="clip-path:circle(50%)"></div></div><div class=w id=w15 style="left:635px;top:485px"><div class=t id=t15 style="clip-path:circle(closest-side at 20px 20px)"></div></div></body></html>"#,
        items: &[
            ([5.0, 5.0, 200.0, 150.0], 6400.0, Some([30, 20, 120, 80])),
            ([215.0, 5.0, 200.0, 150.0], 2800.0, Some([40, 30, 100, 60])),
            ([425.0, 5.0, 200.0, 150.0], 1120.0, Some([70, 30, 60, 40])),
            ([635.0, 5.0, 200.0, 150.0], 2304.0, Some([54, 28, 72, 64])),
            ([5.0, 165.0, 200.0, 150.0], 2639.09, Some([40, 30, 100, 60])),
            (
                [215.0, 165.0, 200.0, 150.0],
                6071.33,
                Some([30, 20, 120, 80]),
            ),
            (
                [425.0, 165.0, 200.0, 150.0],
                4399.8,
                Some([30, 20, 120, 80]),
            ),
            (
                [635.0, 165.0, 200.0, 150.0],
                6400.0,
                Some([30, 20, 120, 80]),
            ),
            ([5.0, 325.0, 200.0, 150.0], 0.0, None),
            ([215.0, 325.0, 200.0, 150.0], 0.0, None),
            (
                [425.0, 325.0, 200.0, 150.0],
                2135.47,
                Some([46, 34, 88, 52]),
            ),
            (
                [635.0, 325.0, 200.0, 150.0],
                1866.67,
                Some([30, 80, 120, 20]),
            ),
            ([5.0, 485.0, 200.0, 150.0], 2710.51, Some([50, 20, 80, 80])),
            (
                [215.0, 485.0, 200.0, 150.0],
                1313.63,
                Some([60, 30, 60, 60]),
            ),
            (
                [425.0, 485.0, 200.0, 150.0],
                4162.02,
                Some([39, 20, 102, 80]),
            ),
            ([635.0, 485.0, 200.0, 150.0], 829.88, Some([30, 20, 40, 40])),
        ],
    },
    PaintCase {
        name: "cp-paint-2",
        html: r#"<!DOCTYPE html><html><head><title>t</title><style>
body{margin:0;background:#fff}
.w{position:absolute;width:200px;height:150px}
.t{box-sizing:content-box;width:80px;height:40px;padding:10px;border:10px solid #000;margin:20px 30px;background:#888;background-clip:padding-box}
.k{position:absolute;left:-30px;top:-10px;width:30px;height:30px;background:#444}
</style></head><body><div class=w id=w0 style="left:5px;top:5px"><div class=t id=t0 style="clip-path:circle(farthest-side at 20px 20px)"></div></div><div class=w id=w1 style="left:215px;top:5px"><div class=t id=t1 style="clip-path:circle(40px at right 10px bottom 20px)"></div></div><div class=w id=w2 style="left:425px;top:5px"><div class=t id=t2 style="clip-path:circle(at left top)"></div></div><div class=w id=w3 style="left:635px;top:5px"><div class=t id=t3 style="clip-path:circle(0)"></div></div><div class=w id=w4 style="left:5px;top:165px"><div class=t id=t4 style="clip-path:circle(500px)"></div></div><div class=w id=w5 style="left:215px;top:165px"><div class=t id=t5 style="clip-path:circle(25% at 100% 0)"></div></div><div class=w id=w6 style="left:425px;top:165px"><div class=t id=t6 style="clip-path:ellipse(30px 20px)"></div></div><div class=w id=w7 style="left:635px;top:165px"><div class=t id=t7 style="clip-path:ellipse(50% 25% at 25% 75%)"></div></div><div class=w id=w8 style="left:5px;top:325px"><div class=t id=t8 style="clip-path:ellipse(farthest-side closest-side)"></div></div><div class=w id=w9 style="left:215px;top:325px"><div class=t id=t9 style="clip-path:ellipse()"></div></div><div class=w id=w10 style="left:425px;top:325px"><div class=t id=t10 style="clip-path:polygon(50% 0, 100% 100%, 0 100%)"></div></div><div class=w id=w11 style="left:635px;top:325px"><div class=t id=t11 style="clip-path:polygon(nonzero, 50% 0%, 79% 90%, 2% 35%, 98% 35%, 21% 90%)"></div></div><div class=w id=w12 style="left:5px;top:485px"><div class=t id=t12 style="clip-path:polygon(evenodd, 50% 0%, 79% 90%, 2% 35%, 98% 35%, 21% 90%)"></div></div><div class=w id=w13 style="left:215px;top:485px"><div class=t id=t13 style="clip-path:polygon(10px 10px, 110px 10px, 60px 70px)"></div></div><div class=w id=w14 style="left:425px;top:485px"><div class=t id=t14 style="clip-path:polygon(0 0, 100% 100%)"></div></div><div class=w id=w15 style="left:635px;top:485px"><div class=t id=t15 style="clip-path:polygon(10px 10px)"></div></div></body></html>"#,
        items: &[
            ([5.0, 5.0, 200.0, 150.0], 6002.33, Some([30, 20, 120, 80])),
            ([215.0, 5.0, 200.0, 150.0], 1730.25, Some([100, 40, 50, 60])),
            ([425.0, 5.0, 200.0, 150.0], 0.0, None),
            ([635.0, 5.0, 200.0, 150.0], 0.0, None),
            ([5.0, 165.0, 200.0, 150.0], 6400.0, Some([30, 20, 120, 80])),
            (
                [215.0, 165.0, 200.0, 150.0],
                447.07,
                Some([124, 20, 26, 26]),
            ),
            ([425.0, 165.0, 200.0, 150.0], 879.17, Some([60, 40, 60, 40])),
            (
                [635.0, 165.0, 200.0, 150.0],
                1907.57,
                Some([30, 60, 90, 40]),
            ),
            ([5.0, 325.0, 200.0, 150.0], 4397.62, Some([30, 20, 120, 80])),
            (
                [215.0, 325.0, 200.0, 150.0],
                4396.73,
                Some([30, 20, 120, 80]),
            ),
            (
                [425.0, 325.0, 200.0, 150.0],
                2883.86,
                Some([30, 20, 120, 80]),
            ),
            (
                [635.0, 325.0, 200.0, 150.0],
                1293.46,
                Some([32, 20, 116, 72]),
            ),
            ([5.0, 485.0, 200.0, 150.0], 908.33, Some([32, 20, 116, 72])),
            (
                [215.0, 485.0, 200.0, 150.0],
                1400.24,
                Some([40, 30, 100, 60]),
            ),
            ([425.0, 485.0, 200.0, 150.0], 0.0, None),
            ([635.0, 485.0, 200.0, 150.0], 0.0, None),
        ],
    },
    PaintCase {
        name: "cp-paint-3",
        html: r#"<!DOCTYPE html><html><head><title>t</title><style>
body{margin:0;background:#fff}
.w{position:absolute;width:200px;height:150px}
.t{box-sizing:content-box;width:80px;height:40px;padding:10px;border:10px solid #000;margin:20px 30px;background:#888;background-clip:padding-box}
.k{position:absolute;left:-30px;top:-10px;width:30px;height:30px;background:#444}
</style></head><body><div class=w id=w0 style="left:5px;top:5px"><div class=t id=t0 style="clip-path:path('M0 0 L60 0 L0 60 z')"></div></div><div class=w id=w1 style="left:215px;top:5px"><div class=t id=t1 style="clip-path:path(evenodd, 'M0 0h100v60h-100z M20 20h40v20h-40z')"></div></div><div class=w id=w2 style="left:425px;top:5px"><div class=t id=t2 style="clip-path:path(nonzero, 'M0 0h100v60h-100z M20 20h40v20h-40z')"></div></div><div class=w id=w3 style="left:635px;top:5px"><div class=t id=t3 style="clip-path:path('M10 40 C 10 0 90 0 90 40 S 50 80 10 40 z')"></div></div><div class=w id=w4 style="left:5px;top:165px"><div class=t id=t4 style="clip-path:path('M60 10 A 40 30 0 1 1 59.9 10 z')"></div></div><div class=w id=w5 style="left:215px;top:165px"><div class=t id=t5 style="clip-path:path('M0 0 L100 0 L100 50')"></div></div><div class=w id=w6 style="left:425px;top:165px"><div class=t id=t6 style="clip-path:path('m10 10 l60 0 l0 40 z')"></div></div><div class=w id=w7 style="left:635px;top:165px"><div class=t id=t7 style="clip-path:path('M-20 -20 L40 -20 L40 40 z')"></div></div><div class=w id=w8 style="left:5px;top:325px"><div class=t id=t8 style="clip-path:path('M0 0h100v60h-100z M20 20h40v20h-40z')"></div></div><div class=w id=w9 style="left:215px;top:325px"><div class=t id=t9 style="clip-path:path(nonzero, 'M0 0h100v60h-100z M20 20v20h40v-20z')"></div></div><div class=w id=w10 style="left:425px;top:325px"><div class=t id=t10 style="clip-path:path(evenodd, 'M0 0h100v60h-100z M20 20v20h40v-20z')"></div></div><div class=w id=w11 style="left:635px;top:325px"><div class=t id=t11 style="clip-path:path('M0 0h100v60h-100z M20 20v20h40v-20z')"></div></div><div class=w id=w12 style="left:5px;top:485px"><div class=t id=t12 style="clip-path:polygon(nonzero, 0 0, 100px 0, 100px 60px, 0 60px, 0 0, 20px 20px, 20px 40px, 60px 40px, 60px 20px, 20px 20px)"></div></div><div class=w id=w13 style="left:215px;top:485px"><div class=t id=t13 style="clip-path:polygon(evenodd, 0 0, 100px 0, 100px 60px, 0 60px, 0 0, 20px 20px, 60px 20px, 60px 40px, 20px 40px, 20px 20px)"></div></div><div class=w id=w14 style="left:425px;top:485px"><div class=t id=t14 style="clip-path:rect(10px 90% 70px 10px)"></div></div><div class=w id=w15 style="left:635px;top:485px"><div class=t id=t15 style="clip-path:rect(auto auto auto auto)"></div></div><div class=w id=w16 style="left:5px;top:645px"><div class=t id=t16 style="clip-path:rect(10px 100px 70px 10px round 15px)"></div></div><div class=w id=w17 style="left:215px;top:645px"><div class=t id=t17 style="clip-path:rect(70px 10px 10px 90px)"></div></div><div class=w id=w18 style="left:425px;top:645px"><div class=t id=t18 style="clip-path:xywh(10px 10px 50% 50%)"></div></div><div class=w id=w19 style="left:635px;top:645px"><div class=t id=t19 style="clip-path:xywh(0 0 100% 100% round 10px)"></div></div><div class=w id=w20 style="left:5px;top:805px"><div class=t id=t20 style="clip-path:xywh(-10px -10px 50px 50px)"></div></div><div class=w id=w21 style="left:215px;top:805px"><div class=t id=t21 style="clip-path:xywh(0 0 0 0)"></div></div></body></html>"#,
        items: &[
            ([5.0, 5.0, 200.0, 150.0], 1373.45, Some([30, 20, 60, 60])),
            ([215.0, 5.0, 200.0, 150.0], 3226.67, Some([30, 20, 100, 60])),
            ([425.0, 5.0, 200.0, 150.0], 3226.67, Some([30, 20, 100, 60])),
            ([635.0, 5.0, 200.0, 150.0], 1678.22, Some([40, 30, 80, 60])),
            ([5.0, 165.0, 200.0, 150.0], 1756.76, Some([49, 30, 81, 60])),
            (
                [215.0, 165.0, 200.0, 150.0],
                1646.67,
                Some([30, 20, 100, 50]),
            ),
            ([425.0, 165.0, 200.0, 150.0], 560.37, Some([40, 30, 60, 40])),
            ([635.0, 165.0, 200.0, 150.0], 560.08, Some([30, 20, 40, 40])),
            ([5.0, 325.0, 200.0, 150.0], 3226.67, Some([30, 20, 100, 60])),
            (
                [215.0, 325.0, 200.0, 150.0],
                3226.67,
                Some([30, 20, 100, 60]),
            ),
            (
                [425.0, 325.0, 200.0, 150.0],
                3226.67,
                Some([30, 20, 100, 60]),
            ),
            (
                [635.0, 325.0, 200.0, 150.0],
                3226.67,
                Some([30, 20, 100, 60]),
            ),
            ([5.0, 485.0, 200.0, 150.0], 3226.67, Some([30, 20, 100, 60])),
            (
                [215.0, 485.0, 200.0, 150.0],
                3226.67,
                Some([30, 20, 100, 60]),
            ),
            ([425.0, 485.0, 200.0, 150.0], 2744.0, Some([40, 30, 98, 60])),
            (
                [635.0, 485.0, 200.0, 150.0],
                6400.0,
                Some([30, 20, 120, 80]),
            ),
            ([5.0, 645.0, 200.0, 150.0], 2427.4, Some([40, 30, 90, 60])),
            ([215.0, 645.0, 200.0, 150.0], 0.0, None),
            ([425.0, 645.0, 200.0, 150.0], 1120.0, Some([40, 30, 60, 40])),
            (
                [635.0, 645.0, 200.0, 150.0],
                6304.74,
                Some([30, 20, 120, 80]),
            ),
            ([5.0, 805.0, 200.0, 150.0], 1120.0, Some([30, 20, 40, 40])),
            ([215.0, 805.0, 200.0, 150.0], 0.0, None),
        ],
    },
    PaintCase {
        name: "cp-paint-boxes",
        html: r#"<!DOCTYPE html><html><head><title>t</title><style>
body{margin:0;background:#fff}
.w{position:absolute;width:200px;height:150px}
.t{box-sizing:content-box;width:80px;height:40px;padding:10px;border:10px solid #000;margin:20px 30px;background:#888;background-clip:padding-box}
.k{position:absolute;left:-30px;top:-10px;width:30px;height:30px;background:#444}
</style></head><body><div class=w id=w0 style="left:5px;top:5px"><div class=t id=t0 style="clip-path:circle(50% at 50% 50%) border-box"></div></div><div class=w id=w1 style="left:215px;top:5px"><div class=t id=t1 style="clip-path:circle(50%) padding-box"></div></div><div class=w id=w2 style="left:425px;top:5px"><div class=t id=t2 style="clip-path:circle(50%) content-box"></div></div><div class=w id=w3 style="left:635px;top:5px"><div class=t id=t3 style="clip-path:circle(50%) margin-box"></div></div><div class=w id=w4 style="left:5px;top:165px"><div class=t id=t4 style="clip-path:inset(0) margin-box"></div></div><div class=w id=w5 style="left:215px;top:165px"><div class=t id=t5 style="clip-path:inset(0) content-box"></div></div><div class=w id=w6 style="left:425px;top:165px"><div class=t id=t6 style="clip-path:inset(0) fill-box"></div></div><div class=w id=w7 style="left:635px;top:165px"><div class=t id=t7 style="clip-path:inset(0) stroke-box"></div></div><div class=w id=w8 style="left:5px;top:325px"><div class=t id=t8 style="clip-path:inset(0) view-box"></div></div><div class=w id=w9 style="left:215px;top:325px"><div class=t id=t9 style="clip-path:border-box"></div></div><div class=w id=w10 style="left:425px;top:325px"><div class=t id=t10 style="clip-path:padding-box"></div></div><div class=w id=w11 style="left:635px;top:325px"><div class=t id=t11 style="clip-path:content-box"></div></div><div class=w id=w12 style="left:5px;top:485px"><div class=t id=t12 style="clip-path:margin-box"></div></div><div class=w id=w13 style="left:215px;top:485px"><div class=t id=t13 style="clip-path:fill-box"></div></div><div class=w id=w14 style="left:425px;top:485px"><div class=t id=t14 style="clip-path:stroke-box"></div></div><div class=w id=w15 style="left:635px;top:485px"><div class=t id=t15 style="clip-path:view-box"></div></div></body></html>"#,
        items: &[
            ([5.0, 5.0, 200.0, 150.0], 4162.02, Some([39, 20, 102, 80])),
            ([215.0, 5.0, 200.0, 150.0], 2904.11, Some([48, 20, 84, 80])),
            ([425.0, 5.0, 200.0, 150.0], 1475.56, Some([58, 28, 64, 64])),
            ([635.0, 5.0, 200.0, 150.0], 6400.0, Some([30, 20, 120, 80])),
            ([5.0, 165.0, 200.0, 150.0], 6400.0, Some([30, 20, 120, 80])),
            (
                [215.0, 165.0, 200.0, 150.0],
                1493.33,
                Some([50, 40, 80, 40]),
            ),
            (
                [425.0, 165.0, 200.0, 150.0],
                1493.33,
                Some([50, 40, 80, 40]),
            ),
            (
                [635.0, 165.0, 200.0, 150.0],
                6400.0,
                Some([30, 20, 120, 80]),
            ),
            ([5.0, 325.0, 200.0, 150.0], 6400.0, Some([30, 20, 120, 80])),
            (
                [215.0, 325.0, 200.0, 150.0],
                6400.0,
                Some([30, 20, 120, 80]),
            ),
            (
                [425.0, 325.0, 200.0, 150.0],
                2800.0,
                Some([40, 30, 100, 60]),
            ),
            (
                [635.0, 325.0, 200.0, 150.0],
                1493.33,
                Some([50, 40, 80, 40]),
            ),
            ([5.0, 485.0, 200.0, 150.0], 6400.0, Some([30, 20, 120, 80])),
            (
                [215.0, 485.0, 200.0, 150.0],
                1493.33,
                Some([50, 40, 80, 40]),
            ),
            (
                [425.0, 485.0, 200.0, 150.0],
                6400.0,
                Some([30, 20, 120, 80]),
            ),
            (
                [635.0, 485.0, 200.0, 150.0],
                6400.0,
                Some([30, 20, 120, 80]),
            ),
        ],
    },
    PaintCase {
        name: "cp-paint-other",
        html: r#"<!DOCTYPE html><html><head><title>t</title><style>
body{margin:0;background:#fff}
.w{position:absolute;width:200px;height:150px}
.t{box-sizing:content-box;width:80px;height:40px;padding:10px;border:10px solid #000;margin:20px 30px;background:#888;background-clip:padding-box}
.k{position:absolute;left:-30px;top:-10px;width:30px;height:30px;background:#444}
</style></head><body><div class=w id=w0 style="left:5px;top:5px"><div class=t id=t0 style="border-radius:30px;clip-path:border-box"></div></div><div class=w id=w1 style="left:215px;top:5px"><div class=t id=t1 style="border-radius:30px;clip-path:content-box"></div></div><div class=w id=w2 style="left:425px;top:5px"><div class=t id=t2 style="border-radius:30px;margin:20px 30px;clip-path:margin-box"></div></div><div class=w id=w3 style="left:635px;top:5px"><div class=t id=t3 style="transform:rotate(15deg);clip-path:circle(50%)"></div></div><div class=w id=w4 style="left:5px;top:165px"><div class=t id=t4 style="transform:scale(1.5);transform-origin:0 0;clip-path:inset(10px round 10px)"></div></div><div class=w id=w5 style="left:215px;top:165px"><div class=t id=t5 style="opacity:.5;clip-path:circle(50%)"></div></div><div class=w id=w6 style="left:425px;top:165px"><div class=t id=t6 style="border-radius:30px"></div></div><div class=w id=w7 style="left:635px;top:165px"><div class=t id=t7 style="outline:5px solid #000;clip-path:inset(0)"></div></div><div class=w id=w8 style="left:5px;top:325px"><div class=t id=t8 style="outline:5px solid #000;clip-path:inset(-10px)"></div></div><div class=w id=w9 style="left:215px;top:325px"><div class=t id=t9 style="position:absolute;left:30px;top:20px;margin:0;clip-path:circle(50%)"></div></div><div class=w id=w10 style="left:425px;top:325px"><div class=t id=t10 style="position:relative;top:5px;left:5px;clip-path:circle(50%)"></div></div><div class=w id=w11 style="left:635px;top:325px"><div class=t id=t11 style="display:inline-block;clip-path:circle(50%)"></div></div><div class=w id=w12 style="left:5px;top:485px"><div class=t id=t12 style="overflow:hidden;border-radius:20px;clip-path:inset(0 50% 0 0)"></div></div></body></html>"#,
        items: &[
            ([5.0, 5.0, 200.0, 150.0], 5758.22, Some([30, 20, 120, 80])),
            ([215.0, 5.0, 200.0, 150.0], 1448.96, Some([50, 40, 80, 40])),
            ([425.0, 5.0, 200.0, 150.0], 5792.57, Some([30, 20, 120, 80])),
            ([635.0, 5.0, 200.0, 150.0], 4153.53, Some([39, 13, 102, 94])),
            ([5.0, 165.0, 200.0, 150.0], 6207.6, Some([45, 35, 150, 90])),
            (
                [215.0, 165.0, 200.0, 150.0],
                2094.97,
                Some([39, 20, 102, 80]),
            ),
            (
                [425.0, 165.0, 200.0, 150.0],
                5790.77,
                Some([30, 20, 120, 80]),
            ),
            (
                [635.0, 165.0, 200.0, 150.0],
                6400.0,
                Some([30, 20, 120, 80]),
            ),
            ([5.0, 325.0, 200.0, 150.0], 8500.0, Some([25, 15, 130, 90])),
            (
                [215.0, 325.0, 200.0, 150.0],
                4161.6,
                Some([39, 20, 102, 80]),
            ),
            (
                [425.0, 325.0, 200.0, 150.0],
                4162.02,
                Some([44, 25, 102, 80]),
            ),
            (
                [635.0, 325.0, 200.0, 150.0],
                4162.64,
                Some([39, 20, 102, 80]),
            ),
            ([5.0, 485.0, 200.0, 150.0], 3053.09, Some([30, 20, 60, 80])),
        ],
    },
    PaintCase {
        name: "cp-paint-child",
        html: r#"<!DOCTYPE html><html><head><title>t</title><style>
body{margin:0;background:#fff}
.w{position:absolute;width:200px;height:150px}
.t{box-sizing:content-box;width:80px;height:40px;padding:10px;border:10px solid #000;margin:20px 30px;background:#888;background-clip:padding-box}
.k{position:absolute;left:-30px;top:-10px;width:30px;height:30px;background:#444}
</style></head><body><div class=w id=w0 style="left:5px;top:5px"><div class=t id=t0 style="position:relative;clip-path:inset(0)"><div class=k></div><div style='position:absolute;right:-40px;bottom:-40px;width:60px;height:60px;background:#000'></div></div></div><div class=w id=w1 style="left:215px;top:5px"><div class=t id=t1 style="position:relative;clip-path:inset(-10px)"><div class=k></div><div style='position:absolute;right:-40px;bottom:-40px;width:60px;height:60px;background:#000'></div></div></div><div class=w id=w2 style="left:425px;top:5px"><div class=t id=t2 style="position:relative;clip-path:circle(50%)"><div class=k></div><div style='position:absolute;right:-40px;bottom:-40px;width:60px;height:60px;background:#000'></div></div></div><div class=w id=w3 style="left:635px;top:5px"><div class=t id=t3 style="position:relative;clip-path:polygon(0 0, 100% 0, 0 100%)"><div class=k></div><div style='position:absolute;right:-40px;bottom:-40px;width:60px;height:60px;background:#000'></div></div></div><div class=w id=w4 style="left:5px;top:165px"><div class=t id=t4 style="clip-path:inset(0)"><div class=k></div><div style='position:absolute;right:-40px;bottom:-40px;width:60px;height:60px;background:#000'></div></div></div></body></html>"#,
        items: &[
            ([5.0, 5.0, 200.0, 150.0], 6533.33, Some([30, 20, 120, 80])),
            ([215.0, 5.0, 200.0, 150.0], 7453.33, Some([20, 20, 140, 90])),
            ([425.0, 5.0, 200.0, 150.0], 4342.01, Some([39, 20, 102, 82])),
            ([635.0, 5.0, 200.0, 150.0], 3119.46, Some([30, 20, 120, 80])),
            ([5.0, 165.0, 200.0, 150.0], 6400.0, Some([30, 20, 120, 80])),
        ],
    },
    PaintCase {
        name: "cp-paint-contexts-base",
        html: r"<!DOCTYPE html><html><head><title>t</title><style>body{margin:0;font:20px/30px sans-serif}.w{position:absolute;width:300px;height:100px;left:5px}.s{background:#888}</style></head><body><div class=w id=w0 style='top:5px'>ab <span class=s id=s0 style=''>inline span here</span> cd</div><div class=w id=w1 style='top:110px'>ab <span class=s id=s1 style=''>inline span here</span> cd</div><div class=w id=w2 style='top:215px'><table style=''><tr><td class=s style='width:100px;height:40px'>cell</td><td class=s>c2</td></tr></table></div><div class=w id=w3 style='top:320px;display:flex'><div class=s style='width:100px;height:60px;'></div><div class=s style='width:100px;height:60px;'></div></div><div class=w id=w4 style='top:425px'><img id=i4 style='' width=100 height=60 src='data:image/svg+xml,%3Csvg xmlns=%22http://www.w3.org/2000/svg%22 width=%22100%22 height=%2260%22%3E%3Crect width=%22100%22 height=%2260%22/%3E%3C/svg%3E'></div><div class=w id=w5 style='top:530px'><button style='height:50px;width:100px'>btn</button> <input style='' value=value size=10></div></body></html>",
        items: &[
            ([5.0, 5.0, 300.0, 100.0], 1934.2, Some([0, 4, 195, 23])),
            ([5.0, 110.0, 300.0, 100.0], 1934.2, Some([0, 4, 195, 23])),
            ([5.0, 215.0, 300.0, 100.0], 2564.07, Some([2, 2, 127, 42])),
            ([5.0, 320.0, 300.0, 100.0], 5600.0, Some([0, 0, 200, 60])),
            ([5.0, 425.0, 300.0, 100.0], 6000.0, Some([0, 0, 100, 60])),
            ([5.0, 530.0, 300.0, 100.0], 722.02, Some([0, 0, 211, 50])),
        ],
    },
    PaintCase {
        name: "cp-paint-contexts",
        html: r"<!DOCTYPE html><html><head><title>t</title><style>body{margin:0;font:20px/30px sans-serif}.w{position:absolute;width:300px;height:100px;left:5px}.s{background:#888}</style></head><body><div class=w id=w0 style='top:5px'>ab <span class=s id=s0 style='clip-path:inset(0 50% 0 0)'>inline span here</span> cd</div><div class=w id=w1 style='top:110px'>ab <span class=s id=s1 style='clip-path:circle(30px)'>inline span here</span> cd</div><div class=w id=w2 style='top:215px'><table style='clip-path:inset(0 50% 0 0)'><tr><td class=s style='width:100px;height:40px'>cell</td><td class=s>c2</td></tr></table></div><div class=w id=w3 style='top:320px;display:flex'><div class=s style='width:100px;height:60px;clip-path:circle(30px)'></div><div class=s style='width:100px;height:60px;clip-path:inset(10px)'></div></div><div class=w id=w4 style='top:425px'><img id=i4 style='clip-path:circle(30px)' width=100 height=60 src='data:image/svg+xml,%3Csvg xmlns=%22http://www.w3.org/2000/svg%22 width=%22100%22 height=%2260%22%3E%3Crect width=%22100%22 height=%2260%22/%3E%3C/svg%3E'></div><div class=w id=w5 style='top:530px'><button style='clip-path:circle(25px);height:50px;width:100px'>btn</button> <input style='clip-path:inset(0 50% 0 0)' value=value size=10></div></body></html>",
        items: &[
            ([5.0, 5.0, 300.0, 100.0], 1067.61, Some([0, 4, 195, 22])),
            ([5.0, 110.0, 300.0, 100.0], 906.64, Some([0, 4, 195, 23])),
            ([5.0, 215.0, 300.0, 100.0], 1315.91, Some([2, 2, 64, 42])),
            ([5.0, 320.0, 300.0, 100.0], 2806.96, Some([20, 0, 170, 60])),
            ([5.0, 425.0, 300.0, 100.0], 2814.38, Some([20, 0, 60, 60])),
            ([5.0, 530.0, 300.0, 100.0], 321.89, Some([25, 0, 134, 50])),
        ],
    },
    PaintCase {
        name: "cp-svg",
        html: r#"<!DOCTYPE html><html><head><title>t</title><style>body{margin:0}.w{position:absolute}</style></head><body><div class=w id=w0 style="left:5px;top:5px;width:200px;height:100px"><svg id=s0 width=200 height=100><rect x=20 y=10 width=100 height=60 clip-path='circle(50%)'/></svg></div><div class=w id=w1 style="left:215px;top:5px;width:200px;height:100px"><svg id=s1 width=200 height=100><rect x=20 y=10 width=100 height=60 style='clip-path:circle(50%)'/></svg></div><div class=w id=w2 style="left:425px;top:5px;width:200px;height:100px"><svg id=s2 width=200 height=100><rect x=20 y=10 width=100 height=60 clip-path='inset(10px)'/></svg></div><div class=w id=w3 style="left:635px;top:5px;width:200px;height:100px"><svg id=s3 width=200 height=100><rect x=20 y=10 width=100 height=60 style='clip-path:polygon(50% 0,100% 100%,0 100%)'/></svg></div><div class=w id=w4 style="left:5px;top:115px;width:200px;height:100px"><svg id=s4 width=200 height=100><rect x=20 y=10 width=100 height=60 style='clip-path:circle(50%) fill-box'/></svg></div><div class=w id=w5 style="left:215px;top:115px;width:200px;height:100px"><svg id=s5 width=200 height=100><rect x=20 y=10 width=100 height=60 stroke='#000' stroke-width=20 style='clip-path:circle(50%) stroke-box'/></svg></div><div class=w id=w6 style="left:425px;top:115px;width:200px;height:100px"><svg id=s6 width=200 height=100><rect x=20 y=10 width=100 height=60 style='clip-path:circle(50%) view-box'/></svg></div><div class=w id=w7 style="left:635px;top:115px;width:200px;height:100px"><svg id=s7 width=200 height=100><rect x=20 y=10 width=100 height=60 style='clip-path:circle(50%) border-box'/></svg></div><div class=w id=w8 style="left:5px;top:225px;width:200px;height:100px"><svg id=s8 width=200 height=100><rect x=20 y=10 width=100 height=60 style='clip-path:circle(50%) margin-box'/></svg></div><div class=w id=w9 style="left:215px;top:225px;width:200px;height:100px"><svg id=s9 width=200 height=100><rect x=20 y=10 width=100 height=60 style='clip-path:circle(50%) padding-box'/></svg></div><div class=w id=w10 style="left:425px;top:225px;width:200px;height:100px"><svg id=s10 width=200 height=100><rect x=20 y=10 width=100 height=60 style='clip-path:circle(50%) content-box'/></svg></div><div class=w id=w11 style="left:635px;top:225px;width:200px;height:100px"><svg id=s11 width=200 height=100><rect x=20 y=10 width=100 height=60 rx=30 style='clip-path:fill-box'/></svg></div><div class=w id=w12 style="left:5px;top:335px;width:200px;height:100px"><svg id=s12 width=200 height=100><g clip-path='inset(10px)'><rect x=20 y=10 width=60 height=60 /><rect x=100 y=40 width=60 height=60 /></g></svg></div><div class=w id=w13 style="left:215px;top:335px;width:200px;height:100px"><svg id=s13 width=200 height=100><g transform='translate(30 20) scale(1.5)' clip-path='circle(30px)'><rect x=0 y=0 width=60 height=40 /></g></svg></div><div class=w id=w14 style="left:425px;top:335px;width:200px;height:100px"><svg id=s14 width=200 height=100><rect x=20 y=10 width=100 height=60 transform='rotate(20 70 40)' style='clip-path:inset(0 50% 0 0)'/></svg></div><div class=w id=w15 style="left:635px;top:335px;width:200px;height:100px"><svg id=s15 width=200 height=100><rect x=20 y=10 width=100 height=60 style='clip-path:path("M0 0 L40 0 L0 40 z")'/></svg></div><div class=w id=w16 style="left:5px;top:445px;width:200px;height:100px"><svg id=s16 width=200 height=100><rect x=20 y=10 width=100 height=60 style='clip-path:circle(30px at 50px 40px)'/></svg></div><div class=w id=w17 style="left:215px;top:445px;width:200px;height:100px"><svg id=s17 width=200 height=100><rect x=20 y=10 width=100 height=60 style='clip-path:ellipse(25% 50% at 25% 50%)'/></svg></div><div class=w id=w18 style="left:425px;top:445px;width:200px;height:100px"><svg id=s18 width=200 height=100><circle cx=70 cy=40 r=30 style='clip-path:inset(0 0 50% 0)'/></svg></div><div class=w id=w19 style="left:635px;top:445px;width:200px;height:100px"><svg id=s19 width=200 height=100><line x1=10 y1=10 x2=100 y2=60 stroke='#000' stroke-width=10 style='clip-path:inset(0 50% 0 0)'/></svg></div><div class=w id=w20 style="left:5px;top:555px;width:200px;height:100px"><svg id=s20 width=200 height=100><defs><rect id=r x=20 y=10 width=100 height=60 /></defs><use href='#r' style='clip-path:circle(50%)'/></svg></div><div class=w id=w21 style="left:215px;top:555px;width:200px;height:100px"><svg id=s21 width=200 height=100><defs><clipPath id=cc><rect x=0 y=0 width=70 height=40 /></clipPath></defs><rect x=20 y=10 width=100 height=60 clip-path='url(#cc)'/></svg></div></body></html>"#,
        items: &[
            ([5.0, 5.0, 200.0, 100.0], 4452.92, Some([28, 10, 84, 60])),
            ([215.0, 5.0, 200.0, 100.0], 4452.86, Some([28, 10, 84, 60])),
            ([425.0, 5.0, 200.0, 100.0], 3200.0, Some([30, 20, 80, 40])),
            ([635.0, 5.0, 200.0, 100.0], 3000.08, Some([20, 10, 100, 60])),
            ([5.0, 115.0, 200.0, 100.0], 4452.92, Some([28, 10, 84, 60])),
            (
                [215.0, 115.0, 200.0, 100.0],
                7224.65,
                Some([19, 0, 102, 80]),
            ),
            (
                [425.0, 115.0, 200.0, 100.0],
                5782.99,
                Some([20, 10, 100, 60]),
            ),
            (
                [635.0, 115.0, 200.0, 100.0],
                4452.92,
                Some([28, 10, 84, 60]),
            ),
            ([5.0, 225.0, 200.0, 100.0], 4455.53, Some([28, 10, 84, 60])),
            (
                [215.0, 225.0, 200.0, 100.0],
                4455.47,
                Some([28, 10, 84, 60]),
            ),
            (
                [425.0, 225.0, 200.0, 100.0],
                4455.53,
                Some([28, 10, 84, 60]),
            ),
            (
                [635.0, 225.0, 200.0, 100.0],
                5214.33,
                Some([20, 10, 100, 60]),
            ),
            ([5.0, 335.0, 200.0, 100.0], 5000.0, Some([30, 20, 120, 70])),
            (
                [215.0, 335.0, 200.0, 100.0],
                4960.89,
                Some([30, 20, 90, 60]),
            ),
            ([425.0, 335.0, 200.0, 100.0], 2933.71, Some([12, 0, 69, 69])),
            ([635.0, 335.0, 200.0, 100.0], 800.08, Some([20, 10, 40, 40])),
            ([5.0, 445.0, 200.0, 100.0], 2507.39, Some([40, 20, 60, 50])),
            (
                [215.0, 445.0, 200.0, 100.0],
                2347.53,
                Some([20, 10, 50, 60]),
            ),
            (
                [425.0, 445.0, 200.0, 100.0],
                1407.23,
                Some([40, 10, 60, 30]),
            ),
            ([635.0, 445.0, 200.0, 100.0], 510.85, Some([7, 5, 48, 36])),
            ([5.0, 555.0, 200.0, 100.0], 4452.92, Some([28, 10, 84, 60])),
            ([215.0, 555.0, 200.0, 100.0], 1500.0, Some([20, 10, 50, 30])),
        ],
    },
    PaintCase {
        name: "cp-svg-svgroot",
        html: r#"<!DOCTYPE html><html><head><title>t</title><style>body{margin:0}</style></head><body>
<svg id=s0 width=200 height=100 style="position:absolute;left:5px;top:5px;clip-path:circle(50%);background:#888;border:10px solid #000;padding:10px"><rect width=200 height=100 /></svg>
<svg id=s1 width=200 height=100 style="position:absolute;left:250px;top:5px;clip-path:inset(10px) content-box;background:#888;border:10px solid #000;padding:10px"><rect width=200 height=100 /></svg>
<svg id=s2 width=200 height=100 style="position:absolute;left:5px;top:150px;clip-path:polygon(0 0,100% 0,0 100%)"><rect width=200 height=100 /></svg>
<svg id=s3 width=200 height=100 viewBox="0 0 100 50" style="position:absolute;left:250px;top:150px;clip-path:circle(50%) view-box"><rect width=100 height=50 /></svg>
</body></html>"#,
        items: &[
            ([5.0, 5.0, 240.0, 140.0], 23218.7, Some([21, 0, 198, 140])),
            ([250.0, 5.0, 240.0, 140.0], 14400.0, Some([30, 30, 180, 80])),
            ([5.0, 150.0, 200.0, 100.0], 10000.0, Some([0, 0, 200, 100])),
            (
                [250.0, 150.0, 200.0, 100.0],
                14672.97,
                Some([20, 0, 160, 100]),
            ),
        ],
    },
    PaintCase {
        name: "cp-svg-root2",
        html: r#"<!DOCTYPE html><html><head><title>t</title><style>body{margin:0}</style></head><body><svg id=s0 width=200 height=100  style="position:absolute;left:5px;top:5px;clip-path:polygon(0 0,100% 0,0 100%)"><rect width=200 height=100 /></svg><svg id=s1 width=200 height=100  style="position:absolute;left:235px;top:5px;clip-path:circle(50%)"><rect width=200 height=100 /></svg><svg id=s2 width=200 height=100  style="position:absolute;left:465px;top:5px;clip-path:inset(10px)"><rect width=200 height=100 /></svg><svg id=s3 width=200 height=100  style="position:absolute;left:695px;top:5px;"><rect width=200 height=100 /></svg><svg id=s4 width=200 height=100  style="position:absolute;left:5px;top:135px;clip-path:polygon(0 0,100% 0,0 100%);background:#888"><rect width=200 height=100 /></svg><svg id=s5 width=200 height=100  style="position:absolute;left:235px;top:135px;clip-path:polygon(0 0,100% 0,0 100%);display:block"><rect width=200 height=100 /></svg><svg id=s6 width=200 height=100  style="position:absolute;left:465px;top:135px;clip-path:polygon(0 0,100% 0,0 100%);border:1px solid #000"><rect width=200 height=100 /></svg><svg id=s7 width=200 height=100 viewBox='0 0 100 50' style="position:absolute;left:695px;top:135px;clip-path:polygon(0 0,100% 0,0 100%)"><rect width=200 height=100 /></svg><svg id=s8 width=200 height=100  style="position:absolute;left:5px;top:265px;clip-path:circle(50%) fill-box"><rect width=200 height=100 /></svg><svg id=s9 width=200 height=100  style="position:absolute;left:235px;top:265px;clip-path:circle(50%) view-box"><rect width=200 height=100 /></svg><svg id=s10 width=200 height=100  style="position:absolute;left:465px;top:265px;clip-path:circle(50%) content-box"><rect width=200 height=100 /></svg><svg id=s11 width=200 height=100  style="position:absolute;left:695px;top:265px;clip-path:circle(50%) margin-box;margin:10px"><rect width=200 height=100 /></svg></body></html>"#,
        items: &[
            ([5.0, 5.0, 200.0, 100.0], 10000.0, Some([0, 0, 200, 100])),
            (
                [235.0, 5.0, 200.0, 100.0],
                14672.29,
                Some([20, 0, 160, 100]),
            ),
            ([465.0, 5.0, 200.0, 100.0], 14400.0, Some([10, 10, 180, 80])),
            ([695.0, 5.0, 200.0, 100.0], 20000.0, Some([0, 0, 200, 100])),
            ([5.0, 135.0, 200.0, 100.0], 10017.65, Some([0, 0, 200, 100])),
            (
                [235.0, 135.0, 200.0, 100.0],
                10000.0,
                Some([0, 0, 200, 100]),
            ),
            (
                [465.0, 135.0, 202.0, 102.0],
                10298.42,
                Some([0, 0, 202, 102]),
            ),
            (
                [695.0, 135.0, 200.0, 100.0],
                10000.0,
                Some([0, 0, 200, 100]),
            ),
            (
                [5.0, 265.0, 200.0, 100.0],
                14672.97,
                Some([20, 0, 160, 100]),
            ),
            (
                [235.0, 265.0, 200.0, 100.0],
                14672.97,
                Some([20, 0, 160, 100]),
            ),
            (
                [465.0, 265.0, 200.0, 100.0],
                14672.97,
                Some([20, 0, 160, 100]),
            ),
            (
                [705.0, 275.0, 200.0, 100.0],
                16716.78,
                Some([11, 0, 178, 100]),
            ),
        ],
    },
    PaintCase {
        name: "cp-margin",
        html: r#"<!DOCTYPE html><html><head><title>t</title><style>
body{margin:0}
.b{position:absolute;width:100px;height:60px;background:#000;clip-path:circle(25%) margin-box}
</style></head><body>
<div id=m0 class=b style="left:20px;top:20px;margin:10px"></div>
<div id=m1 class=b style="left:240px;top:20px;margin:0 20px 40px 0"></div>
<div id=m2 class=b style="left:460px;top:20px;margin:30px 0 0 50px;clip-path:circle(25%) margin-box;border-radius:20px"></div>
<div id=m3 class=b style="left:20px;top:200px;margin:10px;clip-path:inset(0) margin-box"></div>
<div id=m4 class=b style="left:240px;top:200px;margin:10px 40px;clip-path:inset(0 round 10px) margin-box"></div>
<svg id=m5 class=b width=100 height=60 style="left:460px;top:200px;margin:10px 30px 0 0;background:none"><rect width=100 height=60 /></svg>
<svg id=m6 width=100 height=60 style="position:absolute;left:20px;top:380px;margin:10px 30px 0 0;clip-path:circle(25%) margin-box"><rect width=100 height=60 /></svg>
<svg id=m7 width=100 height=60 style="position:absolute;left:240px;top:380px;margin:10px 30px 0 0;clip-path:circle(25%)"><rect width=100 height=60 /></svg>
</body></html>"#,
        items: &[
            ([30.0, 30.0, 100.0, 60.0], 2026.94, Some([24, 4, 52, 52])),
            ([240.0, 20.0, 100.0, 60.0], 1726.42, Some([32, 22, 56, 38])),
            ([510.0, 50.0, 100.0, 60.0], 2149.8, Some([0, 0, 56, 46])),
            ([30.0, 210.0, 100.0, 60.0], 6000.0, Some([0, 0, 100, 60])),
            ([280.0, 210.0, 100.0, 60.0], 6000.0, Some([0, 0, 100, 60])),
            ([460.0, 210.0, 100.0, 60.0], 2113.55, Some([38, 0, 54, 51])),
            ([20.0, 390.0, 100.0, 60.0], 2113.8, Some([38, 0, 54, 51])),
            ([240.0, 390.0, 100.0, 60.0], 1323.37, Some([29, 9, 42, 42])),
        ],
    },
    PaintCase {
        name: "cp-svg-stroke",
        html: r##"<!DOCTYPE html><html><head><title>t</title><style>body{margin:0}</style></head><body>
<svg id=s0 width=200 height=100 style="position:absolute;left:5px;top:5px"><rect x=30 y=20 width=100 height=60 stroke="#000" stroke-width=20 fill="#888" style="clip-path:circle(50%)" /></svg>
<svg id=s1 width=200 height=100 style="position:absolute;left:250px;top:5px"><rect x=30 y=20 width=100 height=60 stroke="#000" stroke-width=20 fill="#888" style="clip-path:inset(5px)" /></svg>
<svg id=s2 width=200 height=100 style="position:absolute;left:5px;top:150px"><g stroke="#000" stroke-width=20 fill="#888" style="clip-path:inset(5px)"><rect x=30 y=20 width=100 height=60 /></g></svg>
<svg id=s3 width=200 height=100 style="position:absolute;left:250px;top:150px"><rect x=30 y=20 width=100 height=60 stroke="#000" stroke-width=20 fill="#888" style="clip-path:inset(5px) fill-box" /></svg>
<svg id=s4 width=200 height=100 style="position:absolute;left:5px;top:300px"><line x1=10 y1=10 x2=100 y2=60 stroke="#000" stroke-width=10 style="clip-path:inset(0 50% 0 0) stroke-box" /></svg>
<svg id=s5 width=200 height=100 style="position:absolute;left:250px;top:300px"><line x1=10 y1=10 x2=100 y2=60 stroke="#000" stroke-width=10 style="clip-path:inset(0 50% 0 0) fill-box" /></svg>
<svg id=s6 width=200 height=100 style="position:absolute;left:5px;top:450px"><rect x=30 y=20 width=100 height=60 stroke="#000" stroke-width=20 fill="#888" stroke-linejoin="miter" transform="rotate(30 80 50)" style="clip-path:inset(5px)" /></svg>
</body></html>"##,
        items: &[
            ([5.0, 5.0, 200.0, 100.0], 5509.77, Some([29, 10, 102, 80])),
            ([250.0, 5.0, 200.0, 100.0], 5993.33, Some([25, 15, 110, 70])),
            ([5.0, 150.0, 200.0, 100.0], 5993.33, Some([25, 15, 110, 70])),
            (
                [250.0, 150.0, 200.0, 100.0],
                2793.33,
                Some([35, 25, 90, 50]),
            ),
            ([5.0, 300.0, 200.0, 100.0], 510.85, Some([7, 5, 48, 36])),
            ([250.0, 300.0, 200.0, 100.0], 487.16, Some([10, 10, 45, 31])),
            ([5.0, 450.0, 200.0, 100.0], 5855.45, Some([15, 0, 130, 100])),
        ],
    },
    PaintCase {
        name: "cp-stack2",
        html: r#"<!DOCTYPE html><html><head><title>t</title><style>
body{margin:0}
.p{position:absolute;left:10px;width:200px;height:100px;background:#ccc}
.lo{position:relative;z-index:-1;width:50px;height:50px;background:#000}
.hi{position:relative;z-index:1;width:50px;height:50px;background:#000}
</style></head><body>
<div class=p id=p1 style="top:10px;clip-path:inset(0)"><div class=lo></div></div>
<div class=p id=p2 style="top:130px"><div class=lo></div></div>
<div class=p id=p3 style="top:250px;opacity:.99"><div class=lo></div></div>
<div class=p id=p4 style="top:370px;clip-path:inset(0)"><div class=hi></div></div>
<div class=p id=p5 style="top:490px;clip-path:circle(50%)"><div class=lo></div></div>
<div class=p id=p6 style="top:610px;z-index:0"><div class=lo></div></div>
</body></html>"#,
        items: &[
            ([10.0, 10.0, 200.0, 100.0], 6000.0, Some([0, 0, 200, 100])),
            ([10.0, 130.0, 200.0, 100.0], 4000.0, Some([0, 0, 200, 100])),
            ([10.0, 250.0, 200.0, 100.0], 5980.39, Some([0, 0, 200, 100])),
            ([10.0, 370.0, 200.0, 100.0], 6000.0, Some([0, 0, 200, 100])),
            ([10.0, 490.0, 200.0, 100.0], 3871.4, Some([20, 0, 160, 100])),
            ([10.0, 610.0, 200.0, 100.0], 6000.0, Some([0, 0, 200, 100])),
        ],
    },
];
