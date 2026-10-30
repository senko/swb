// The cases of `tools/probes/web-geometry-matrix.json` with the values that Chromium 148
// gave (`just probe --json tools/probes/web-geometry-matrix.json`, three runs, all equal);
// where swb departs on purpose, the test overrides the value. Generated.

use crate::common::Case;

/// Page 1 of the cases.
pub(crate) const PAGE_1: &str = r"<!DOCTYPE html><html><head><title>t</title></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(x) { return Object.is(x, -0) ? '0' : String(Math.round(x * 1e6) / 1e6); }
function Z(x) { return Object.is(x, -0) ? '-0' : String(Math.round(x * 1e6) / 1e6); }
function F(m) { return (m.is2D ? '2D ' : '3D ') + (m.isIdentity ? 'I ' : '') + Array.from(m.toFloat64Array()).map(N).join(','); }
function S(list, f) { return list.map(v => { try { return JSON.stringify(v) + ' => ' + f(v); } catch (e) { return JSON.stringify(v) + ' => ' + (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }); }
</script></body></html>";

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "interfaces",
        html: PAGE_1,
        queries: &[
            (
                r"E(() => [typeof DOMMatrix, typeof DOMMatrixReadOnly, typeof WebKitCSSMatrix, WebKitCSSMatrix === DOMMatrix, typeof DOMPoint, typeof DOMPointReadOnly, typeof DOMMatrix2DInit, typeof DOMMatrixInit, DOMMatrix.length, DOMMatrix.name, DOMMatrixReadOnly.length, DOMPoint.length, DOMPointReadOnly.length, Object.getPrototypeOf(DOMMatrix) === DOMMatrixReadOnly, Object.getPrototypeOf(DOMPoint) === DOMPointReadOnly, JSON.stringify(Object.getOwnPropertyDescriptor(window, 'WebKitCSSMatrix'), (k, v) => typeof v === 'function' ? 'fn' : v), JSON.stringify(Object.getOwnPropertyDescriptor(window, 'DOMMatrix'), (k, v) => typeof v === 'function' ? 'fn' : v), Object.prototype.toString.call(new DOMMatrix()), Object.prototype.toString.call(new DOMMatrixReadOnly()), Object.prototype.toString.call(new DOMPoint()), 'm11' in new WebKitCSSMatrix(), 'WebKitCSSMatrix' in window])",
                r#"["function", "function", "function", true, "function", "function", "undefined", "undefined", 0, "DOMMatrix", 0, 0, 0, true, true, "{\"value\":\"fn\",\"writable\":true,\"enumerable\":false,\"configurable\":true}", "{\"value\":\"fn\",\"writable\":true,\"enumerable\":false,\"configurable\":true}", "[object DOMMatrix]", "[object DOMMatrixReadOnly]", "[object DOMPoint]", true, true]"#,
            ),
            (
                r"E(() => [Object.getOwnPropertyNames(DOMMatrixReadOnly.prototype).join(), Object.getOwnPropertyNames(DOMMatrix.prototype).join(), Object.getOwnPropertyNames(DOMMatrixReadOnly).join(), Object.getOwnPropertyNames(DOMMatrix).join(), Object.getOwnPropertyNames(DOMPointReadOnly.prototype).join(), Object.getOwnPropertyNames(DOMPoint.prototype).join(), Object.getOwnPropertyNames(DOMPointReadOnly).join(), Object.getOwnPropertyNames(DOMPoint).join()])",
                r#"["a,b,c,d,e,f,m11,m12,m13,m14,m21,m22,m23,m24,m31,m32,m33,m34,m41,m42,m43,m44,is2D,isIdentity,flipX,flipY,inverse,multiply,rotate,rotateAxisAngle,rotateFromVector,scale,scale3d,scaleNonUniform,skewX,skewY,toFloat32Array,toFloat64Array,toJSON,transformPoint,translate,constructor,toString", "a,b,c,d,e,f,m11,m12,m13,m14,m21,m22,m23,m24,m31,m32,m33,m34,m41,m42,m43,m44,invertSelf,multiplySelf,preMultiplySelf,rotateAxisAngleSelf,rotateFromVectorSelf,rotateSelf,scale3dSelf,scaleSelf,skewXSelf,skewYSelf,translateSelf,constructor,setMatrixValue", "length,name,prototype,fromFloat32Array,fromFloat64Array,fromMatrix", "length,name,prototype,fromFloat32Array,fromFloat64Array,fromMatrix", "x,y,z,w,matrixTransform,toJSON,constructor", "x,y,z,w,constructor", "length,name,prototype,fromPoint", "length,name,prototype,fromPoint"]"#,
            ),
            (
                r"E(() => { const d = (o, n) => { const x = Object.getOwnPropertyDescriptor(o, n); return x ? [typeof x.get, typeof x.set, x.enumerable, x.configurable, x.get && x.get.name, x.set && x.set.name, x.value && typeof x.value].join('/') : 'none'; }; return [d(DOMMatrixReadOnly.prototype, 'a'), d(DOMMatrix.prototype, 'a'), d(DOMMatrixReadOnly.prototype, 'is2D'), d(DOMMatrix.prototype, 'is2D'), d(DOMMatrixReadOnly.prototype, 'translate'), d(DOMMatrix.prototype, 'translateSelf'), d(DOMMatrixReadOnly.prototype, 'toJSON'), d(DOMMatrixReadOnly.prototype, 'toString'), d(DOMMatrix.prototype, 'toString'), d(DOMMatrixReadOnly.prototype, 'm11'), d(DOMMatrix.prototype, 'm44'), d(DOMPointReadOnly.prototype, 'x'), d(DOMPoint.prototype, 'x'), d(DOMPoint.prototype, 'w')]; })",
                r#"["function/undefined/true/true/get a//", "function/function/true/true/get a/set a/", "function/undefined/true/true/get is2D//", "none", "undefined/undefined/true/true///function", "undefined/undefined/true/true///function", "undefined/undefined/true/true///function", "undefined/undefined/true/true///function", "none", "function/undefined/true/true/get m11//", "function/function/true/true/get m44/set m44/", "function/undefined/true/true/get x//", "function/function/true/true/get x/set x/", "function/function/true/true/get w/set w/"]"#,
            ),
            (
                r"E(() => ['translate', 'scale', 'scaleNonUniform', 'scale3d', 'rotate', 'rotateFromVector', 'rotateAxisAngle', 'skewX', 'skewY', 'multiply', 'flipX', 'flipY', 'inverse', 'transformPoint', 'toFloat32Array', 'toFloat64Array', 'toJSON', 'toString'].map(n => n + ':' + DOMMatrixReadOnly.prototype[n].length).join(' ') + ' | ' + ['multiplySelf', 'preMultiplySelf', 'translateSelf', 'scaleSelf', 'scale3dSelf', 'rotateSelf', 'rotateFromVectorSelf', 'rotateAxisAngleSelf', 'skewXSelf', 'skewYSelf', 'invertSelf', 'setMatrixValue'].map(n => n + ':' + DOMMatrix.prototype[n].length).join(' ') + ' | ' + ['fromMatrix', 'fromFloat32Array', 'fromFloat64Array'].map(n => n + ':' + DOMMatrixReadOnly[n].length + DOMMatrix[n].length).join(' ') + ' | ' + ['fromPoint'].map(n => n + ':' + DOMPoint[n].length).join(' ') + ' ' + DOMPointReadOnly.prototype.matrixTransform.length + ' ' + DOMPointReadOnly.prototype.toJSON.length)",
                r#""translate:0 scale:0 scaleNonUniform:0 scale3d:0 rotate:0 rotateFromVector:0 rotateAxisAngle:0 skewX:0 skewY:0 multiply:0 flipX:0 flipY:0 inverse:0 transformPoint:0 toFloat32Array:0 toFloat64Array:0 toJSON:0 toString:0 | multiplySelf:0 preMultiplySelf:0 translateSelf:0 scaleSelf:0 scale3dSelf:0 rotateSelf:0 rotateFromVectorSelf:0 rotateAxisAngleSelf:0 skewXSelf:0 skewYSelf:0 invertSelf:0 setMatrixValue:1 | fromMatrix:00 fromFloat32Array:11 fromFloat64Array:11 | fromPoint:0 0 0""#,
            ),
        ],
    },
    Case {
        name: "strings-1",
        html: PAGE_1,
        queries: &[
            (
                r#"E(() => S([null, "", " ", "none", "matrix(1,2,3,4,5,6)", "translate(10px, 20px)", "translate(10px)", "translateX(5px)", "translateY(5px)", "translateZ(5px)", "translate3d(1px, 2px, 3px)", "translate3d(1px, 2px, 0px)"], v => F(new DOMMatrix(v))))"#,
                r#"["null => DOMException SyntaxError: Failed to construct 'DOMMatrix': Failed to parse 'null'.", "\"\" => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "\" \" => DOMException SyntaxError: Failed to construct 'DOMMatrix': Failed to parse ' '.", "\"none\" => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "\"matrix(1,2,3,4,5,6)\" => 2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "\"translate(10px, 20px)\" => 2D 1,0,0,0,0,1,0,0,0,0,1,0,10,20,0,1", "\"translate(10px)\" => 2D 1,0,0,0,0,1,0,0,0,0,1,0,10,0,0,1", "\"translateX(5px)\" => 2D 1,0,0,0,0,1,0,0,0,0,1,0,5,0,0,1", "\"translateY(5px)\" => 2D 1,0,0,0,0,1,0,0,0,0,1,0,0,5,0,1", "\"translateZ(5px)\" => 3D 1,0,0,0,0,1,0,0,0,0,1,0,0,0,5,1", "\"translate3d(1px, 2px, 3px)\" => 3D 1,0,0,0,0,1,0,0,0,0,1,0,1,2,3,1", "\"translate3d(1px, 2px, 0px)\" => 3D 1,0,0,0,0,1,0,0,0,0,1,0,1,2,0,1"]"#,
            ),
            (
                r#"E(() => S([null, "", " ", "none", "matrix(1,2,3,4,5,6)", "translate(10px, 20px)", "translate(10px)", "translateX(5px)", "translateY(5px)", "translateZ(5px)", "translate3d(1px, 2px, 3px)", "translate3d(1px, 2px, 0px)"], v => F(new DOMMatrixReadOnly(v))))"#,
                r#"["null => DOMException SyntaxError: Failed to construct 'DOMMatrixReadOnly': Failed to parse 'null'.", "\"\" => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "\" \" => DOMException SyntaxError: Failed to construct 'DOMMatrixReadOnly': Failed to parse ' '.", "\"none\" => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "\"matrix(1,2,3,4,5,6)\" => 2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "\"translate(10px, 20px)\" => 2D 1,0,0,0,0,1,0,0,0,0,1,0,10,20,0,1", "\"translate(10px)\" => 2D 1,0,0,0,0,1,0,0,0,0,1,0,10,0,0,1", "\"translateX(5px)\" => 2D 1,0,0,0,0,1,0,0,0,0,1,0,5,0,0,1", "\"translateY(5px)\" => 2D 1,0,0,0,0,1,0,0,0,0,1,0,0,5,0,1", "\"translateZ(5px)\" => 3D 1,0,0,0,0,1,0,0,0,0,1,0,0,0,5,1", "\"translate3d(1px, 2px, 3px)\" => 3D 1,0,0,0,0,1,0,0,0,0,1,0,1,2,3,1", "\"translate3d(1px, 2px, 0px)\" => 3D 1,0,0,0,0,1,0,0,0,0,1,0,1,2,0,1"]"#,
            ),
        ],
    },
    Case {
        name: "strings-2",
        html: PAGE_1,
        queries: &[(
            r#"E(() => S(["scale(2)", "scale(2, 3)", "scaleX(2)", "scaleY(3)", "scaleZ(4)", "scale3d(1, 2, 3)", "scale3d(1, 2, 1)", "rotate(90deg)", "rotateZ(90deg)", "rotateX(90deg)", "rotateY(90deg)", "rotate3d(0, 0, 1, 90deg)"], v => F(new DOMMatrix(v))))"#,
            r#"["\"scale(2)\" => 2D 2,0,0,0,0,2,0,0,0,0,1,0,0,0,0,1", "\"scale(2, 3)\" => 2D 2,0,0,0,0,3,0,0,0,0,1,0,0,0,0,1", "\"scaleX(2)\" => 2D 2,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "\"scaleY(3)\" => 2D 1,0,0,0,0,3,0,0,0,0,1,0,0,0,0,1", "\"scaleZ(4)\" => 3D 1,0,0,0,0,1,0,0,0,0,4,0,0,0,0,1", "\"scale3d(1, 2, 3)\" => 3D 1,0,0,0,0,2,0,0,0,0,3,0,0,0,0,1", "\"scale3d(1, 2, 1)\" => 3D 1,0,0,0,0,2,0,0,0,0,1,0,0,0,0,1", "\"rotate(90deg)\" => 2D 0,1,0,0,-1,0,0,0,0,0,1,0,0,0,0,1", "\"rotateZ(90deg)\" => 2D 0,1,0,0,-1,0,0,0,0,0,1,0,0,0,0,1", "\"rotateX(90deg)\" => 3D 1,0,0,0,0,0,1,0,0,-1,0,0,0,0,0,1", "\"rotateY(90deg)\" => 3D 0,0,-1,0,0,1,0,0,1,0,0,0,0,0,0,1", "\"rotate3d(0, 0, 1, 90deg)\" => 3D 0,1,0,0,-1,0,0,0,0,0,1,0,0,0,0,1"]"#,
        )],
    },
    Case {
        name: "strings-3",
        html: PAGE_1,
        queries: &[(
            r#"E(() => S(["rotate3d(1, 0, 0, 90deg)", "rotate3d(0, 0, 0, 90deg)", "skew(10deg)", "skew(10deg, 20deg)", "skewX(10deg)", "skewY(10deg)", "perspective(100px)", "perspective(0)", "matrix3d(1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1)", "matrix3d(1,0,0,0,0,1,0,0,0,0,1,0,5,6,0,1)", "translate(1px) scale(2)", "scale(2) translate(1px)"], v => F(new DOMMatrix(v))))"#,
            r#"["\"rotate3d(1, 0, 0, 90deg)\" => 3D 1,0,0,0,0,0,1,0,0,-1,0,0,0,0,0,1", "\"rotate3d(0, 0, 0, 90deg)\" => 3D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "\"skew(10deg)\" => 2D 1,0,0,0,0.176327,1,0,0,0,0,1,0,0,0,0,1", "\"skew(10deg, 20deg)\" => 2D 1,0.36397,0,0,0.176327,1,0,0,0,0,1,0,0,0,0,1", "\"skewX(10deg)\" => 2D 1,0,0,0,0.176327,1,0,0,0,0,1,0,0,0,0,1", "\"skewY(10deg)\" => 2D 1,0.176327,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "\"perspective(100px)\" => 3D 1,0,0,0,0,1,0,0,0,0,1,-0.01,0,0,0,1", "\"perspective(0)\" => 3D 1,0,0,0,0,1,0,0,0,0,1,-1,0,0,0,1", "\"matrix3d(1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1)\" => 3D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "\"matrix3d(1,0,0,0,0,1,0,0,0,0,1,0,5,6,0,1)\" => 3D 1,0,0,0,0,1,0,0,0,0,1,0,5,6,0,1", "\"translate(1px) scale(2)\" => 2D 2,0,0,0,0,2,0,0,0,0,1,0,1,0,0,1", "\"scale(2) translate(1px)\" => 2D 2,0,0,0,0,2,0,0,0,0,1,0,2,0,0,1"]"#,
        )],
    },
    Case {
        name: "strings-4",
        html: PAGE_1,
        queries: &[(
            r#"E(() => S(["translate(1px) translateZ(0)", "rotate(1rad)", "rotate(1turn)", "rotate(100grad)", "rotate(0)", "rotate(30)", "translate(1cm, 1mm)", "translate(1in, 1pt)", "translate(1pc, 4Q)", "translate(0)", "translate(1em)", "translate(10%)"], v => F(new DOMMatrix(v))))"#,
            r#"["\"translate(1px) translateZ(0)\" => 3D 1,0,0,0,0,1,0,0,0,0,1,0,1,0,0,1", "\"rotate(1rad)\" => 2D 0.540302,0.841471,0,0,-0.841471,0.540302,0,0,0,0,1,0,0,0,0,1", "\"rotate(1turn)\" => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "\"rotate(100grad)\" => 2D 0,1,0,0,-1,0,0,0,0,0,1,0,0,0,0,1", "\"rotate(0)\" => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "\"rotate(30)\" => DOMException SyntaxError: Failed to construct 'DOMMatrix': Failed to parse 'rotate(30)'.", "\"translate(1cm, 1mm)\" => 2D 1,0,0,0,0,1,0,0,0,0,1,0,37.795277,3.779528,0,1", "\"translate(1in, 1pt)\" => 2D 1,0,0,0,0,1,0,0,0,0,1,0,96,1.333333,0,1", "\"translate(1pc, 4Q)\" => 2D 1,0,0,0,0,1,0,0,0,0,1,0,16,3.779528,0,1", "\"translate(0)\" => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "\"translate(1em)\" => DOMException SyntaxError: Failed to construct 'DOMMatrix': Values must be resolvable at parse time", "\"translate(10%)\" => DOMException SyntaxError: Failed to construct 'DOMMatrix': Values must be resolvable at parse time"]"#,
        )],
    },
    Case {
        name: "strings-5",
        html: PAGE_1,
        queries: &[(
            r#"E(() => S(["translate(1vw)", "translate(1px,)", "translate(1px 2px)", "translate", "translate()", "foo", "matrix(1,2,3)", "matrix(1,2,3,4,5,6,7)", "MATRIX(1,2,3,4,5,6)", "matrix( 1 , 2 , 3 , 4 , 5 , 6 )", "matrix(1 2 3 4 5 6)", "translate(1px, 2px) foo"], v => F(new DOMMatrix(v))))"#,
            r#"["\"translate(1vw)\" => DOMException SyntaxError: Failed to construct 'DOMMatrix': Values must be resolvable at parse time", "\"translate(1px,)\" => DOMException SyntaxError: Failed to construct 'DOMMatrix': Failed to parse 'translate(1px,)'.", "\"translate(1px 2px)\" => DOMException SyntaxError: Failed to construct 'DOMMatrix': Failed to parse 'translate(1px 2px)'.", "\"translate\" => DOMException SyntaxError: Failed to construct 'DOMMatrix': Failed to parse 'translate'.", "\"translate()\" => DOMException SyntaxError: Failed to construct 'DOMMatrix': Failed to parse 'translate()'.", "\"foo\" => DOMException SyntaxError: Failed to construct 'DOMMatrix': Failed to parse 'foo'.", "\"matrix(1,2,3)\" => DOMException SyntaxError: Failed to construct 'DOMMatrix': Failed to parse 'matrix(1,2,3)'.", "\"matrix(1,2,3,4,5,6,7)\" => DOMException SyntaxError: Failed to construct 'DOMMatrix': Failed to parse 'matrix(1,2,3,4,5,6,7)'.", "\"MATRIX(1,2,3,4,5,6)\" => 2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "\"matrix( 1 , 2 , 3 , 4 , 5 , 6 )\" => 2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "\"matrix(1 2 3 4 5 6)\" => DOMException SyntaxError: Failed to construct 'DOMMatrix': Failed to parse 'matrix(1 2 3 4 5 6)'.", "\"translate(1px, 2px) foo\" => DOMException SyntaxError: Failed to construct 'DOMMatrix': Failed to parse 'translate(1px, 2px) foo'."]"#,
        )],
    },
    Case {
        name: "strings-6",
        html: PAGE_1,
        queries: &[(
            r#"E(() => S(["scale(50%)", "scale(1e2)", "scale(.5)", "rotate(90deg", "translate(10px) ; scale(2)", "matrix(1,0,0,1,0,0) none", "initial", "inherit", "var(--x)", "1", "translateX(1px) rotateX(10deg) translateY(2px)", "  translate(1px)  "], v => F(new DOMMatrix(v))))"#,
            r#"["\"scale(50%)\" => 2D 0.5,0,0,0,0,0.5,0,0,0,0,1,0,0,0,0,1", "\"scale(1e2)\" => 2D 100,0,0,0,0,100,0,0,0,0,1,0,0,0,0,1", "\"scale(.5)\" => 2D 0.5,0,0,0,0,0.5,0,0,0,0,1,0,0,0,0,1", "\"rotate(90deg\" => 2D 0,1,0,0,-1,0,0,0,0,0,1,0,0,0,0,1", "\"translate(10px) ; scale(2)\" => DOMException SyntaxError: Failed to construct 'DOMMatrix': Failed to parse 'translate(10px) ; scale(2)'.", "\"matrix(1,0,0,1,0,0) none\" => DOMException SyntaxError: Failed to construct 'DOMMatrix': Failed to parse 'matrix(1,0,0,1,0,0) none'.", "\"initial\" => DOMException SyntaxError: Failed to construct 'DOMMatrix': Failed to parse 'initial'.", "\"inherit\" => DOMException SyntaxError: Failed to construct 'DOMMatrix': Failed to parse 'inherit'.", "\"var(--x)\" => DOMException SyntaxError: Failed to construct 'DOMMatrix': Failed to parse 'var(--x)'.", "\"1\" => DOMException SyntaxError: Failed to construct 'DOMMatrix': Failed to parse '1'.", "\"translateX(1px) rotateX(10deg) translateY(2px)\" => 3D 1,0,0,0,0,0.984808,0.173648,0,0,-0.173648,0.984808,0,1,1.969616,0.347296,1", "\"  translate(1px)  \" => 2D 1,0,0,0,0,1,0,0,0,0,1,0,1,0,0,1"]"#,
        )],
    },
    Case {
        name: "strings-7",
        html: PAGE_1,
        queries: &[(
            r#"E(() => S(["translate(1px)scale(2)", "rotateY(45deg) rotateX(30deg)", "scale3d(2, 2, 2) translate3d(1px, 1px, 1px)", "skew(45deg, 45deg) rotate(45deg)", "perspective(50px) rotateY(45deg)", "matrix3d(1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16)"], v => F(new DOMMatrix(v))))"#,
            r#"["\"translate(1px)scale(2)\" => 2D 2,0,0,0,0,2,0,0,0,0,1,0,1,0,0,1", "\"rotateY(45deg) rotateX(30deg)\" => 3D 0.707107,0,-0.707107,0,0.353553,0.866025,0.353553,0,0.612372,-0.5,0.612372,0,0,0,0,1", "\"scale3d(2, 2, 2) translate3d(1px, 1px, 1px)\" => 3D 2,0,0,0,0,2,0,0,0,0,2,0,2,2,2,1", "\"skew(45deg, 45deg) rotate(45deg)\" => 2D 1.414214,1.414214,0,0,0,0,0,0,0,0,1,0,0,0,0,1", "\"perspective(50px) rotateY(45deg)\" => 3D 0.707107,0,-0.707107,0.014142,0,1,0,0,0.707107,0,0.707107,-0.014142,0,0,0,1", "\"matrix3d(1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16)\" => 3D 1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16"]"#,
        )],
    },
    Case {
        name: "sequences",
        html: PAGE_1,
        queries: &[
            (
                r#"E(() => S([[], [1], [1, 2, 3, 4, 5], [1, 2, 3, 4, 5, 6], [1, 2, 3, 4, 5, 6, 7], [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15], [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16], [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17], ["1", "2", "3", "4", "5", "6"], [null, true, "x", 0, 1.5, -2]], v => F(new DOMMatrix(v))))"#,
                r#"["[] => TypeError: Failed to construct 'DOMMatrix': The sequence must contain 6 elements for a 2D matrix or 16 elements for a 3D matrix.", "[1] => TypeError: Failed to construct 'DOMMatrix': The sequence must contain 6 elements for a 2D matrix or 16 elements for a 3D matrix.", "[1,2,3,4,5] => TypeError: Failed to construct 'DOMMatrix': The sequence must contain 6 elements for a 2D matrix or 16 elements for a 3D matrix.", "[1,2,3,4,5,6] => 2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "[1,2,3,4,5,6,7] => TypeError: Failed to construct 'DOMMatrix': The sequence must contain 6 elements for a 2D matrix or 16 elements for a 3D matrix.", "[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15] => TypeError: Failed to construct 'DOMMatrix': The sequence must contain 6 elements for a 2D matrix or 16 elements for a 3D matrix.", "[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16] => 3D 1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16", "[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17] => TypeError: Failed to construct 'DOMMatrix': The sequence must contain 6 elements for a 2D matrix or 16 elements for a 3D matrix.", "[\"1\",\"2\",\"3\",\"4\",\"5\",\"6\"] => 2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "[null,true,\"x\",0,1.5,-2] => 2D 0,1,0,0,NaN,0,0,0,0,0,1,0,1.5,-2,0,1"]"#,
            ),
            (
                r"E(() => S([[NaN, Infinity, -Infinity, 0, -0, 1e300]], v => F(new DOMMatrix(v))))",
                r#"["[null,null,null,0,0,1e+300] => 2D NaN,Infinity,0,0,-Infinity,0,0,0,0,0,1,0,0,1e+300,0,1"]"#,
            ),
            (
                r"E(() => [new Float32Array([1,2,3,4,5,6]), new Float64Array([1,2,3,4,5,6]), new Float32Array(16), new Uint8Array(6), new Set([1,2,3,4,5,6]), {length: 6, 0: 1}, 5, true, {}, [undefined, 1, 2, 3, 4, 5], new DOMMatrix([1,2,3,4,5,6]), new DOMMatrixReadOnly([1,0,0,1,5,5]), new DOMPoint(1,2,3,4)].map(v => { try { return F(new DOMMatrix(v)); } catch (e) { return e.constructor.name + ': ' + e.message; } }))",
                r#"["2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "3D 0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0", "2D 0,0,0,0,0,0,0,0,0,0,1,0,0,0,0,1", "2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "DOMException: Failed to construct 'DOMMatrix': Failed to parse '[object Object]'.", "DOMException: Failed to construct 'DOMMatrix': Failed to parse '5'.", "DOMException: Failed to construct 'DOMMatrix': Failed to parse 'true'.", "DOMException: Failed to construct 'DOMMatrix': Failed to parse '[object Object]'.", "2D NaN,1,0,0,2,3,0,0,0,0,1,0,4,5,0,1", "2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "2D 1,0,0,0,0,1,0,0,0,0,1,0,5,5,0,1", "DOMException: Failed to construct 'DOMMatrix': Failed to parse '[object DOMPoint]'."]"#,
            ),
            (
                r"E(() => [DOMMatrix(), DOMMatrixReadOnly()].length)",
                r#""TypeError: Failed to construct 'DOMMatrix': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => { try { DOMMatrix(); } catch (e) { return e.message; } })",
                r#""Failed to construct 'DOMMatrix': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => { try { DOMPoint(); } catch (e) { return e.message; } })",
                r#""Failed to construct 'DOMPoint': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => { try { new DOMMatrixReadOnly('foo'); } catch (e) { return e.name + ': ' + e.message; } })",
                r#""SyntaxError: Failed to construct 'DOMMatrixReadOnly': Failed to parse 'foo'.""#,
            ),
            (
                r"E(() => { try { new DOMMatrixReadOnly('translate(1em)'); } catch (e) { return e.name + ': ' + e.message + ' ' + e.code; } })",
                r#""SyntaxError: Failed to construct 'DOMMatrixReadOnly': Values must be resolvable at parse time 12""#,
            ),
        ],
    },
    Case {
        name: "stringifier",
        html: PAGE_1,
        queries: &[
            (
                r"E(() => S([[1,2,3,4,5,6], [NaN,2,3,4,5,6], [Infinity,0,0,1,0,0], [1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16], [1/3, 2/3, 1e-7, 1e21, 123456789, -0.5], [-0, 0, 1e300, 1.5e-10, 100, 0.000001]], v => { const m = new DOMMatrix(v); return [m.toString(), String(m), `${m}`, JSON.stringify(m)].join(' | '); }))",
                r#"["[1,2,3,4,5,6] => matrix(1, 2, 3, 4, 5, 6) | matrix(1, 2, 3, 4, 5, 6) | matrix(1, 2, 3, 4, 5, 6) | {\"a\":1,\"b\":2,\"c\":3,\"d\":4,\"e\":5,\"f\":6,\"m11\":1,\"m12\":2,\"m13\":0,\"m14\":0,\"m21\":3,\"m22\":4,\"m23\":0,\"m24\":0,\"m31\":0,\"m32\":0,\"m33\":1,\"m34\":0,\"m41\":5,\"m42\":6,\"m43\":0,\"m44\":1,\"is2D\":true,\"isIdentity\":false}", "[null,2,3,4,5,6] => DOMException InvalidStateError: Failed to execute 'toString' on 'DOMMatrixReadOnly': DOMMatrix cannot be serialized with NaN or Infinity values.", "[null,0,0,1,0,0] => DOMException InvalidStateError: Failed to execute 'toString' on 'DOMMatrixReadOnly': DOMMatrix cannot be serialized with NaN or Infinity values.", "[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16] => matrix3d(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16) | matrix3d(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16) | matrix3d(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16) | {\"a\":1,\"b\":2,\"c\":5,\"d\":6,\"e\":13,\"f\":14,\"m11\":1,\"m12\":2,\"m13\":3,\"m14\":4,\"m21\":5,\"m22\":6,\"m23\":7,\"m24\":8,\"m31\":9,\"m32\":10,\"m33\":11,\"m34\":12,\"m41\":13,\"m42\":14,\"m43\":15,\"m44\":16,\"is2D\":false,\"isIdentity\":false}", "[0.3333333333333333,0.6666666666666666,1e-7,1e+21,123456789,-0.5] => matrix(0.3333333333333333, 0.6666666666666666, 1e-7, 1e+21, 123456789, -0.5) | matrix(0.3333333333333333, 0.6666666666666666, 1e-7, 1e+21, 123456789, -0.5) | matrix(0.3333333333333333, 0.6666666666666666, 1e-7, 1e+21, 123456789, -0.5) | {\"a\":0.3333333333333333,\"b\":0.6666666666666666,\"c\":1e-7,\"d\":1e+21,\"e\":123456789,\"f\":-0.5,\"m11\":0.3333333333333333,\"m12\":0.6666666666666666,\"m13\":0,\"m14\":0,\"m21\":1e-7,\"m22\":1e+21,\"m23\":0,\"m24\":0,\"m31\":0,\"m32\":0,\"m33\":1,\"m34\":0,\"m41\":123456789,\"m42\":-0.5,\"m43\":0,\"m44\":1,\"is2D\":true,\"isIdentity\":false}", "[0,0,1e+300,1.5e-10,100,0.000001] => matrix(0, 0, 1e+300, 1.5e-10, 100, 0.000001) | matrix(0, 0, 1e+300, 1.5e-10, 100, 0.000001) | matrix(0, 0, 1e+300, 1.5e-10, 100, 0.000001) | {\"a\":0,\"b\":0,\"c\":1e+300,\"d\":1.5e-10,\"e\":100,\"f\":0.000001,\"m11\":0,\"m12\":0,\"m13\":0,\"m14\":0,\"m21\":1e+300,\"m22\":1.5e-10,\"m23\":0,\"m24\":0,\"m31\":0,\"m32\":0,\"m33\":1,\"m34\":0,\"m41\":100,\"m42\":0.000001,\"m43\":0,\"m44\":1,\"is2D\":true,\"isIdentity\":false}"]"#,
            ),
            (
                r"E(() => { const m = new DOMMatrix([1,2,3,4,5,6]); const w = m.translate(Infinity); try { String(w); } catch (e) { return [e.constructor.name, e.name, e.code, e.message]; } })",
                r#"["DOMException", "InvalidStateError", 11, "Failed to execute 'toString' on 'DOMMatrixReadOnly': DOMMatrix cannot be serialized with NaN or Infinity values."]"#,
            ),
            (
                r"E(() => { const m = new DOMMatrix([1,2,3,4,5,6]); const j = m.toJSON(); return [Object.keys(j).join(), JSON.stringify(j), Object.getPrototypeOf(j) === Object.prototype, JSON.stringify(new DOMMatrix([1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16])), JSON.stringify(new DOMPoint(1,2,3,4)), JSON.stringify(new DOMPointReadOnly(1,2,3,4)), JSON.stringify(new DOMMatrix([NaN,0,0,1,0,0]))]; })",
                r#"["a,b,c,d,e,f,m11,m12,m13,m14,m21,m22,m23,m24,m31,m32,m33,m34,m41,m42,m43,m44,is2D,isIdentity", "{\"a\":1,\"b\":2,\"c\":3,\"d\":4,\"e\":5,\"f\":6,\"m11\":1,\"m12\":2,\"m13\":0,\"m14\":0,\"m21\":3,\"m22\":4,\"m23\":0,\"m24\":0,\"m31\":0,\"m32\":0,\"m33\":1,\"m34\":0,\"m41\":5,\"m42\":6,\"m43\":0,\"m44\":1,\"is2D\":true,\"isIdentity\":false}", true, "{\"a\":1,\"b\":2,\"c\":5,\"d\":6,\"e\":13,\"f\":14,\"m11\":1,\"m12\":2,\"m13\":3,\"m14\":4,\"m21\":5,\"m22\":6,\"m23\":7,\"m24\":8,\"m31\":9,\"m32\":10,\"m33\":11,\"m34\":12,\"m41\":13,\"m42\":14,\"m43\":15,\"m44\":16,\"is2D\":false,\"isIdentity\":false}", "{\"x\":1,\"y\":2,\"z\":3,\"w\":4}", "{\"x\":1,\"y\":2,\"z\":3,\"w\":4}", "{\"a\":null,\"b\":0,\"c\":0,\"d\":1,\"e\":0,\"f\":0,\"m11\":null,\"m12\":0,\"m13\":0,\"m14\":0,\"m21\":0,\"m22\":1,\"m23\":0,\"m24\":0,\"m31\":0,\"m32\":0,\"m33\":1,\"m34\":0,\"m41\":0,\"m42\":0,\"m43\":0,\"m44\":1,\"is2D\":true,\"isIdentity\":false}"]"#,
            ),
            (
                r"E(() => { const m = new DOMMatrix([1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16]); const f32 = m.toFloat32Array(); const f64 = m.toFloat64Array(); return [f32.constructor.name, f64.constructor.name, f32.length, f64.length, Array.from(f32).join(), Array.from(f64).join(), f32.buffer.byteLength, f64.buffer.byteLength, f32.buffer === m.toFloat32Array().buffer, f32 === m.toFloat32Array(), new DOMMatrix([1/3,0,0,1,0,0]).toFloat32Array()[0], new DOMMatrix([0.1,0,0,1,0,0]).toFloat32Array()[0], f32.byteOffset]; })",
                r#"["Float32Array", "Float64Array", 16, 16, "1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16", "1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16", 64, 128, false, false, 0.3333333432674408, 0.10000000149011612, 0]"#,
            ),
        ],
    },
    Case {
        name: "attributes",
        html: PAGE_1,
        queries: &[
            (
                r"E(() => { const m = new DOMMatrix([1,2,3,4,5,6]); const r = []; for (const n of ['a','b','c','d','e','f','m11','m12','m13','m14','m21','m22','m23','m24','m31','m32','m33','m34','m41','m42','m43','m44','is2D','isIdentity']) r.push(n + '=' + m[n]); return r.join(' '); })",
                r#""a=1 b=2 c=3 d=4 e=5 f=6 m11=1 m12=2 m13=0 m14=0 m21=3 m22=4 m23=0 m24=0 m31=0 m32=0 m33=1 m34=0 m41=5 m42=6 m43=0 m44=1 is2D=true isIdentity=false""#,
            ),
            (
                r"E(() => { const m = new DOMMatrix([1,2,3,4,5,6]); const r = []; m.a = 10; r.push(F(m)); m.m11 = 11; r.push(F(m), m.a); m.m13 = 5; r.push(F(m), m.is2D); m.m13 = 0; r.push(F(m), m.is2D); m.m33 = 2; r.push(F(m), m.is2D); m.m33 = 1; r.push(m.is2D); m.m43 = NaN; r.push(m.is2D); m.m43 = 0; m.m44 = 3; r.push(m.is2D); m.is2D = true; r.push(m.is2D); m.m34 = 0.5; r.push(m.is2D); m.e = 7; r.push(m.m41, m.is2D); m.m41 = 8; r.push(m.e); return r; })",
                r#"["2D 10,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "2D 11,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", 11, "3D 11,2,5,0,3,4,0,0,0,0,1,0,5,6,0,1", false, "3D 11,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", false, "3D 11,2,0,0,3,4,0,0,0,0,2,0,5,6,0,1", false, false, false, false, false, false, 7, false, 8]"#,
            ),
            (
                r"E(() => { const m = new DOMMatrix(); m.is2D = false; return [m.is2D, F(m)]; })",
                r#"[true, "2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1"]"#,
            ),
            (
                r"E(() => { const m = new DOMMatrix(); m.isIdentity = false; return [m.isIdentity]; })",
                r"[true]",
            ),
            (
                r"E(() => { const m = new DOMMatrixReadOnly([1,2,3,4,5,6]); m.a = 5; return [m.a, Object.getOwnPropertyDescriptor(DOMMatrixReadOnly.prototype, 'a').set]; })",
                r"[1, null]",
            ),
            (
                r"E(() => { 'use strict'; const m = new DOMMatrixReadOnly([1,2,3,4,5,6]); try { m.a = 5; } catch (e) { return e.constructor.name + ': ' + e.message; } return 'no error'; })",
                r#""TypeError: Cannot set property a of [object DOMMatrixReadOnly] which has only a getter""#,
            ),
            (
                r"E(() => { const m = new DOMMatrix(); const r = []; m.a = 5; m.a = '6'; r.push(m.a); m.a = null; r.push(String(m.a)); m.a = undefined; r.push(String(m.a)); r.push(E(() => { m.a = Symbol(); }), E(() => { m.a = {valueOf() { return 7; }}; }), m.a); m.b = NaN; r.push(String(m.b), F(m)); return r; })",
                r#"[6, "0", "NaN", "TypeError: Failed to set the 'a' property on 'DOMMatrix': Cannot convert a Symbol value to a number", "undefined", 7, "NaN", "2D 7,NaN,0,0,0,1,0,0,0,0,1,0,0,0,0,1"]"#,
            ),
            (
                r"E(() => { const m = new DOMMatrix([1,2,3,4,5,6]); return [m.isIdentity, new DOMMatrix().isIdentity, new DOMMatrix([1,0,0,1,0,0]).isIdentity, new DOMMatrix([1,0,0,1,0,-0]).isIdentity, new DOMMatrix([1,0,0,1,1e-30,0]).isIdentity, new DOMMatrix([NaN,0,0,1,0,0]).isIdentity, new DOMMatrix().translate(0, 0, 1).isIdentity, new DOMMatrix().translate(0, 0, 0).is2D, new DOMMatrix().translate(0, 0, -0).is2D, new DOMMatrix().scale(1, 1, 2).is2D, new DOMMatrix().scale(1, 1, 1).is2D, new DOMMatrix().scale(2).is2D, new DOMMatrix().rotate(0, 0, 90).is2D, new DOMMatrix().rotate(0, 1, 0).is2D, new DOMMatrix().rotate(1, 0, 0).is2D, new DOMMatrix().rotateAxisAngle(0, 0, 1, 0).is2D, new DOMMatrix().rotateAxisAngle(1, 0, 0, 0).is2D, new DOMMatrix().rotateAxisAngle(0, 1, 0, 0).is2D, new DOMMatrix().rotateAxisAngle(0, 0, 1, 10).is2D, new DOMMatrix().scale3d(1).is2D, new DOMMatrix().scale3d(2).is2D, new DOMMatrix().scale3d(1, 0, 0, 1).is2D, new DOMMatrix().scale(1, 1, 1, 0, 0, 1).is2D]; })",
                r"[false, true, true, true, false, false, false, true, true, false, true, true, true, false, false, true, false, false, true, true, false, false, false]",
            ),
        ],
    },
    Case {
        name: "operations",
        html: PAGE_1,
        queries: &[
            (
                r"E(() => { const m = new DOMMatrix([1,2,3,4,5,6]); const r = [F(m.translate(10, 20)), F(m.translate(10)), F(m.translate()), F(m.translate(1, 2, 3)), F(m.translate(0, 0, 0)), F(m.translate(NaN)), F(m.translate('5', '6')), F(m.translate(undefined, 5)), F(m.translate(null, 5))]; r.push(m.translate(1) === m, m.translate(1) instanceof DOMMatrix, m.translate(1).constructor.name, F(m)); return r; })",
                r#"["2D 1,2,0,0,3,4,0,0,0,0,1,0,75,106,0,1", "2D 1,2,0,0,3,4,0,0,0,0,1,0,15,26,0,1", "2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "3D 1,2,0,0,3,4,0,0,0,0,1,0,12,16,3,1", "2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "2D 1,2,0,0,3,4,0,0,0,0,1,0,NaN,NaN,NaN,NaN", "2D 1,2,0,0,3,4,0,0,0,0,1,0,28,40,0,1", "2D 1,2,0,0,3,4,0,0,0,0,1,0,20,26,0,1", "2D 1,2,0,0,3,4,0,0,0,0,1,0,20,26,0,1", false, true, "DOMMatrix", "2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1"]"#,
            ),
            (
                r"E(() => { const m = new DOMMatrixReadOnly([1,2,3,4,5,6]); const x = m.translate(1); return [x.constructor.name, x instanceof DOMMatrix, x instanceof DOMMatrixReadOnly, m.constructor.name, m.scale(2).constructor.name, m.multiply({}).constructor.name, m.inverse().constructor.name, DOMMatrixReadOnly.fromMatrix({}).constructor.name, DOMMatrix.fromMatrix({}).constructor.name, DOMMatrixReadOnly.fromFloat32Array(new Float32Array(6)).constructor.name, DOMMatrix.fromFloat64Array(new Float64Array(6)).constructor.name, DOMMatrix.fromMatrix(new DOMMatrixReadOnly()).constructor.name]; })",
                r#"["DOMMatrix", true, true, "DOMMatrixReadOnly", "DOMMatrix", "DOMMatrix", "DOMMatrix", "DOMMatrixReadOnly", "DOMMatrix", "DOMMatrixReadOnly", "DOMMatrix", "DOMMatrix"]"#,
            ),
            (
                r"E(() => { const m = new DOMMatrix([1,2,3,4,5,6]); return [F(m.scale(2)), F(m.scale(2, 3)), F(m.scale(2, 3, 4)), F(m.scale(2, 2, 1, 10, 20, 0)), F(m.scale(2, 2, 1, 10, 20, 30)), F(m.scale()), F(m.scale(undefined, 3)), F(m.scale(2, 3, 1)), F(m.scale(2, 3, 1.5)), F(m.scale3d(2)), F(m.scale3d(2, 10, 20, 30)), F(m.scale3d(2, 10, 20, 0)), F(m.scale3d()), F(m.scale3d(1)), F(m.scaleNonUniform(2, 3)), F(m.scaleNonUniform(2)), F(m.scaleNonUniform()), F(m.scale(0)), F(m.scale(-1)), Array.from(m.scale(-1).toFloat64Array()).map(Z).join(), Array.from(m.flipX().toFloat64Array()).map(Z).join(), Array.from(m.flipY().toFloat64Array()).map(Z).join()]; })",
                r#"["2D 2,4,0,0,6,8,0,0,0,0,1,0,5,6,0,1", "2D 2,4,0,0,9,12,0,0,0,0,1,0,5,6,0,1", "3D 2,4,0,0,9,12,0,0,0,0,4,0,5,6,0,1", "2D 2,4,0,0,6,8,0,0,0,0,1,0,-65,-94,0,1", "3D 2,4,0,0,6,8,0,0,0,0,1,0,-65,-94,0,1", "2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "2D 1,2,0,0,9,12,0,0,0,0,1,0,5,6,0,1", "2D 2,4,0,0,9,12,0,0,0,0,1,0,5,6,0,1", "3D 2,4,0,0,9,12,0,0,0,0,1.5,0,5,6,0,1", "3D 2,4,0,0,6,8,0,0,0,0,2,0,5,6,0,1", "3D 2,4,0,0,6,8,0,0,0,0,2,0,-65,-94,-30,1", "3D 2,4,0,0,6,8,0,0,0,0,2,0,-65,-94,0,1", "2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "2D 2,4,0,0,9,12,0,0,0,0,1,0,5,6,0,1", "2D 2,4,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "2D 0,0,0,0,0,0,0,0,0,0,1,0,5,6,0,1", "2D -1,-2,0,0,-3,-4,0,0,0,0,1,0,5,6,0,1", "-1,-2,-0,-0,-3,-4,-0,-0,0,0,1,0,5,6,0,1", "-1,-2,-0,-0,3,4,0,0,0,0,1,0,5,6,0,1", "1,2,0,0,-3,-4,-0,-0,0,0,1,0,5,6,0,1"]"#,
            ),
            (
                r"E(() => { const m = new DOMMatrix(); return [F(m.rotate(90)), F(m.rotate(90, 0, 0)), F(m.rotate(0, 90)), F(m.rotate(0, 0, 90)), F(m.rotate(10, 20)), F(m.rotate(10, 20, 30)), F(m.rotate()), F(m.rotate(0, 0, 0)), F(m.rotate(45, undefined, undefined)), F(m.rotate(45, 0)), F(m.rotate(NaN)), F(m.rotate(360)), F(m.rotate(-0)), F(m.rotate(180)), F(m.rotate(270)), F(m.rotate(-90)), F(m.rotate(30))]; })",
                r#"["2D 0,1,0,0,-1,0,0,0,0,0,1,0,0,0,0,1", "3D 1,0,0,0,0,0,1,0,0,-1,0,0,0,0,0,1", "3D 0,0,-1,0,0,1,0,0,1,0,0,0,0,0,0,1", "2D 0,1,0,0,-1,0,0,0,0,0,1,0,0,0,0,1", "3D 0.939693,0,-0.34202,0,0.059391,0.984808,0.163176,0,0.336824,-0.173648,0.925417,0,0,0,0,1", "3D 0.813798,0.469846,-0.34202,0,-0.44097,0.882564,0.163176,0,0.378522,0.018028,0.925417,0,0,0,0,1", "2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "2D 0.707107,0.707107,0,0,-0.707107,0.707107,0,0,0,0,1,0,0,0,0,1", "3D 1,0,0,0,0,0.707107,0.707107,0,0,-0.707107,0.707107,0,0,0,0,1", "2D NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,0,0,1,0,0,0,0,1", "2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "2D -1,0,0,0,0,-1,0,0,0,0,1,0,0,0,0,1", "2D 0,-1,0,0,1,0,0,0,0,0,1,0,0,0,0,1", "2D 0,-1,0,0,1,0,0,0,0,0,1,0,0,0,0,1", "2D 0.866025,0.5,0,0,-0.5,0.866025,0,0,0,0,1,0,0,0,0,1"]"#,
            ),
            (
                r"E(() => { const m = new DOMMatrix(); return [F(m.rotateFromVector(1, 1)), F(m.rotateFromVector(0, 1)), F(m.rotateFromVector(0, 0)), F(m.rotateFromVector(-1, 0)), F(m.rotateFromVector()), F(m.rotateFromVector(1)), F(m.rotateFromVector(NaN, 1)), F(m.rotateAxisAngle(0, 0, 1, 90)), F(m.rotateAxisAngle(1, 0, 0, 90)), F(m.rotateAxisAngle(0, 1, 0, 90)), F(m.rotateAxisAngle(1, 1, 1, 120)), F(m.rotateAxisAngle(0, 0, 0, 90)), F(m.rotateAxisAngle(2, 0, 0, 90)), F(m.rotateAxisAngle(0, 0, 5, 90)), F(m.rotateAxisAngle()), F(m.rotateAxisAngle(0, 0, 1)), F(m.rotateAxisAngle(1, 0, 0, 0)), F(m.rotateAxisAngle(-0, 0, 1, 90)), F(m.rotateAxisAngle(0, 0, -1, 90)), F(m.rotateAxisAngle(1, 2, 3, 45)), F(m.rotateAxisAngle(-1, 0, 1, 60))]; })",
                r#"["2D 0.707107,0.707107,0,0,-0.707107,0.707107,0,0,0,0,1,0,0,0,0,1", "2D 0,1,0,0,-1,0,0,0,0,0,1,0,0,0,0,1", "2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "2D -1,0,0,0,0,-1,0,0,0,0,1,0,0,0,0,1", "2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "2D NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,0,0,1,0,0,0,0,1", "2D 0,1,0,0,-1,0,0,0,0,0,1,0,0,0,0,1", "3D 1,0,0,0,0,0,1,0,0,-1,0,0,0,0,0,1", "3D 0,0,-1,0,0,1,0,0,1,0,0,0,0,0,0,1", "3D 0,1,0,0,0,0,1,0,1,0,0,0,0,0,0,1", "2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "3D 1,0,0,0,0,0,1,0,0,-1,0,0,0,0,0,1", "2D 0,1,0,0,-1,0,0,0,0,0,1,0,0,0,0,1", "2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "3D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "2D 0,1,0,0,-1,0,0,0,0,0,1,0,0,0,0,1", "2D 0,-1,0,0,1,0,0,0,0,0,1,0,0,0,0,1", "3D 0.728028,0.608789,-0.315202,0,-0.525105,0.790791,0.314508,0,0.440727,-0.063457,0.895395,0,0,0,0,1", "3D 0.75,0.612372,-0.25,0,-0.612372,0.5,-0.612372,0,-0.25,0.612372,0.75,0,0,0,0,1"]"#,
            ),
            (
                r"E(() => { const m = new DOMMatrix([1,2,3,4,5,6]); return [F(m.skewX(10)), F(m.skewY(10)), F(m.skewX()), F(m.skewY()), F(m.skewX(45)), F(m.skewX(NaN)), F(m.flipX()), F(m.flipY()), F(m.flipX().flipX()), F(m.inverse()), F(m.inverse().inverse())]; })",
                r#"["2D 1,2,0,0,3.176327,4.352654,0,0,0,0,1,0,5,6,0,1", "2D 1.528981,2.705308,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "2D 1,2,0,0,4,6,0,0,0,0,1,0,5,6,0,1", "2D 1,2,0,0,NaN,NaN,NaN,NaN,0,0,1,0,5,6,0,1", "2D -1,-2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "2D 1,2,0,0,-3,-4,0,0,0,0,1,0,5,6,0,1", "2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "2D -2,1,0,0,1.5,-0.5,0,0,0,0,1,0,1,-2,0,1", "2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1"]"#,
            ),
            (
                r"E(() => S([[1,0,0,0,0,0], [0,0,0,0,0,0], [2,0,0,2,0,0], [1,2,3,4,5,6], [1,2,2,4,0,0], [NaN,0,0,1,0,0], [1e-5,0,0,1e-5,0,0], [1e-9,0,0,1e-9,0,0], [1e-200,0,0,1e-200,0,0]], v => { const m = new DOMMatrix(v); const i = m.inverse(); return F(i) + ' src ' + F(m); }))",
                r#"["[1,0,0,0,0,0] => 3D NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN src 2D 1,0,0,0,0,0,0,0,0,0,1,0,0,0,0,1", "[0,0,0,0,0,0] => 3D NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN src 2D 0,0,0,0,0,0,0,0,0,0,1,0,0,0,0,1", "[2,0,0,2,0,0] => 2D 0.5,0,0,0,0,0.5,0,0,0,0,1,0,0,0,0,1 src 2D 2,0,0,0,0,2,0,0,0,0,1,0,0,0,0,1", "[1,2,3,4,5,6] => 2D -2,1,0,0,1.5,-0.5,0,0,0,0,1,0,1,-2,0,1 src 2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "[1,2,2,4,0,0] => 3D NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN src 2D 1,2,0,0,2,4,0,0,0,0,1,0,0,0,0,1", "[null,0,0,1,0,0] => 3D NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN src 2D NaN,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "[0.00001,0,0,0.00001,0,0] => 2D 100000,0,0,0,0,100000,0,0,0,0,1,0,0,0,0,1 src 2D 0.00001,0,0,0,0,0.00001,0,0,0,0,1,0,0,0,0,1", "[1e-9,0,0,1e-9,0,0] => 2D 1000000000,0,0,0,0,1000000000,0,0,0,0,1,0,0,0,0,1 src 2D 0,0,0,0,0,0,0,0,0,0,1,0,0,0,0,1", "[1e-200,0,0,1e-200,0,0] => 3D NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN src 2D 0,0,0,0,0,0,0,0,0,0,1,0,0,0,0,1"]"#,
            ),
            (
                r"E(() => S([[1,0,0,0, 0,1,0,0, 0,0,1,0, 5,6,7,1], [1,0,0,0, 0,1,0,0, 0,0,0,0, 0,0,0,1], [2,0,0,0, 0,3,0,0, 0,0,4,0, 1,2,3,1], [1,0,0,0, 0,1,0,0, 0,0,1,0.5, 0,0,0,1], [1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16], [1,2,0,1, 0,1,3,0, 4,0,1,2, 1,1,1,1]], v => { const m = new DOMMatrix(v); return F(m.inverse()) + ' | ' + F(m.multiply(m.inverse())); }))",
                r#"["[1,0,0,0,0,1,0,0,0,0,1,0,5,6,7,1] => 3D 1,0,0,0,0,1,0,0,0,0,1,0,-5,-6,-7,1 | 3D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "[1,0,0,0,0,1,0,0,0,0,0,0,0,0,0,1] => 3D NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN | 3D NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN", "[2,0,0,0,0,3,0,0,0,0,4,0,1,2,3,1] => 3D 0.5,0,0,0,0,0.333333,0,0,0,0,0.25,0,-0.5,-0.666667,-0.75,1 | 3D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "[1,0,0,0,0,1,0,0,0,0,1,0.5,0,0,0,1] => 3D 1,0,0,0,0,1,0,0,0,0,1,-0.5,0,0,0,1 | 3D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16] => 3D NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN | 3D NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN", "[1,2,0,1,0,1,3,0,4,0,1,2,1,1,1,1] => 3D 0.625,0.375,0.5,-1.625,0.75,0.25,0,-0.75,-0.25,0.25,0,0.25,-1.125,-0.875,-0.5,3.125 | 3D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1"]"#,
            ),
            (
                r"E(() => { const a = new DOMMatrix([1,2,3,4,5,6]); const b = new DOMMatrix([7,8,9,10,11,12]); return [F(a.multiply(b)), F(b.multiply(a)), F(a.multiply()), F(a.multiply({})), F(a.multiply({a: 2})), F(a.multiply({m13: 1})), F(a.multiply({is2D: false})), F(a.multiply({m44: 2})), F(a.multiply(b.translate(0,0,1))), F(a.multiply(undefined)), F(a.multiply(null))]; })",
                r#"["2D 31,46,0,0,39,58,0,0,0,0,1,0,52,76,0,1", "2D 25,28,0,0,57,64,0,0,0,0,1,0,100,112,0,1", "2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "2D 2,4,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "3D 1,2,1,0,3,4,0,0,0,0,1,0,5,6,0,1", "3D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "3D 1,2,0,0,3,4,0,0,0,0,1,0,10,12,0,2", "3D 31,46,0,0,39,58,0,0,0,0,1,0,52,76,1,1", "2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1"]"#,
            ),
            (
                r"E(() => { const a = new DOMMatrix([1,2,3,4,5,6]); const b = new DOMMatrix([7,8,9,10,11,12]); const r = []; const c = new DOMMatrix(a); r.push(c.multiplySelf(b) === c, F(c)); const d = new DOMMatrix(a); r.push(d.preMultiplySelf(b) === d, F(d)); const e = new DOMMatrix(a); r.push(e.translateSelf(1, 2) === e, e.scaleSelf(2) === e, e.scale3dSelf(2) === e, e.rotateSelf(5) === e, e.rotateFromVectorSelf(1, 1) === e, e.rotateAxisAngleSelf(0,0,1,5) === e, e.skewXSelf(1) === e, e.skewYSelf(1) === e, e.invertSelf() === e); const f = new DOMMatrix([1,2,3,4,5,6]); f.preMultiplySelf({m13: 1}); r.push(f.is2D); return r; })",
                r#"[true, "2D 31,46,0,0,39,58,0,0,0,0,1,0,52,76,0,1", true, "2D 25,28,0,0,57,64,0,0,0,0,1,0,100,112,0,1", true, true, true, true, true, true, true, true, true, false]"#,
            ),
            (
                r"E(() => { const m = new DOMMatrix(); const r = []; r.push(m.setMatrixValue('translate(10px, 20px)') === m, F(m)); r.push(E(() => m.setMatrixValue('foo')), F(m)); r.push(E(() => String(m.setMatrixValue(''))), F(m)); r.push(E(() => m.setMatrixValue()), E(() => m.setMatrixValue(undefined)), E(() => m.setMatrixValue(null)), E(() => m.setMatrixValue(5)), F(m)); r.push(E(() => String(m.setMatrixValue('translate3d(1px,2px,3px)'))), F(m), E(() => String(m.setMatrixValue('scale(2)'))), F(m)); return r; })",
                r#"[true, "2D 1,0,0,0,0,1,0,0,0,0,1,0,10,20,0,1", "DOMException SyntaxError: Failed to execute 'setMatrixValue' on 'DOMMatrix': Failed to parse 'foo'.", "2D 1,0,0,0,0,1,0,0,0,0,1,0,10,20,0,1", "matrix(1, 0, 0, 1, 0, 0)", "2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "TypeError: Failed to execute 'setMatrixValue' on 'DOMMatrix': 1 argument required, but only 0 present.", "DOMException SyntaxError: Failed to execute 'setMatrixValue' on 'DOMMatrix': Failed to parse 'undefined'.", "DOMException SyntaxError: Failed to execute 'setMatrixValue' on 'DOMMatrix': Failed to parse 'null'.", "DOMException SyntaxError: Failed to execute 'setMatrixValue' on 'DOMMatrix': Failed to parse '5'.", "2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 2, 3, 1)", "3D 1,0,0,0,0,1,0,0,0,0,1,0,1,2,3,1", "matrix(2, 0, 0, 2, 0, 0)", "2D 2,0,0,0,0,2,0,0,0,0,1,0,0,0,0,1"]"#,
            ),
        ],
    },
    Case {
        name: "from",
        html: PAGE_1,
        queries: &[
            (
                r"E(() => S([{}, {a: 1}, {a: 1, m11: 2}, {a: 1, m11: 1}, {a: 1, m11: -0}, {a: 0, m11: -0}, {a: NaN, m11: NaN}, {m11: 2}, {a: 2}, {b: 2, m12: 3}, {c: 2, m21: 3}, {d: 2, m22: 3}, {e: 2, m41: 3}, {f: 2, m42: 3}, {is2D: true}, {is2D: false}, {is2D: true, m13: 1}, {is2D: true, m13: 0}, {is2D: true, m13: -0}, {is2D: true, m33: 2}, {is2D: true, m33: 1}, {is2D: true, m44: 2}, {is2D: true, m44: 1}, {is2D: true, m14: 1}, {is2D: true, m23: 1}, {is2D: true, m24: 1}, {is2D: true, m31: 1}, {is2D: true, m32: 1}, {is2D: true, m34: 1}, {is2D: true, m43: 1}, {m13: 1}, {m13: 0}, {m33: 1}, {m33: 2}, {m44: 1}, {m44: 2}, {m43: 1}, {is2D: false, m13: 0}, {is2D: 1}, {is2D: 0}, {is2D: 'false'}, {a: '5'}, {a: 'x'}, {a: null}, {a: undefined}, {m11: undefined}, 5, 'x', null, undefined, [], [1], new DOMMatrix([1,2,3,4,5,6]), new DOMPoint(1,2,3,4)], v => F(DOMMatrix.fromMatrix(v))))",
                r#"["{} => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "{\"a\":1} => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "{\"a\":1,\"m11\":2} => TypeError: Failed to execute 'fromMatrix' on 'DOMMatrix': Property mismatch on matrix initialization.", "{\"a\":1,\"m11\":1} => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "{\"a\":1,\"m11\":0} => TypeError: Failed to execute 'fromMatrix' on 'DOMMatrix': Property mismatch on matrix initialization.", "{\"a\":0,\"m11\":0} => 2D 0,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "{\"a\":null,\"m11\":null} => 2D NaN,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "{\"m11\":2} => 2D 2,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "{\"a\":2} => 2D 2,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "{\"b\":2,\"m12\":3} => TypeError: Failed to execute 'fromMatrix' on 'DOMMatrix': Property mismatch on matrix initialization.", "{\"c\":2,\"m21\":3} => TypeError: Failed to execute 'fromMatrix' on 'DOMMatrix': Property mismatch on matrix initialization.", "{\"d\":2,\"m22\":3} => TypeError: Failed to execute 'fromMatrix' on 'DOMMatrix': Property mismatch on matrix initialization.", "{\"e\":2,\"m41\":3} => TypeError: Failed to execute 'fromMatrix' on 'DOMMatrix': Property mismatch on matrix initialization.", "{\"f\":2,\"m42\":3} => TypeError: Failed to execute 'fromMatrix' on 'DOMMatrix': Property mismatch on matrix initialization.", "{\"is2D\":true} => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "{\"is2D\":false} => 3D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "{\"is2D\":true,\"m13\":1} => TypeError: Failed to execute 'fromMatrix' on 'DOMMatrix': The is2D member is set to true but the input matrix is a 3d matrix.", "{\"is2D\":true,\"m13\":0} => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "{\"is2D\":true,\"m13\":0} => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "{\"is2D\":true,\"m33\":2} => TypeError: Failed to execute 'fromMatrix' on 'DOMMatrix': The is2D member is set to true but the input matrix is a 3d matrix.", "{\"is2D\":true,\"m33\":1} => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "{\"is2D\":true,\"m44\":2} => TypeError: Failed to execute 'fromMatrix' on 'DOMMatrix': The is2D member is set to true but the input matrix is a 3d matrix.", "{\"is2D\":true,\"m44\":1} => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "{\"is2D\":true,\"m14\":1} => TypeError: Failed to execute 'fromMatrix' on 'DOMMatrix': The is2D member is set to true but the input matrix is a 3d matrix.", "{\"is2D\":true,\"m23\":1} => TypeError: Failed to execute 'fromMatrix' on 'DOMMatrix': The is2D member is set to true but the input matrix is a 3d matrix.", "{\"is2D\":true,\"m24\":1} => TypeError: Failed to execute 'fromMatrix' on 'DOMMatrix': The is2D member is set to true but the input matrix is a 3d matrix.", "{\"is2D\":true,\"m31\":1} => TypeError: Failed to execute 'fromMatrix' on 'DOMMatrix': The is2D member is set to true but the input matrix is a 3d matrix.", "{\"is2D\":true,\"m32\":1} => TypeError: Failed to execute 'fromMatrix' on 'DOMMatrix': The is2D member is set to true but the input matrix is a 3d matrix.", "{\"is2D\":true,\"m34\":1} => TypeError: Failed to execute 'fromMatrix' on 'DOMMatrix': The is2D member is set to true but the input matrix is a 3d matrix.", "{\"is2D\":true,\"m43\":1} => TypeError: Failed to execute 'fromMatrix' on 'DOMMatrix': The is2D member is set to true but the input matrix is a 3d matrix.", "{\"m13\":1} => 3D 1,0,1,0,0,1,0,0,0,0,1,0,0,0,0,1", "{\"m13\":0} => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "{\"m33\":1} => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "{\"m33\":2} => 3D 1,0,0,0,0,1,0,0,0,0,2,0,0,0,0,1", "{\"m44\":1} => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "{\"m44\":2} => 3D 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,2", "{\"m43\":1} => 3D 1,0,0,0,0,1,0,0,0,0,1,0,0,0,1,1", "{\"is2D\":false,\"m13\":0} => 3D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "{\"is2D\":1} => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "{\"is2D\":0} => 3D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "{\"is2D\":\"false\"} => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "{\"a\":\"5\"} => 2D 5,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "{\"a\":\"x\"} => 2D NaN,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "{\"a\":null} => 2D 0,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "{} => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "{} => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "5 => TypeError: Failed to execute 'fromMatrix' on 'DOMMatrix': The provided value is not of type 'DOMMatrixInit'.", "\"x\" => TypeError: Failed to execute 'fromMatrix' on 'DOMMatrix': The provided value is not of type 'DOMMatrixInit'.", "null => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "undefined => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "[] => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "[1] => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1", "{\"a\":1,\"b\":2,\"c\":3,\"d\":4,\"e\":5,\"f\":6,\"m11\":1,\"m12\":2,\"m13\":0,\"m14\":0,\"m21\":3,\"m22\":4,\"m23\":0,\"m24\":0,\"m31\":0,\"m32\":0,\"m33\":1,\"m34\":0,\"m41\":5,\"m42\":6,\"m43\":0,\"m44\":1,\"is2D\":true,\"isIdentity\":false} => 2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "{\"x\":1,\"y\":2,\"z\":3,\"w\":4} => 2D I 1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1"]"#,
            ),
            (
                r"E(() => S([6, 16, 0, 5, 15, 17, 1], n => F(DOMMatrix.fromFloat32Array(new Float32Array(Array.from({length: n}, (_, i) => i + 1))))).concat(S([6, 16, 3], n => F(DOMMatrix.fromFloat64Array(new Float64Array(Array.from({length: n}, (_, i) => i + 1)))))).concat([E(() => DOMMatrix.fromFloat32Array(new Float64Array(6))), E(() => DOMMatrix.fromFloat64Array(new Float32Array(6))), E(() => DOMMatrix.fromFloat32Array([1,2,3,4,5,6])), E(() => DOMMatrix.fromFloat32Array()), E(() => DOMMatrix.fromFloat32Array(new Uint8Array(6))), E(() => DOMMatrix.fromFloat32Array(new ArrayBuffer(24))), E(() => DOMMatrix.fromFloat32Array(new DataView(new ArrayBuffer(24))))]))",
                r#"["6 => 2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "16 => 3D 1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16", "0 => TypeError: Failed to execute 'fromFloat32Array' on 'DOMMatrix': The sequence must contain 6 elements for a 2D matrix or 16 elements for a 3D matrix.", "5 => TypeError: Failed to execute 'fromFloat32Array' on 'DOMMatrix': The sequence must contain 6 elements for a 2D matrix or 16 elements for a 3D matrix.", "15 => TypeError: Failed to execute 'fromFloat32Array' on 'DOMMatrix': The sequence must contain 6 elements for a 2D matrix or 16 elements for a 3D matrix.", "17 => TypeError: Failed to execute 'fromFloat32Array' on 'DOMMatrix': The sequence must contain 6 elements for a 2D matrix or 16 elements for a 3D matrix.", "1 => TypeError: Failed to execute 'fromFloat32Array' on 'DOMMatrix': The sequence must contain 6 elements for a 2D matrix or 16 elements for a 3D matrix.", "6 => 2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "16 => 3D 1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16", "3 => TypeError: Failed to execute 'fromFloat64Array' on 'DOMMatrix': The sequence must contain 6 elements for a 2D matrix or 16 elements for a 3D matrix.", "TypeError: Failed to execute 'fromFloat32Array' on 'DOMMatrix': parameter 1 is not of type 'Float32Array'.", "TypeError: Failed to execute 'fromFloat64Array' on 'DOMMatrix': parameter 1 is not of type 'Float64Array'.", "TypeError: Failed to execute 'fromFloat32Array' on 'DOMMatrix': parameter 1 is not of type 'Float32Array'.", "TypeError: Failed to execute 'fromFloat32Array' on 'DOMMatrix': 1 argument required, but only 0 present.", "TypeError: Failed to execute 'fromFloat32Array' on 'DOMMatrix': parameter 1 is not of type 'Float32Array'.", "TypeError: Failed to execute 'fromFloat32Array' on 'DOMMatrix': parameter 1 is not of type 'Float32Array'.", "TypeError: Failed to execute 'fromFloat32Array' on 'DOMMatrix': parameter 1 is not of type 'Float32Array'."]"#,
            ),
            (
                r"E(() => { const m = new DOMMatrix([1,2,3,4,5,6]); const c = new DOMMatrix(m); const d = new DOMMatrixReadOnly(m); const e = new DOMMatrix('matrix(1,2,3,4,5,6)'); return [F(c), F(d), F(e), c === m, d.is2D, F(new DOMMatrix(new DOMMatrixReadOnly([1,0,0,1,5,5]))), F(new DOMMatrix([1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16]).translate(1, 1)), E(() => F(new DOMMatrix(new DOMMatrix([NaN,0,0,1,0,0]))))]; })",
                r#"["2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", "2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", false, true, "2D 1,0,0,0,0,1,0,0,0,0,1,0,5,5,0,1", "3D 1,2,3,4,5,6,7,8,9,10,11,12,19,22,25,28", "DOMException InvalidStateError: Failed to execute 'toString' on 'DOMMatrixReadOnly': DOMMatrix cannot be serialized with NaN or Infinity values."]"#,
            ),
            (
                r"E(() => S([DOMMatrix, DOMMatrixReadOnly, DOMPoint, DOMPointReadOnly], C => typeof C.call).concat([E(() => Reflect.construct(DOMMatrix, [], Object).constructor.name), E(() => DOMMatrix.prototype.a), E(() => Object.getOwnPropertyDescriptor(DOMMatrix.prototype, 'a').get.call({})), E(() => Object.getOwnPropertyDescriptor(DOMMatrix.prototype, 'a').get.call(new DOMMatrixReadOnly())), E(() => Object.getOwnPropertyDescriptor(DOMMatrix.prototype, 'a').get.call(new DOMPoint())), E(() => DOMMatrixReadOnly.prototype.translate.call({}, 1)), E(() => DOMMatrixReadOnly.prototype.translate.call(new DOMMatrix(), 1).constructor.name), E(() => DOMMatrix.prototype.translateSelf.call(new DOMMatrixReadOnly(), 1))]))",
                r#"["undefined => function", "undefined => function", "undefined => function", "undefined => function", "Object", "TypeError: Illegal invocation", "TypeError: Illegal invocation", "TypeError: Illegal invocation", "TypeError: Illegal invocation", "TypeError: Illegal invocation", "DOMMatrix", "TypeError: Illegal invocation"]"#,
            ),
            (
                r"E(() => { class M extends DOMMatrix { foo() { return 1; } } const m = new M([1,2,3,4,5,6]); return [m.foo(), m instanceof DOMMatrix, m.constructor.name, m.translate(1).constructor.name, F(m), m.translateSelf(1) === m]; })",
                r#"[1, true, "M", "DOMMatrix", "2D 1,2,0,0,3,4,0,0,0,0,1,0,5,6,0,1", true]"#,
            ),
        ],
    },
    Case {
        name: "points",
        html: PAGE_1,
        queries: &[
            (
                r"E(() => { const m = new DOMMatrix([2,0,0,2,10,20]); const r = []; for (const v of [undefined, {}, {x: 1, y: 1}, {x: 1, y: 1, z: 1}, {x: 1, y: 1, z: 1, w: 2}, {x: 1, y: 1, w: 0}, new DOMPoint(3, 4), new DOMPointReadOnly(3, 4), {x: NaN}, {x: 'a'}, {x: '5', y: '6'}, 5, null]) { try { const p = m.transformPoint(v); r.push(p.constructor.name + ' ' + [p.x, p.y, p.z, p.w].join()); } catch (e) { r.push(e.constructor.name + ': ' + e.message); } } const p3 = new DOMMatrix().rotate(10, 20, 30).transformPoint({x: 1, y: 2, z: 3, w: 1}); r.push([p3.x, p3.y, p3.z, p3.w].map(N).join()); return r; })",
                r#"["DOMPoint 10,20,0,1", "DOMPoint 10,20,0,1", "DOMPoint 12,22,0,1", "DOMPoint 12,22,1,1", "DOMPoint 22,42,1,2", "DOMPoint 2,2,0,0", "DOMPoint 16,28,0,1", "DOMPoint 16,28,0,1", "DOMPoint NaN,NaN,0,1", "DOMPoint NaN,NaN,0,1", "DOMPoint 20,32,0,1", "TypeError: Failed to execute 'transformPoint' on 'DOMMatrixReadOnly': The provided value is not of type 'DOMPointInit'.", "DOMPoint 10,20,0,1", "1.067425,2.289059,2.760581,1"]"#,
            ),
            (
                r"E(() => { const r = []; for (const args of [[], [1], [1,2], [1,2,3], [1,2,3,4], [undefined, undefined, undefined, undefined], ['1', '2'], [NaN, Infinity], [null, null]]) { const p = new DOMPoint(...args); r.push([p.x, p.y, p.z, p.w].join()); } const p = new DOMPoint(1,2,3,4); p.x = 5; p.w = 6; r.push([p.x, p.y, p.z, p.w].join()); const q = DOMPoint.fromPoint({x: 1, w: 3}); r.push(q.constructor.name, [q.x, q.y, q.z, q.w].join()); const ro = DOMPointReadOnly.fromPoint({}); r.push(ro.constructor.name, [ro.x, ro.y, ro.z, ro.w].join()); r.push(DOMPoint.fromPoint(new DOMPoint(7,8,9,10)).x, DOMPoint.fromPoint().w); r.push(new DOMPointReadOnly(1,2).matrixTransform({a: 2}).constructor.name, new DOMPointReadOnly(1,2).matrixTransform({a: 2, d: 3, e: 10}).x, new DOMPoint(1,2).matrixTransform().x, new DOMPoint(1,2,3,4).matrixTransform(new DOMMatrix().translate(1,2,3)).z); return r; })",
                r#"["0,0,0,1", "1,0,0,1", "1,2,0,1", "1,2,3,1", "1,2,3,4", "0,0,0,1", "1,2,0,1", "NaN,Infinity,0,1", "0,0,0,1", "5,2,3,6", "DOMPoint", "1,0,0,3", "DOMPointReadOnly", "0,0,0,1", 7, 1, "DOMPoint", 12, 1, 15]"#,
            ),
            (
                r"E(() => { const p = new DOMPointReadOnly(1,2,3,4); return [Object.keys(p.toJSON()).join(), JSON.stringify(p), E(() => { p.x = 9; return p.x; }), E(() => { 'use strict'; p.x = 9; })]; })",
                r#"["x,y,z,w", "{\"x\":1,\"y\":2,\"z\":3,\"w\":4}", 1, "TypeError: Cannot set property x of #<DOMPointReadOnly> which has only a getter"]"#,
            ),
            (
                r"E(() => { const r = []; const p = new DOMPoint(); for (const n of ['x', 'y', 'z', 'w']) { p[n] = 'abc'; r.push(String(p[n])); p[n] = null; r.push(String(p[n])); } r.push(E(() => { p.x = Symbol(); })); return r; })",
                r#"["NaN", "0", "NaN", "0", "NaN", "0", "NaN", "0", "TypeError: Failed to set the 'x' property on 'DOMPoint': Cannot convert a Symbol value to a number"]"#,
            ),
            (
                r"E(() => [E(() => new DOMPoint(1, 2).matrixTransform(5)), E(() => new DOMPoint(1, 2).matrixTransform({a: 1, m11: 2})), E(() => new DOMPoint(1, 2).matrixTransform({is2D: true, m13: 1})), E(() => DOMPoint.fromPoint(5)), E(() => DOMPoint.fromPoint(null).x), E(() => DOMPointReadOnly.prototype.matrixTransform.call({}))])",
                r#"["TypeError: Failed to execute 'matrixTransform' on 'DOMPointReadOnly': The provided value is not of type 'DOMMatrixInit'.", "TypeError: Failed to execute 'matrixTransform' on 'DOMPointReadOnly': Property mismatch on matrix initialization.", "TypeError: Failed to execute 'matrixTransform' on 'DOMPointReadOnly': The is2D member is set to true but the input matrix is a 3d matrix.", "TypeError: Failed to execute 'fromPoint' on 'DOMPoint': The provided value is not of type 'DOMPointInit'.", 0, "TypeError: Illegal invocation"]"#,
            ),
        ],
    },
];
