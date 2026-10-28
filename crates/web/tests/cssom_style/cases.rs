// The cases of `tools/probes/web-cssom-style.json` with the values that
// Chromium 148 gave (`just probe --json tools/probes/web-cssom-style.json`, three
// runs, all equal); where swb departs on purpose, the test overrides the value.
// Generated.

use crate::common::Case;

/// Page 1 of the cases.
pub(crate) const PAGE_1: &str = r"<!DOCTYPE html><html><head><title>t</title></head><body><div id=d style='COLOR: Blue; margin:0 auto'></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(t) { return document.createElement(t || 'p'); }
function A(e) { const r = []; for (let i = 0; i < e.style.length; i++) r.push(e.style[i] + ':' + e.style.getPropertyValue(e.style[i]) + (e.style.getPropertyPriority(e.style[i]) ? '!' : '')); return r.join('; '); }
</script></body></html>";

/// Page 2 of the cases.
pub(crate) const PAGE_2: &str = r"<html><head><title>t</title></head><body><div id=d style='COLOR: Blue; margin:0 auto'></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(t) { return document.createElement(t || 'p'); }
function A(e) { const r = []; for (let i = 0; i < e.style.length; i++) r.push(e.style[i] + ':' + e.style.getPropertyValue(e.style[i]) + (e.style.getPropertyPriority(e.style[i]) ? '!' : '')); return r.join('; '); }
</script></body></html>";

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "attribute",
        html: PAGE_1,
        queries: &[
            (
                r"E(() => [d.style.color, d.style.margin, d.style.marginLeft, d.style.cssText, d.style.length, d.getAttribute('style')])",
                r#"["blue", "0px auto", "auto", "color: blue; margin: 0px auto;", 5, "COLOR: Blue; margin:0 auto"]"#,
            ),
            (
                r"E(() => { const e = N(); const s = e.style; return [e.hasAttribute('style'), s.length, s.cssText, s.color, s.getPropertyValue('color'), s.removeProperty('color'), e.hasAttribute('style')]; })",
                r#"[false, 0, "", "", "", "", false]"#,
            ),
            (
                r"E(() => { const e = N(); e.setAttribute('style', 'COLOR:Blue;  margin:0'); e.style.color = 'red'; return [e.getAttribute('style'), e.style.cssText]; })",
                r#"["color: red; margin: 0px;", "color: red; margin: 0px;"]"#,
            ),
            (
                r"E(() => { const e = N(); e.setAttribute('style', 'color: red'); const a = e.style.color; e.setAttribute('style', 'color: blue; width: 1px'); const b = [e.style.color, e.style.width]; e.removeAttribute('style'); return [a, b.join(), e.style.color, e.style.length]; })",
                r#"["red", "blue,1px", "", 0]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.color = 'red'; e.style.cssText = ''; return [e.hasAttribute('style'), e.getAttribute('style'), e.style.length]; })",
                r#"[true, "", 0]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.color = 'red'; e.style.removeProperty('color'); return [e.hasAttribute('style'), e.getAttribute('style')]; })",
                r#"[true, ""]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.cssText = 'x'.repeat(10); return [e.style.cssText, e.hasAttribute('style'), e.getAttribute('style')]; })",
                r#"["", true, ""]"#,
            ),
            (
                r"E(() => { const e = N(); e.style = 'color: red; width: 1px'; return [e.style.cssText, e.getAttribute('style')]; })",
                r#"["color: red; width: 1px;", "color: red; width: 1px;"]"#,
            ),
            (
                r"E(() => { const e = N(); const s = e.style; e.setAttribute('style', 'top: 1px'); e.style.left = '2px'; return [s === e.style, e.getAttribute('style'), s.top]; })",
                r#"[true, "top: 1px; left: 2px;", "1px"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.color = 'red'; const c = e.cloneNode(); return [c.getAttribute('style'), c.style.color, c.style === e.style]; })",
                r#"["color: red;", "red", false]"#,
            ),
        ],
    },
    Case {
        name: "properties",
        html: PAGE_1,
        queries: &[
            (
                r"E(() => { const s = N().style; s.backgroundColor = 'red'; s['border-top-color'] = 'blue'; s.webkitTransform = 'scale(2)'; s.WebkitTransform = 'scale(3)'; return [s.cssText, s['background-color'], s.webkitTransform, s.WebkitTransform, s.transform]; })",
                r#"["background-color: red; border-top-color: blue; transform: scale(3);", "red", "scale(3)", "scale(3)", "scale(3)"]"#,
            ),
            (
                r"E(() => { const s = N().style; s.cssFloat = 'left'; return [s.cssFloat, s.float, s.getPropertyValue('float'), s.cssText, typeof s.float]; })",
                r#"["left", "left", "left", "float: left;", "string"]"#,
            ),
            (
                r"E(() => { const s = N().style; s.float = 'right'; return [s.cssFloat, s.cssText]; })",
                r#"["right", "float: right;"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.zIndex = 5; e.style.opacity = 0.5; e.style.width = 100; e.style.color = 5; e.style.flexGrow = 2; return [e.style.cssText]; })",
                r#"["z-index: 5; opacity: 0.5; flex-grow: 2;"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.color = 'red'; e.style.color = null; const a = e.style.cssText; e.style.color = 'red'; e.style.color = undefined; return [a, e.style.cssText]; })",
                r#"["", "color: red;"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.width = '1px'; e.style.WIDTH = '2px'; return [e.style.cssText, e.style.WIDTH]; })",
                r#"["width: 1px;", "2px"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.setProperty('width', '10px'); const a = e.style.getPropertyValue('width'); e.style.setProperty('width', 'bogus'); e.style.width = 'auto auto'; return [a, e.style.width, e.style.cssText]; })",
                r#"["10px", "10px", "width: 10px;"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.marginInlineStart = '5px'; e.style.inlineSize = '6px'; e.style.wordWrap = 'break-word'; e.style.gridGap = '1px'; e.style.webkitUserSelect = 'none'; return [e.style.cssText, e.style.wordWrap, e.style.userSelect, e.style.length]; })",
                r#"["margin-inline-start: 5px; inline-size: 6px; overflow-wrap: break-word; gap: 1px; user-select: none;", "break-word", "none", 6]"#,
            ),
            (
                r"E(() => { const s = N().style; s.transform = 'translate3d(10px,20px,0)'; const a = s.transform; s['-webkit-transform'] = 'none'; return [a, s.transform, s.cssText]; })",
                r#"["translate3d(10px, 20px, 0px)", "none", "transform: none;"]"#,
            ),
            (
                r"E(() => { const s = N().style; s.display = 'none'; s.display = ''; return [s.display, s.cssText, s.length]; })",
                r#"["", "", 0]"#,
            ),
        ],
    },
    Case {
        name: "feature-tests",
        html: PAGE_1,
        queries: &[
            (
                r"E(() => { const s = d.style; return ['transform', 'webkitTransform', 'WebkitTransform', 'MozTransform', 'msTransform', 'OTransform', 'cssFloat', 'float', 'styleFloat', 'background-color', 'backgroundColor', 'webkitUserSelect', 'userSelect', 'webkitBoxSizing', 'wordWrap', 'marginInlineStart', 'insetBlockStart', 'gridGap', 'webkitFlex', 'textDecorationThickness', 'maskType', 'webkitMaskImage', '--x', 'cssText', 'length', 'parentRule', 'foo', 'objectPosition', 'backgroundPositionX', 'webkitMaskPositionX', 'borderSpacing', 'outlineOffset', 'opacity', 'zIndex', 'BACKGROUND', 'Transform'].map(n => n + '=' + (n in s)).join(' '); })",
                r#""transform=true webkitTransform=true WebkitTransform=true MozTransform=false msTransform=false OTransform=false cssFloat=true float=true styleFloat=false background-color=true backgroundColor=true webkitUserSelect=true userSelect=true webkitBoxSizing=true wordWrap=true marginInlineStart=true insetBlockStart=true gridGap=true webkitFlex=true textDecorationThickness=true maskType=true webkitMaskImage=true --x=false cssText=true length=true parentRule=true foo=false objectPosition=true backgroundPositionX=true webkitMaskPositionX=true borderSpacing=true outlineOffset=true opacity=true zIndex=true BACKGROUND=false Transform=false""#,
            ),
            (
                r"E(() => { const s = d.style; return ['transition', 'animation', 'all', 'webkitAppearance', 'filter', 'boxShadow', 'willChange', 'zoom', 'touchAction', 'borderImage'].map(n => n + '=' + (n in s)).join(' '); })",
                r#""transition=true animation=true all=true webkitAppearance=true filter=true boxShadow=true willChange=true zoom=true touchAction=true borderImage=true""#,
            ),
            (
                r"E(() => { const p = ['transform', 'WebkitTransform', 'MozTransform', 'msTransform']; const s = N().style; return p.filter(n => n in s).join(); })",
                r#""transform,WebkitTransform""#,
            ),
        ],
    },
    Case {
        name: "objects",
        html: PAGE_1,
        queries: &[
            (
                r"E(() => { const s = N().style; s.foo = 1; s.color = 'red'; return [s.foo, Object.keys(s)[0], Object.keys(s).includes('color'), Object.keys(s).includes('backgroundColor'), Object.keys(s).includes('background-color'), Object.keys(s).includes('foo'), s.hasOwnProperty('color'), s.hasOwnProperty('0'), s.hasOwnProperty('1'), JSON.stringify(Object.getOwnPropertyDescriptor(s, '0')), Object.prototype.toString.call(s), delete s.color, s.color, delete s.foo, s.foo]; })",
                r#"[1, "0", true, true, false, true, true, true, false, "{\"value\":\"color\",\"writable\":false,\"enumerable\":true,\"configurable\":true}", "[object CSSStyleDeclaration]", true, "red", true, null]"#,
            ),
            (
                r"E(() => { const s = N().style; return [JSON.stringify(Object.getOwnPropertyDescriptor(s, 'color')), JSON.stringify(Object.getOwnPropertyDescriptor(s, 'background-color')), typeof Object.getOwnPropertyDescriptor(s, 'foo')]; })",
                r#"["{\"value\":\"\",\"writable\":true,\"enumerable\":true,\"configurable\":true}", "{\"value\":\"\",\"writable\":true,\"enumerable\":true,\"configurable\":true}", "undefined"]"#,
            ),
            (
                r"E(() => { const p = CSSStyleDeclaration.prototype; return [typeof Object.getOwnPropertyDescriptor(p, 'color'), Object.getOwnPropertyNames(p).join(), CSSStyleDeclaration.name, CSSStyleDeclaration.length]; })",
                r#"["undefined", "cssText,length,parentRule,cssFloat,getPropertyPriority,getPropertyValue,item,removeProperty,setProperty,constructor", "CSSStyleDeclaration", 0]"#,
            ),
            (
                r"E(() => { const p = CSSStyleDeclaration.prototype; return ['cssText', 'length', 'item', 'getPropertyValue', 'getPropertyPriority', 'setProperty', 'removeProperty', 'parentRule', 'cssFloat'].map(n => { const x = Object.getOwnPropertyDescriptor(p, n); return n + ':' + (x ? (x.get ? 'acc' : 'val') + (x.set ? 'S' : '') + (x.value ? x.value.length : '') + (x.enumerable ? 'E' : '') : 'none'); }).join(' '); })",
                r#""cssText:accSE length:accE item:val1E getPropertyValue:val1E getPropertyPriority:val1E setProperty:val2E removeProperty:val1E parentRule:accE cssFloat:accSE""#,
            ),
            (
                r"E(() => { const e = N(); return [e.style === e.style, e.style.parentRule, String(e.style), e.style.constructor.name, Object.getPrototypeOf(HTMLElement.prototype) === Element.prototype, 'style' in HTMLElement.prototype, 'style' in SVGElement.prototype, 'style' in Element.prototype]; })",
                r#"[true, null, "[object CSSStyleDeclaration]", "CSSStyleDeclaration", true, true, true, false]"#,
            ),
            (
                r"E(() => { const d2 = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'style'); return [typeof d2.get, typeof d2.set, d2.enumerable, d2.configurable]; })",
                r#"["function", "function", true, true]"#,
            ),
            (
                r"E(() => { const e = document.createElementNS('http://www.w3.org/2000/svg', 'rect'); e.style.fill = 'red'; e.style.color = 'blue'; return [e.getAttribute('style'), e.style.constructor.name, typeof e.style]; })",
                r#"["fill: red; color: blue;", "CSSStyleDeclaration", "object"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.setProperty('color'); return 'x'; })",
                r#""TypeError: Failed to execute 'setProperty' on 'CSSStyleDeclaration': 2 arguments required, but only 1 present.""#,
            ),
            (
                r"E(() => { const e = N(); return [e.style.setProperty('color', 'red'), e.style.removeProperty('foo'), e.style.getPropertyValue('foo'), e.style.getPropertyPriority('foo')]; })",
                r#"[null, "", "", ""]"#,
            ),
            (
                r"E(() => CSSStyleDeclaration.prototype.getPropertyValue.call({}, 'color'))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => { new CSSStyleDeclaration(); return 1; })",
                r#""TypeError: Failed to construct 'CSSStyleDeclaration': Illegal constructor""#,
            ),
            (
                r"E(() => { const s = N().style; s.margin = '1px'; return [s.length, s.item(0), s.item(3), s.item(4), s[0], String(s[4]), s.item(-1), s.item(4294967296), [...s].join()]; })",
                r#"[4, "margin-top", "margin-left", "", "margin-top", "undefined", "", "margin-top", "margin-top,margin-right,margin-bottom,margin-left"]"#,
            ),
            (
                r"E(() => { const s = N().style; s[0] = 'x'; return [s[0], s.length]; })",
                r"[null, 0]",
            ),
            (
                r"E(() => { const s = N().style; s.color = 'red'; try { Object.defineProperty(s, 'color', { value: 'blue' }); } catch (e) { return e.constructor.name; } return s.color; })",
                r#""blue""#,
            ),
            (
                r"E(() => { 'use strict'; const s = N().style; try { delete s.color; return 'deleted'; } catch (e) { return e.constructor.name; } })",
                r#""deleted""#,
            ),
        ],
    },
    Case {
        name: "priorities",
        html: PAGE_1,
        queries: &[
            (
                r"E(() => { const e = N(); e.style.setProperty('color', 'red', 'important'); return [e.style.getPropertyPriority('color'), e.style.cssText, e.style.color]; })",
                r#"["important", "color: red !important;", "red"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.setProperty('color', 'red', 'IMPORTANT'); e.style.setProperty('width', '1px', 'foo'); e.style.setProperty('height', '1px', ''); return [e.style.cssText]; })",
                r#"["color: red !important; height: 1px;"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.setProperty('color', 'red', 'important'); e.style.color = 'blue'; const a = e.style.cssText; e.style.setProperty('color', 'green'); return [a, e.style.cssText]; })",
                r#"["color: blue;", "color: green;"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.setProperty('margin', '1px', 'important'); return [e.style.cssText, e.style.getPropertyPriority('margin'), e.style.getPropertyPriority('margin-top'), e.style.length]; })",
                r#"["margin: 1px !important;", "important", "important", 4]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.setProperty('margin', '1px', 'important'); e.style.marginTop = '2px'; return [e.style.cssText, e.style.margin, e.style.getPropertyPriority('margin')]; })",
                r#"["margin-top: 2px; margin-right: 1px !important; margin-bottom: 1px !important; margin-left: 1px !important;", "", ""]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.setProperty('color', 'red', 'important'); e.style.setProperty('color', 'blue', 'bogus'); return [e.style.cssText]; })",
                r#"["color: red !important;"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.setProperty('color', 'red', null); e.style.setProperty('width', '1px', undefined); return [e.style.cssText]; })",
                r#"["color: red; width: 1px;"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.cssText = 'color: red !IMPORTANT; width: 1px ! important; height: 2px!important'; return [e.style.cssText]; })",
                r#"["color: red !important; width: 1px !important; height: 2px !important;"]"#,
            ),
        ],
    },
    Case {
        name: "css-text",
        html: PAGE_1,
        queries: &[
            (
                r"E(() => { const e = N(); e.style.cssText = 'color: RED; foo: bar; width: 10px !important; color: blue; /* c */ height: 1px; margin: 1px 2px; ;;; bad; padding-top: 0'; return [e.style.cssText, e.getAttribute('style'), e.style.length]; })",
                r#"["color: blue; height: 1px; margin: 1px 2px; padding-top: 0px; width: 10px !important;", "color: blue; height: 1px; margin: 1px 2px; padding-top: 0px; width: 10px !important;", 8]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.cssText = 'color: red; width: 1px; color: blue'; return [e.style.cssText]; })",
                r#"["width: 1px; color: blue;"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.cssText = 'margin: 1px; margin-top: 2px'; return [e.style.cssText, e.style.margin]; })",
                r#"["margin: 2px 1px 1px;", "2px 1px 1px"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.cssText = 'margin: 1px !important; margin-top: 2px'; return [e.style.cssText]; })",
                r#"["margin: 1px !important;"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.cssText = 'margin-top: 1px !important; margin-right: 1px; margin-bottom: 1px; margin-left: 1px'; return [e.style.cssText, e.style.margin]; })",
                r#"["margin-right: 1px; margin-bottom: 1px; margin-left: 1px; margin-top: 1px !important;", ""]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.cssText = '  ;color:red'; const a = e.style.cssText; e.style.cssText = 'color:red;}width:1px'; const b = e.style.cssText; e.style.cssText = 'color:red;{width:1px};height:1px'; return [a, b, e.style.cssText]; })",
                r#"["color: red;", "color: red;", "color: red; height: 1px;"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.cssText = '--a: 1px; --a: 2px; --b:x'; return [e.style.cssText, e.style.length, e.style.item(0), e.style.getPropertyValue('--a')]; })",
                r#"["--a: 2px; --b: x;", 2, "--a", "2px"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.cssText = null; const a = [e.hasAttribute('style'), e.getAttribute('style')]; e.style.cssText = 5; return [a.join(), e.getAttribute('style')]; })",
                r#"["true,", ""]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.setProperty('COLOR', 'red'); e.style.setProperty('--X', '1'); return [e.style.cssText, e.style.getPropertyValue('COLOR'), e.style.getPropertyValue('--x'), e.style.getPropertyValue('--X')]; })",
                r#"["color: red; --X: 1;", "red", "", "1"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.setProperty('--x', '  '); e.style.setProperty('--y', ''); return [e.style.cssText, e.style.length]; })",
                r#"["--x: ;", 1]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.color = 'red'; e.style.width = '1px'; e.style.color = 'blue'; return [e.style.cssText]; })",
                r#"["color: blue; width: 1px;"]"#,
            ),
        ],
    },
    Case {
        name: "shorthands",
        html: PAGE_1,
        queries: &[
            (
                r"E(() => { const e = N(); e.style.margin = '1px 2px'; const r = e.style.removeProperty('margin'); return [r, e.style.cssText, e.style.length]; })",
                r#"["", "", 0]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.margin = '1px 2px'; const r = e.style.removeProperty('margin-top'); return [r, e.style.cssText, e.style.margin, e.style.length]; })",
                r#"["1px", "margin-right: 2px; margin-bottom: 1px; margin-left: 2px;", "", 3]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.marginTop = '1px'; e.style.marginRight = '1px'; e.style.marginBottom = '1px'; const a = e.style.cssText; e.style.marginLeft = '1px'; return [a, e.style.cssText, e.style.margin, e.style.length]; })",
                r#"["margin-top: 1px; margin-right: 1px; margin-bottom: 1px;", "margin: 1px;", "1px", 4]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.marginTop = '1px'; e.style.color = 'red'; e.style.marginRight = '2px'; e.style.marginBottom = '1px'; e.style.marginLeft = '2px'; return [e.style.cssText]; })",
                r#"["margin: 1px 2px; color: red;"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.cssText = 'border: 1px solid red; border-top-color: blue'; return [e.style.cssText, e.style.border, e.style.borderColor, e.style.borderTop, e.style.borderWidth]; })",
                r#"["border-width: 1px; border-style: solid; border-color: blue red red; border-image: initial;", "", "blue red red", "1px solid blue", "1px"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.border = 'none'; return [e.style.cssText, e.style.border, e.style.borderStyle, e.style.borderTopWidth]; })",
                r#"["border-width: medium; border-style: none; border-color: currentcolor; border-image: initial;", "", "none", "medium"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.border = '1px solid'; return [e.style.cssText, e.style.borderTopColor, e.style.borderLeftStyle]; })",
                r#"["border: 1px solid;", "currentcolor", "solid"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.background = 'red'; e.style.backgroundImage = 'url(a.png)'; return [e.style.cssText, e.style.background, e.style.backgroundRepeat]; })",
                r#"["background: url(\"a.png\") red;", "url(\"a.png\") red", "initial"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.background = 'url(a.png) center / cover no-repeat'; return [e.style.cssText, e.style.backgroundPositionX, e.style.backgroundSize, e.style.backgroundColor]; })",
                r#"["background: url(\"a.png\") center center / cover no-repeat;", "center", "cover", "initial"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.font = '12px a'; e.style.fontWeight = 'bold'; return [e.style.cssText, e.style.font, e.style.lineHeight]; })",
                r#"["font: bold 12px a;", "bold 12px a", "normal"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.font = 'bold italic 12px/1.5 Arial, sans-serif'; return [e.style.cssText, e.style.fontStyle, e.style.fontFamily, e.style.lineHeight]; })",
                r#"["font: italic bold 12px / 1.5 Arial, sans-serif;", "italic", "Arial, sans-serif", "1.5"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.flex = '1'; const a = [e.style.flexGrow, e.style.flexShrink, e.style.flexBasis].join(); e.style.flex = 'none'; return [a, e.style.flex, e.style.cssText]; })",
                r#"["1,1,0%", "0 0 auto", "flex: 0 0 auto;"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.overflow = 'hidden auto'; e.style.textDecoration = 'underline'; e.style.outline = '0'; return [e.style.cssText, e.style.overflowY, e.style.textDecorationColor, e.style.outlineStyle]; })",
                r#"["overflow: hidden auto; text-decoration: underline; outline: 0px;", "auto", "initial", "initial"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.borderRadius = '5px / 10px'; e.style.inset = '0'; return [e.style.cssText, e.style.borderTopLeftRadius, e.style.left]; })",
                r#"["border-radius: 5px / 10px; inset: 0px;", "5px 10px", "0px"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.margin = 'var(--m)'; return [e.style.cssText, e.style.margin, e.style.marginTop, e.style.length]; })",
                r#"["margin: var(--m);", "var(--m)", "", 4]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.margin = 'inherit'; const a = [e.style.cssText, e.style.marginTop]; e.style.marginTop = '1px'; return [a.join(), e.style.cssText, e.style.margin]; })",
                r#"["margin: inherit;,inherit", "margin-top: 1px; margin-right: inherit; margin-bottom: inherit; margin-left: inherit;", ""]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.cssText = 'margin: 1px 2px; padding: 3px; color: red'; e.style.removeProperty('padding-left'); return [e.style.cssText, e.style.padding]; })",
                r#"["margin: 1px 2px; padding-top: 3px; padding-right: 3px; padding-bottom: 3px; color: red;", ""]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.gridArea = 'a'; e.style.backgroundPosition = 'center'; e.style.listStyle = 'square'; e.style.flexFlow = 'column'; return [e.style.cssText, e.style.gridRowEnd]; })",
                r#"["grid-area: a; background-position: center center; list-style: square; flex-flow: column;", "a"]"#,
            ),
        ],
    },
    Case {
        name: "values",
        html: PAGE_1,
        queries: &[
            (
                r"E(() => { const e = N(); const r = []; for (const v of ['red', '#F00', 'rgba(0,0,0,.5)', 'transparent', 'currentColor', 'hsl(120, 100%, 50%)', 'foo']) { e.style.color = v; r.push(e.style.color); } return r.join(' | '); })",
                r#""red | rgb(255, 0, 0) | rgba(0, 0, 0, 0.5) | transparent | currentcolor | rgb(0, 255, 0) | rgb(0, 255, 0)""#,
            ),
            (
                r"E(() => { const e = N(); const r = []; for (const v of ['10PX', '0', '.5em', '50%', 'auto', 'calc(10px + 5px)', 'calc(100% - 10px)', '1.23456789px', '-1px']) { e.style.width = ''; e.style.width = v; r.push(e.style.width); } return r.join(' | '); })",
                r#""10px | 0px | 0.5em | 50% | auto | calc(15px) | calc(100% - 10px) | 1.23457px | ""#,
            ),
            (
                r"E(() => { const e = N(); const r = []; for (const v of ['translate(10px,20px)', 'translateX(0)', 'rotate(0)', 'scale(1.5, 2)', 'matrix(1,0,0,1,0,0)', 'none', 'bogus(1)']) { e.style.transform = ''; e.style.transform = v; r.push(e.style.transform); } return r.join(' | '); })",
                r#""translate(10px, 20px) | translateX(0px) | rotate(0deg) | scale(1.5, 2) | matrix(1, 0, 0, 1, 0, 0) | none | ""#,
            ),
            (
                r"E(() => { const e = N(); const r = []; for (const v of ['BLOCK', 'inline flex', 'none', 'grid']) { e.style.display = v; r.push(e.style.display); } return r.join(' | '); })",
                r#""block | inline-flex | none | grid""#,
            ),
            (
                r#"E(() => { const e = N(); e.style.fontFamily = "Arial  ,  'Helvetica Neue', SANS-SERIF"; e.style.backgroundImage = 'url(a.png)'; e.style.content = "'x'"; e.style.opacity = '50%'; return [e.style.fontFamily, e.style.backgroundImage, e.style.content, e.style.opacity]; })"#,
                r#"["Arial, \"Helvetica Neue\", sans-serif", "url(\"a.png\")", "\"x\"", "0.5"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.transformOrigin = 'top left'; e.style.zIndex = '2147483648'; e.style.counterReset = 'x'; e.style.aspectRatio = '2'; return e.style.cssText; })",
                r#""transform-origin: left top; z-index: 2147483647; counter-reset: x 0; aspect-ratio: 2 / 1;""#,
            ),
        ],
    },
    Case {
        name: "quirks",
        html: PAGE_2,
        queries: &[
            (
                r"E(() => { const e = N(); e.style.width = '100'; e.style.height = 100; e.style.marginTop = '5'; e.style.lineHeight = '2'; e.style.setProperty('padding', '1 2'); e.style.setProperty('margin-inline-start', '3'); return [e.style.cssText, document.compatMode]; })",
                r#"["width: 100px; height: 100px; margin-top: 5px; line-height: 2; padding: 1px 2px;", "BackCompat"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.cssText = 'width: 10; top: 5; border: 1 solid red'; return [e.style.cssText]; })",
                r#"["width: 10px; top: 5px;"]"#,
            ),
        ],
    },
    Case {
        name: "css-namespace",
        html: PAGE_1,
        queries: &[
            (
                r"E(() => [CSS.supports('display', 'flex'), CSS.supports('display', 'bogus'), CSS.supports('DISPLAY', 'flex'), CSS.supports('display: flex'), CSS.supports('(display: flex)'), CSS.supports('display:flex and color:red'), CSS.supports('(display:flex) and (color:red)'), CSS.supports('--x', 'anything'), CSS.supports('--x: 1'), CSS.supports('color', 'red !important'), CSS.supports('not (display: bogus)'), CSS.supports(''), CSS.supports('color', ''), CSS.supports('color', 'var(--x)'), CSS.supports('(color: red) or (bogus: 1)'), CSS.supports('display', ' flex '), CSS.supports('  display', 'flex'), CSS.supports('font-tech(color-COLRv1)'), CSS.supports('width', '10'), CSS.supports('color', 'red; width: 1px'), CSS.supports('color: red;'), CSS.supports('(color: red)  '), CSS.supports('float', 'left'), CSS.supports('cssFloat', 'left'), CSS.supports('position', 'sticky'), CSS.supports('-webkit-transform', 'none'), CSS.supports('gap', '1px'), CSS.supports('aspect-ratio', '1'), CSS.supports('display', 'contents')].join())",
                r#""true,false,true,true,true,false,true,true,true,false,true,false,false,true,true,true,false,true,false,false,false,true,true,false,true,true,true,true,true""#,
            ),
            (
                r"E(() => [CSS.supports('selector(a > b)'), CSS.supports('selector(:has(a))'), CSS.supports('selector(a:hover)'), CSS.supports('selector(<)')].join())",
                r#""true,true,true,false""#,
            ),
            (
                r"E(() => [typeof CSS, Object.prototype.toString.call(CSS), Object.getOwnPropertyNames(CSS).filter(n => n === 'supports' || n === 'escape').join(), CSS.supports.length, CSS.escape.length, CSS.supports.name, typeof CSS.prototype, JSON.stringify(Object.getOwnPropertyDescriptor(window, 'CSS')), JSON.stringify(Object.getOwnPropertyDescriptor(CSS, 'supports')), Object.getPrototypeOf(CSS) === Object.prototype].join(' '))",
                r#""object [object CSS] escape,supports 1 1 supports undefined {\"value\":{\"highlights\":{},\"paintWorklet\":{}},\"writable\":true,\"enumerable\":false,\"configurable\":true} {\"writable\":true,\"enumerable\":true,\"configurable\":true} true""#,
            ),
            (
                r"E(() => [CSS.supports(), 1])",
                r#""TypeError: Failed to execute 'supports' on 'CSS': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => [CSS.supports('a', 'b', 'c'), CSS.supports(1, 2), CSS.supports(null), CSS.supports(undefined)].join())",
                r#""false,false,false,false""#,
            ),
            (
                r"E(() => ['a', '1a', '-1a', '-', '--', 'a b', 'a\u0000b', '\u0001', '\u007f', 'é', '#id', 'a.b', '', '-a', '_a', '0', '--x', 'a\\b', '\t'].map(s => JSON.stringify(CSS.escape(s))).join(' '))",
                r#""\"a\" \"\\\\31 a\" \"-\\\\31 a\" \"\\\\-\" \"--\" \"a\\\\ b\" \"a�b\" \"\\\\1 \" \"\\\\7f \" \"é\" \"\\\\#id\" \"a\\\\.b\" \"\" \"-a\" \"_a\" \"\\\\30 \" \"--x\" \"a\\\\\\\\b\" \"\\\\9 \"""#,
            ),
            (
                r"E(() => [CSS.escape(5), CSS.escape(null), CSS.escape()])",
                r#""TypeError: Failed to execute 'escape' on 'CSS': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => CSS.escape(Symbol()))",
                r#""TypeError: Failed to execute 'escape' on 'CSS': Cannot convert a Symbol value to a string""#,
            ),
            (
                r"E(() => { new CSS.supports('a'); return 1; })",
                r#""TypeError: CSS.supports is not a constructor""#,
            ),
        ],
    },
    Case {
        name: "review-7a1",
        html: PAGE_1,
        queries: &[
            (
                r"E(() => { const e = N(); e.style.outline = 'auto'; return [e.style.outlineStyle, e.style.outlineColor, e.style.outlineWidth, e.style.cssText]; })",
                r#"["auto", "initial", "initial", "outline: auto;"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.outline = 'auto'; const r = e.style.removeProperty('outline-style'); return [r, e.style.cssText]; })",
                r#"["auto", "outline-color: initial; outline-width: initial;"]"#,
            ),
            (
                r"E(() => { const e = N(); e.style.outline = 'auto red'; const a = e.style.cssText; e.style.outline = 'red auto'; return [a, e.style.cssText, e.style.outlineStyle]; })",
                r#"["outline: red auto;", "outline: red auto;", "auto"]"#,
            ),
            (
                r"E(() => ['display:flex !important', '(display:flex !important)', 'display: flex!important', '(color: red !important) and (display: flex)', 'display: flex !IMPORTANT', 'display: flex ! important', '(display: flex !important !important)'].map(c => CSS.supports(c)).join())",
                r#""true,true,true,true,true,true,false""#,
            ),
            (
                r"E(() => { const e = N(); e.style.textDecoration = 'line-through 2px'; return [e.style.textDecorationLine, e.style.cssText]; })",
                r#"["line-through", "text-decoration: line-through 2px;"]"#,
            ),
        ],
    },
];
