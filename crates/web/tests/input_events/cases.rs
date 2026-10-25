// The cases of `tools/probes/web-input-events.json` with the values that
// Chromium 148 gave (`just probe tools/probes/web-input-events.json`, three
// runs, all equal); where swb departs on purpose, the test overrides the value.
// Generated.

use crate::common::Case;

/// A page of several cases.
pub(crate) const PAGE0: &str = r#"<!DOCTYPE html><html><head><title>t</title></head><body style="margin:0"><div id=d style="position:absolute;left:30px;top:40px;width:100px;height:50px"><span id=s>text</span></div><form id=f action="x" method=get><input id=i name=n value=v><input id=cb type=checkbox><button id=b>b</button></form><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(x) { if (x === null) return 'null'; if (x === undefined) return 'undefined'; if (x === window) return 'window'; if (x === document) return 'document'; if (x.nodeType === 1) return x.localName + (x.id ? '#' + x.id : ''); if (x.nodeType) return '#' + x.nodeType; return 'target'; }
function C(o) { const r = []; for (let p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) r.push(Object.prototype.toString.call(p)); return r.join(' '); }
function P(o, names) { return names.map((n) => n + '=' + (typeof o[n] === 'function' ? 'fn' : String(o[n]))).join(' '); }
</script></body></html>"#;

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "ui-event",
        html: PAGE0,
        queries: &[
            (
                r"E(() => P(new UIEvent('x'), ['type', 'bubbles', 'cancelable', 'composed', 'view', 'detail', 'which', 'isTrusted']))",
                r#""type=x bubbles=false cancelable=false composed=false view=null detail=0 which=0 isTrusted=false""#,
            ),
            (
                r"E(() => P(new UIEvent('x', {view: window, detail: 3, which: 7, bubbles: true}), ['bubbles', 'view', 'detail', 'which']))",
                r#""bubbles=true view=[object Window] detail=3 which=0""#,
            ),
            (
                r"E(() => P(new UIEvent('x', {view: null, detail: 2.7}), ['view', 'detail']))",
                r#""view=null detail=2""#,
            ),
            (
                r"E(() => new UIEvent('x', {view: 5}))",
                r#""TypeError: Failed to construct 'UIEvent': Failed to read the 'view' property from 'UIEventInit': Failed to convert value to 'Window'.""#,
            ),
            (
                r"E(() => new UIEvent('x', {view: document}))",
                r#""TypeError: Failed to construct 'UIEvent': Failed to read the 'view' property from 'UIEventInit': Failed to convert value to 'Window'.""#,
            ),
            (
                r"E(() => new UIEvent('x', {view: {}}))",
                r#""TypeError: Failed to construct 'UIEvent': Failed to read the 'view' property from 'UIEventInit': Failed to convert value to 'Window'.""#,
            ),
            (
                r"E(() => new UIEvent('x', 5))",
                r#""TypeError: Failed to construct 'UIEvent': The provided value is not of type 'UIEventInit'.""#,
            ),
            (r"E(() => new UIEvent('x', null).detail)", r"0"),
            (
                r"E(() => [new UIEvent('x', {detail: 'a'}).detail, new UIEvent('x', {detail: -1.9}).detail, new UIEvent('x', {detail: 2 ** 33}).detail, new UIEvent('x', {detail: 2 ** 31}).detail, new UIEvent('x', {detail: '12'}).detail])",
                r#""0,-1,0,-2147483648,12""#,
            ),
            (
                r"E(() => [new UIEvent('x', {which: -1}).which, new UIEvent('x', {which: 2 ** 32 + 5}).which, new UIEvent('x', {which: 1.5}).which])",
                r#""0,0,0""#,
            ),
            (
                r"E(() => new UIEvent())",
                r#""TypeError: Failed to construct 'UIEvent': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => UIEvent('x'))",
                r#""TypeError: Failed to construct 'UIEvent': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => [UIEvent.length, UIEvent.name, C(new UIEvent('x')), Object.getOwnPropertyNames(UIEvent.prototype).join()])",
                r#""1,UIEvent,[object UIEvent] [object Event] [object Object],view,detail,sourceCapabilities,which,initUIEvent,constructor""#,
            ),
            (
                r"E(() => { const d = Object.getOwnPropertyDescriptor(UIEvent.prototype, 'detail'); return [typeof d.get, d.set, d.enumerable, d.configurable, d.get.name].join(); })",
                r#""function,,true,true,get detail""#,
            ),
            (
                r"E(() => { const e = document.createEvent('UIEvent'); return [e.constructor.name, e.view, e.detail, e.type, e.bubbles, e.which].join(); })",
                r#""UIEvent,,0,,false,0""#,
            ),
            (
                r"E(() => { const e = document.createEvent('UIEvents'); e.initUIEvent('foo', true, true, window, 5); return P(e, ['type', 'bubbles', 'cancelable', 'view', 'detail']); })",
                r#""type=foo bubbles=true cancelable=true view=[object Window] detail=5""#,
            ),
            (
                r"E(() => { const e = document.createEvent('UIEvent'); e.initUIEvent('foo'); return P(e, ['type', 'bubbles', 'cancelable', 'view', 'detail']); })",
                r#""type=foo bubbles=false cancelable=false view=null detail=0""#,
            ),
            (
                r"E(() => { const e = document.createEvent('UIEvent'); e.initUIEvent(); })",
                r#""TypeError: Failed to execute 'initUIEvent' on 'UIEvent': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => { const e = document.createEvent('UIEvent'); e.initUIEvent('foo', false, false, 5); })",
                r#""TypeError: Failed to execute 'initUIEvent' on 'UIEvent': parameter 4 is not of type 'Window'.""#,
            ),
            (r"E(() => UIEvent.prototype.initUIEvent.length)", r"1"),
            (
                r"E(() => { const e = new UIEvent('x'); const log = []; d.addEventListener('x', (ev) => log.push(ev.view === window, ev.detail)); d.dispatchEvent(e); return log.join(); })",
                r#""false,0""#,
            ),
        ],
    },
    Case {
        name: "mouse-event",
        html: PAGE0,
        queries: &[
            (
                r#"E(() => P(new MouseEvent('click'), ["screenX", "screenY", "clientX", "clientY", "pageX", "pageY", "x", "y", "offsetX", "offsetY", "layerX", "layerY", "movementX", "movementY", "ctrlKey", "shiftKey", "altKey", "metaKey", "button", "buttons", "relatedTarget", "fromElement", "toElement", "detail", "which", "view"]))"#,
                r#""screenX=0 screenY=0 clientX=0 clientY=0 pageX=0 pageY=0 x=0 y=0 offsetX=0 offsetY=0 layerX=0 layerY=0 movementX=0 movementY=0 ctrlKey=false shiftKey=false altKey=false metaKey=false button=0 buttons=0 relatedTarget=null fromElement=null toElement=null detail=0 which=1 view=null""#,
            ),
            (
                r#"E(() => P(new MouseEvent('click', {screenX: 1, screenY: 2, clientX: 3, clientY: 4, ctrlKey: true, shiftKey: true, altKey: true, metaKey: true, button: 2, buttons: 5, relatedTarget: d, detail: 2, view: window}), ["screenX", "screenY", "clientX", "clientY", "pageX", "pageY", "x", "y", "offsetX", "offsetY", "layerX", "layerY", "movementX", "movementY", "ctrlKey", "shiftKey", "altKey", "metaKey", "button", "buttons", "relatedTarget", "fromElement", "toElement", "detail", "which", "view"]))"#,
                r#""screenX=1 screenY=2 clientX=3 clientY=4 pageX=3 pageY=4 x=3 y=4 offsetX=3 offsetY=4 layerX=3 layerY=4 movementX=0 movementY=0 ctrlKey=true shiftKey=true altKey=true metaKey=true button=2 buttons=5 relatedTarget=[object HTMLDivElement] fromElement=[object HTMLDivElement] toElement=null detail=2 which=3 view=[object Window]""#,
            ),
            (
                r"E(() => P(new MouseEvent('click', {clientX: 1.7, clientY: -2.5, screenX: 1.2, screenY: 9.9}), ['screenX', 'screenY', 'clientX', 'clientY', 'pageX', 'pageY', 'x', 'y', 'offsetX', 'offsetY', 'layerX', 'layerY']))",
                r#""screenX=1 screenY=9 clientX=1 clientY=-3 pageX=1 pageY=-3 x=1 y=-3 offsetX=2 offsetY=-3 layerX=1 layerY=-3""#,
            ),
            (
                r"E(() => P(new MouseEvent('click', {clientX: 1e10, screenX: -1e10, movementX: 2.5, movementY: -3}), ['clientX', 'screenX', 'movementX', 'movementY']))",
                r#""clientX=10000000000 screenX=-10000000000 movementX=2 movementY=-3""#,
            ),
            (
                r"E(() => [new MouseEvent('click', {button: 70000}).button, new MouseEvent('click', {button: -1}).button, new MouseEvent('click', {buttons: 70000}).buttons, new MouseEvent('click', {buttons: -1}).buttons].join())",
                r#""4464,0,4464,65535""#,
            ),
            (
                r"E(() => new MouseEvent('click', {relatedTarget: 5}))",
                r#""TypeError: Failed to construct 'MouseEvent': Failed to read the 'relatedTarget' property from 'MouseEventInit': Failed to convert value to 'EventTarget'.""#,
            ),
            (
                r"E(() => new MouseEvent('click', {relatedTarget: {}}))",
                r#""TypeError: Failed to construct 'MouseEvent': Failed to read the 'relatedTarget' property from 'MouseEventInit': Failed to convert value to 'EventTarget'.""#,
            ),
            (
                r"E(() => new MouseEvent('click', {relatedTarget: window}).relatedTarget === window)",
                r"true",
            ),
            (
                r"E(() => new MouseEvent('click', {relatedTarget: undefined}).relatedTarget)",
                r"null",
            ),
            (
                r"E(() => [new MouseEvent('x', {button: 0}).which, new MouseEvent('x', {button: 1}).which, new MouseEvent('x', {button: 2}).which, new MouseEvent('x', {button: 4}).which, new MouseEvent('x', {which: 9}).which, new MouseEvent('x', {which: 9, button: 2}).which].join())",
                r#""1,2,3,5,1,3""#,
            ),
            (
                r"E(() => { const e = new MouseEvent('x', {ctrlKey: true, modifierCapsLock: true, modifierAltGraph: true}); return ['Control', 'Shift', 'Alt', 'Meta', 'CapsLock', 'AltGraph', 'NumLock', 'Fn', 'Foo', 'control', ''].map((k) => k + '=' + e.getModifierState(k)).join(' '); })",
                r#""Control=true Shift=false Alt=false Meta=false CapsLock=true AltGraph=true NumLock=false Fn=false Foo=false control=false =false""#,
            ),
            (
                r"E(() => new MouseEvent('x').getModifierState())",
                r#""TypeError: Failed to execute 'getModifierState' on 'MouseEvent': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => MouseEvent.prototype.getModifierState.length)",
                r"1",
            ),
            (
                r"E(() => [MouseEvent.length, MouseEvent.name, C(new MouseEvent('x')), Object.getOwnPropertyNames(MouseEvent.prototype).join()])",
                r#""1,MouseEvent,[object MouseEvent] [object UIEvent] [object Event] [object Object],screenX,screenY,clientX,clientY,ctrlKey,shiftKey,altKey,metaKey,button,buttons,relatedTarget,pageX,pageY,x,y,offsetX,offsetY,movementX,movementY,fromElement,toElement,layerX,layerY,getModifierState,initMouseEvent,constructor""#,
            ),
            (
                r"E(() => { const e = document.createEvent('MouseEvents'); return [e.constructor.name, e.type, e.bubbles, e.cancelable, e.clientX, e.which, e.detail].join(); })",
                r#""MouseEvent,,false,false,0,1,0""#,
            ),
            (
                r#"E(() => { const e = document.createEvent('MouseEvent'); e.initMouseEvent('foo', true, true, window, 2, 10, 20, 30, 40, true, false, true, false, 1, d); return P(e, ["screenX", "screenY", "clientX", "clientY", "pageX", "pageY", "x", "y", "offsetX", "offsetY", "layerX", "layerY", "movementX", "movementY", "ctrlKey", "shiftKey", "altKey", "metaKey", "button", "buttons", "relatedTarget", "fromElement", "toElement", "detail", "which", "view", "type", "bubbles", "cancelable"]); })"#,
                r#""screenX=10 screenY=20 clientX=30 clientY=40 pageX=30 pageY=40 x=30 y=40 offsetX=30 offsetY=40 layerX=30 layerY=40 movementX=0 movementY=0 ctrlKey=true shiftKey=true altKey=false metaKey=false button=1 buttons=0 relatedTarget=[object HTMLDivElement] fromElement=[object HTMLDivElement] toElement=null detail=2 which=2 view=[object Window] type=foo bubbles=true cancelable=true""#,
            ),
            (
                r"E(() => { const e = document.createEvent('MouseEvent'); e.initMouseEvent('foo'); return P(e, ['type', 'clientX', 'button', 'relatedTarget', 'view', 'detail']); })",
                r#""type=foo clientX=0 button=0 relatedTarget=null view=null detail=0""#,
            ),
            (
                r"E(() => { const e = document.createEvent('MouseEvent'); e.initMouseEvent(); })",
                r#""TypeError: Failed to execute 'initMouseEvent' on 'MouseEvent': 1 argument required, but only 0 present.""#,
            ),
            (r"E(() => MouseEvent.prototype.initMouseEvent.length)", r"1"),
            (
                r"E(() => { const e = document.createEvent('MouseEvent'); e.initMouseEvent('foo', false, false, null, 0, 1.9, 2.9, 3.9, 4.9); return P(e, ['screenX', 'screenY', 'clientX', 'clientY']); })",
                r#""screenX=1 screenY=2 clientX=3 clientY=4""#,
            ),
            (
                r"E(() => { const e = new MouseEvent('x', {clientX: 5}); d.dispatchEvent(e); return P(e, ['clientX', 'pageX', 'offsetX', 'layerX', 'x']); })",
                r#""clientX=5 pageX=5 offsetX=-25 layerX=-25 x=5""#,
            ),
            (
                r"E(() => { let r; d.addEventListener('mx', (e) => { r = P(e, ['clientX', 'clientY', 'pageX', 'pageY', 'offsetX', 'offsetY', 'layerX', 'layerY', 'x', 'y', 'fromElement', 'toElement']); }); d.dispatchEvent(new MouseEvent('mx', {clientX: 50, clientY: 60, view: window})); return r; })",
                r#""clientX=50 clientY=60 pageX=50 pageY=60 offsetX=20 offsetY=20 layerX=20 layerY=20 x=50 y=60 fromElement=null toElement=[object HTMLDivElement]""#,
            ),
            (
                r"E(() => { let r; d.addEventListener('mx2', (e) => { r = P(e, ['clientX', 'clientY', 'pageX', 'pageY', 'offsetX', 'offsetY', 'layerX', 'layerY']); }); d.dispatchEvent(new MouseEvent('mx2', {clientX: 50, clientY: 60})); return r; })",
                r#""clientX=50 clientY=60 pageX=50 pageY=60 offsetX=20 offsetY=20 layerX=20 layerY=20""#,
            ),
            (
                r"E(() => { let r; d.addEventListener('mx3', (e) => { r = P(e, ['offsetX', 'offsetY', 'layerX', 'layerY']); }); s.dispatchEvent(new MouseEvent('mx3', {clientX: 50, clientY: 60, view: window})); return r; })",
                r#""undefined""#,
            ),
            (
                r"E(() => { const e = new MouseEvent('over', {relatedTarget: s}); let r; d.addEventListener('over', (ev) => { r = P(ev, ['fromElement', 'toElement', 'relatedTarget']); }); d.dispatchEvent(new MouseEvent('over', {relatedTarget: s})); return r; })",
                r#""fromElement=[object HTMLSpanElement] toElement=[object HTMLDivElement] relatedTarget=[object HTMLSpanElement]""#,
            ),
            (
                r"E(() => { const e = new MouseEvent('mouseout', {relatedTarget: s}); let r; d.addEventListener('mouseout', (ev) => { r = [N(ev.fromElement), N(ev.toElement), N(ev.relatedTarget)].join(); }); d.dispatchEvent(e); return r; })",
                r#""div#d,span#s,span#s""#,
            ),
        ],
    },
    Case {
        name: "pointer-wheel",
        html: PAGE0,
        queries: &[
            (
                r"E(() => P(new PointerEvent('pointerdown'), ['pointerId', 'width', 'height', 'pressure', 'tangentialPressure', 'tiltX', 'tiltY', 'twist', 'altitudeAngle', 'azimuthAngle', 'pointerType', 'isPrimary', 'persistentDeviceId', 'button', 'buttons', 'clientX', 'detail', 'which']))",
                r#""pointerId=0 width=1 height=1 pressure=0 tangentialPressure=0 tiltX=0 tiltY=0 twist=0 altitudeAngle=1.5707963267948966 azimuthAngle=0 pointerType= isPrimary=false persistentDeviceId=0 button=0 buttons=0 clientX=0 detail=0 which=1""#,
            ),
            (
                r"E(() => P(new PointerEvent('pointerdown', {pointerId: 3, width: 2.5, height: 3, pressure: 0.5, tangentialPressure: -0.5, tiltX: 10, tiltY: -20, twist: 90, altitudeAngle: 1, azimuthAngle: 2, pointerType: 'pen', isPrimary: true, button: 1, buttons: 2, clientX: 4.5, bubbles: true}), ['pointerId', 'width', 'height', 'pressure', 'tangentialPressure', 'tiltX', 'tiltY', 'twist', 'altitudeAngle', 'azimuthAngle', 'pointerType', 'isPrimary', 'button', 'buttons', 'clientX', 'bubbles']))",
                r#""pointerId=3 width=2.5 height=3 pressure=0.5 tangentialPressure=-0.5 tiltX=10 tiltY=-20 twist=90 altitudeAngle=1 azimuthAngle=2 pointerType=pen isPrimary=true button=1 buttons=2 clientX=4.5 bubbles=true""#,
            ),
            (
                r"E(() => [new PointerEvent('x', {tiltX: 3.9}).tiltX, new PointerEvent('x', {pressure: 2}).pressure, new PointerEvent('x', {width: -1}).width, new PointerEvent('x', {twist: 400}).twist, new PointerEvent('x', {pointerType: 5}).pointerType].join())",
                r#""3,2,-1,400,5""#,
            ),
            (
                r"E(() => [new PointerEvent('x').getCoalescedEvents().length, new PointerEvent('x').getPredictedEvents().length, Array.isArray(new PointerEvent('x').getCoalescedEvents())].join())",
                r#""0,0,true""#,
            ),
            (
                r"E(() => new PointerEvent('x', {coalescedEvents: [new PointerEvent('y')]}).getCoalescedEvents().length)",
                r"1",
            ),
            (
                r"E(() => new PointerEvent('x', {coalescedEvents: [5]}))",
                r#""TypeError: Failed to construct 'PointerEvent': Failed to read the 'coalescedEvents' property from 'PointerEventInit': Failed to convert value to 'PointerEvent'.""#,
            ),
            (
                r"E(() => [PointerEvent.length, PointerEvent.name, C(new PointerEvent('x')), Object.getOwnPropertyNames(PointerEvent.prototype).join()])",
                r#""1,PointerEvent,[object PointerEvent] [object MouseEvent] [object UIEvent] [object Event] [object Object],pointerId,width,height,pressure,tiltX,tiltY,azimuthAngle,altitudeAngle,tangentialPressure,twist,pointerType,isPrimary,getPredictedEvents,persistentDeviceId,constructor,getCoalescedEvents""#,
            ),
            (
                r"E(() => { try { document.createEvent('PointerEvent'); } catch (e) { return e.name + ': ' + e.message; } })",
                r#""NotSupportedError: Failed to execute 'createEvent' on 'Document': The provided event type ('PointerEvent') is invalid.""#,
            ),
            (
                r"E(() => P(new WheelEvent('wheel'), ['deltaX', 'deltaY', 'deltaZ', 'deltaMode', 'wheelDeltaX', 'wheelDeltaY', 'wheelDelta', 'clientX', 'detail', 'which']))",
                r#""deltaX=0 deltaY=0 deltaZ=0 deltaMode=0 wheelDeltaX=0 wheelDeltaY=0 wheelDelta=0 clientX=0 detail=0 which=1""#,
            ),
            (
                r"E(() => P(new WheelEvent('wheel', {deltaX: 1.5, deltaY: -2, deltaZ: 3, deltaMode: 1, clientX: 3, cancelable: true}), ['deltaX', 'deltaY', 'deltaZ', 'deltaMode', 'wheelDeltaX', 'wheelDeltaY', 'wheelDelta', 'clientX']))",
                r#""deltaX=1.5 deltaY=-2 deltaZ=3 deltaMode=1 wheelDeltaX=1 wheelDeltaY=-2 wheelDelta=-2 clientX=3""#,
            ),
            (
                r"E(() => P(new WheelEvent('wheel', {deltaX: 100, deltaY: 100}), ['wheelDeltaX', 'wheelDeltaY', 'wheelDelta']))",
                r#""wheelDeltaX=100 wheelDeltaY=100 wheelDelta=100""#,
            ),
            (
                r"E(() => P(new WheelEvent('wheel', {deltaX: 100, deltaY: 100, deltaMode: 1}), ['wheelDeltaX', 'wheelDeltaY', 'wheelDelta']))",
                r#""wheelDeltaX=100 wheelDeltaY=100 wheelDelta=100""#,
            ),
            (
                r"E(() => P(new WheelEvent('wheel', {wheelDeltaY: 5, deltaY: 1}), ['wheelDeltaX', 'wheelDeltaY', 'deltaY']))",
                r#""wheelDeltaX=0 wheelDeltaY=5 deltaY=1""#,
            ),
            (
                r"E(() => [WheelEvent.DOM_DELTA_PIXEL, WheelEvent.DOM_DELTA_LINE, WheelEvent.DOM_DELTA_PAGE, WheelEvent.prototype.DOM_DELTA_PAGE, new WheelEvent('x', {deltaMode: 7}).deltaMode].join())",
                r#""0,1,2,2,7""#,
            ),
            (
                r"E(() => { const d = Object.getOwnPropertyDescriptor(WheelEvent, 'DOM_DELTA_LINE'); return [d.writable, d.enumerable, d.configurable].join(); })",
                r#""false,true,false""#,
            ),
            (
                r"E(() => [WheelEvent.length, WheelEvent.name, C(new WheelEvent('x')), Object.getOwnPropertyNames(WheelEvent.prototype).join()])",
                r#""1,WheelEvent,[object WheelEvent] [object MouseEvent] [object UIEvent] [object Event] [object Object],deltaX,deltaY,deltaZ,deltaMode,wheelDeltaX,wheelDeltaY,wheelDelta,DOM_DELTA_PIXEL,DOM_DELTA_LINE,DOM_DELTA_PAGE,constructor""#,
            ),
            (
                r"E(() => { try { document.createEvent('WheelEvent'); } catch (e) { return e.name; } })",
                r#""NotSupportedError""#,
            ),
        ],
    },
    Case {
        name: "keyboard-event",
        html: PAGE0,
        queries: &[
            (
                r#"E(() => P(new KeyboardEvent('keydown'), ["key", "code", "location", "ctrlKey", "shiftKey", "altKey", "metaKey", "repeat", "isComposing", "charCode", "keyCode", "which", "detail", "view", "bubbles"]))"#,
                r#""key= code= location=0 ctrlKey=false shiftKey=false altKey=false metaKey=false repeat=false isComposing=false charCode=0 keyCode=0 which=0 detail=0 view=null bubbles=false""#,
            ),
            (
                r#"E(() => P(new KeyboardEvent('keydown', {key: 'a', code: 'KeyA', location: 1, ctrlKey: true, shiftKey: true, altKey: true, metaKey: true, repeat: true, isComposing: true, charCode: 97, keyCode: 65, which: 66, bubbles: true, detail: 3}), ["key", "code", "location", "ctrlKey", "shiftKey", "altKey", "metaKey", "repeat", "isComposing", "charCode", "keyCode", "which", "detail", "view", "bubbles"]))"#,
                r#""key=a code=KeyA location=1 ctrlKey=true shiftKey=true altKey=true metaKey=true repeat=true isComposing=true charCode=97 keyCode=65 which=65 detail=3 view=null bubbles=true""#,
            ),
            (
                r"E(() => [new KeyboardEvent('keydown', {keyCode: 65}).which, new KeyboardEvent('keydown', {charCode: 97}).which, new KeyboardEvent('keypress', {charCode: 97}).which, new KeyboardEvent('keypress', {charCode: 97, keyCode: 65}).which, new KeyboardEvent('keyup', {which: 5, keyCode: 6}).which, new KeyboardEvent('keydown', {which: 5}).which, new KeyboardEvent('keydown', {which: 5}).keyCode].join())",
                r#""65,0,0,65,6,0,0""#,
            ),
            (
                r"E(() => [new KeyboardEvent('x', {keyCode: -1}).keyCode, new KeyboardEvent('x', {keyCode: 2 ** 32 + 1}).keyCode, new KeyboardEvent('x', {location: 9}).location, new KeyboardEvent('x', {key: null}).key, new KeyboardEvent('x', {key: undefined}).key, new KeyboardEvent('x', {key: 12}).key].join('|'))",
                r#""-1|1|9|null||12""#,
            ),
            (
                r"E(() => { const e = new KeyboardEvent('x', {ctrlKey: true, modifierNumLock: true, modifierFn: true}); return ['Control', 'Shift', 'NumLock', 'Fn', 'CapsLock', 'Foo', 'OS', 'Win', 'Super', 'Hyper', 'Symbol', 'SymbolLock', 'ScrollLock', 'FnLock'].map((k) => k + '=' + e.getModifierState(k)).join(' '); })",
                r#""Control=true Shift=false NumLock=true Fn=true CapsLock=false Foo=false OS=false Win=false Super=false Hyper=false Symbol=false SymbolLock=false ScrollLock=false FnLock=false""#,
            ),
            (
                r"E(() => [KeyboardEvent.length, KeyboardEvent.name, C(new KeyboardEvent('x')), Object.getOwnPropertyNames(KeyboardEvent.prototype).join()])",
                r#""1,KeyboardEvent,[object KeyboardEvent] [object UIEvent] [object Event] [object Object],key,code,location,ctrlKey,shiftKey,altKey,metaKey,repeat,isComposing,charCode,keyCode,DOM_KEY_LOCATION_STANDARD,DOM_KEY_LOCATION_LEFT,DOM_KEY_LOCATION_RIGHT,DOM_KEY_LOCATION_NUMPAD,getModifierState,initKeyboardEvent,constructor""#,
            ),
            (
                r"E(() => [KeyboardEvent.DOM_KEY_LOCATION_STANDARD, KeyboardEvent.DOM_KEY_LOCATION_LEFT, KeyboardEvent.DOM_KEY_LOCATION_RIGHT, KeyboardEvent.DOM_KEY_LOCATION_NUMPAD, KeyboardEvent.prototype.DOM_KEY_LOCATION_NUMPAD].join())",
                r#""0,1,2,3,3""#,
            ),
            (
                r"E(() => { const e = document.createEvent('KeyboardEvent'); return [e.constructor.name, e.type, e.key, e.code, e.keyCode, e.location].join(); })",
                r#""KeyboardEvent,,,,0,0""#,
            ),
            (
                r"E(() => { const e = document.createEvent('KeyboardEvent'); e.initKeyboardEvent('keydown', true, true, window, 'x', 2, true, false, true, false); return P(e, ['type', 'bubbles', 'cancelable', 'view', 'key', 'code', 'location', 'ctrlKey', 'altKey', 'shiftKey', 'metaKey', 'keyCode', 'charCode', 'repeat']); })",
                r#""type=keydown bubbles=true cancelable=true view=[object Window] key= code= location=2 ctrlKey=true altKey=false shiftKey=true metaKey=false keyCode=0 charCode=0 repeat=false""#,
            ),
            (
                r"E(() => { const e = document.createEvent('KeyboardEvent'); e.initKeyboardEvent('keydown'); return P(e, ['type', 'key', 'location', 'ctrlKey']); })",
                r#""type=keydown key= location=0 ctrlKey=false""#,
            ),
            (
                r"E(() => KeyboardEvent.prototype.initKeyboardEvent.length)",
                r"1",
            ),
            (
                r"E(() => { const e = new KeyboardEvent('keydown', {key: 'a', keyCode: 65}); e.initKeyboardEvent('keyup', false, false, null, 'b'); return P(e, ['type', 'key', 'keyCode', 'code']); })",
                r#""type=keyup key=a keyCode=65 code=""#,
            ),
            (
                r"E(() => { let r; d.addEventListener('keydown', (e) => { r = P(e, ['key', 'isTrusted', 'which']); }); d.dispatchEvent(new KeyboardEvent('keydown', {key: 'z', keyCode: 90})); return r; })",
                r#""key=z isTrusted=false which=90""#,
            ),
        ],
    },
    Case {
        name: "focus-input-misc",
        html: PAGE0,
        queries: &[
            (
                r"E(() => P(new FocusEvent('focus'), ['relatedTarget', 'view', 'detail', 'which', 'bubbles']))",
                r#""relatedTarget=null view=null detail=0 which=0 bubbles=false""#,
            ),
            (
                r"E(() => N(new FocusEvent('focus', {relatedTarget: d, bubbles: true}).relatedTarget))",
                r#""div#d""#,
            ),
            (
                r"E(() => new FocusEvent('focus', {relatedTarget: 5}))",
                r#""TypeError: Failed to construct 'FocusEvent': Failed to read the 'relatedTarget' property from 'FocusEventInit': Failed to convert value to 'EventTarget'.""#,
            ),
            (
                r"E(() => [FocusEvent.length, FocusEvent.name, C(new FocusEvent('x')), Object.getOwnPropertyNames(FocusEvent.prototype).join()])",
                r#""1,FocusEvent,[object FocusEvent] [object UIEvent] [object Event] [object Object],relatedTarget,constructor""#,
            ),
            (
                r"E(() => { try { document.createEvent('FocusEvent').relatedTarget; return document.createEvent('FocusEvent').constructor.name; } catch (e) { return e.name; } })",
                r#""FocusEvent""#,
            ),
            (
                r"E(() => P(new InputEvent('input'), ['data', 'isComposing', 'inputType', 'dataTransfer', 'view', 'detail', 'which']))",
                r#""data=null isComposing=false inputType= dataTransfer=null view=null detail=0 which=0""#,
            ),
            (
                r"E(() => P(new InputEvent('input', {data: 'x', isComposing: true, inputType: 'insertText', bubbles: true}), ['data', 'isComposing', 'inputType']))",
                r#""data=x isComposing=true inputType=insertText""#,
            ),
            (
                r"E(() => [new InputEvent('x', {data: null}).data, new InputEvent('x', {data: undefined}).data, new InputEvent('x', {data: 5}).data, new InputEvent('x', {data: '\ud800'}).data.length].join('|'))",
                r#""||5|1""#,
            ),
            (
                r"E(() => [InputEvent.length, InputEvent.name, C(new InputEvent('x')), Object.getOwnPropertyNames(InputEvent.prototype).join()])",
                r#""1,InputEvent,[object InputEvent] [object UIEvent] [object Event] [object Object],data,isComposing,inputType,dataTransfer,getTargetRanges,constructor""#,
            ),
            (
                r"E(() => new InputEvent('x').getTargetRanges().length)",
                r"0",
            ),
            (
                r"E(() => P(new CompositionEvent('compositionstart'), ['data', 'view', 'detail', 'which']))",
                r#""data= view=null detail=0 which=0""#,
            ),
            (
                r"E(() => P(new CompositionEvent('compositionstart', {data: 'ab'}), ['data']))",
                r#""data=ab""#,
            ),
            (
                r"E(() => { const e = document.createEvent('CompositionEvent'); e.initCompositionEvent('compositionend', true, false, window, 'zz'); return P(e, ['type', 'bubbles', 'data', 'view']); })",
                r#""type=compositionend bubbles=true data=zz view=[object Window]""#,
            ),
            (
                r"E(() => [CompositionEvent.length, CompositionEvent.name, C(new CompositionEvent('x')), Object.getOwnPropertyNames(CompositionEvent.prototype).join()])",
                r#""1,CompositionEvent,[object CompositionEvent] [object UIEvent] [object Event] [object Object],data,initCompositionEvent,constructor""#,
            ),
            (
                r"E(() => { const e = document.createEvent('TextEvent'); e.initTextEvent('textInput', true, true, window, 'hi'); return [e.constructor.name, e.type, e.data, e.bubbles].join(); })",
                r#""TextEvent,textInput,hi,true""#,
            ),
            (
                r"E(() => { const e = document.createEvent('TextEvent'); return [e.data, Object.getOwnPropertyNames(TextEvent.prototype).join(), C(e)].join('|'); })",
                r#""|data,initTextEvent,constructor|[object TextEvent] [object UIEvent] [object Event] [object Object]""#,
            ),
            (
                r"E(() => { const e = document.createEvent('TextEvent'); e.initTextEvent('x'); return e.data; })",
                r#""undefined""#,
            ),
            (
                r"E(() => E(() => new TextEvent('x')))",
                r#""TypeError: Failed to construct 'TextEvent': Illegal constructor""#,
            ),
        ],
    },
    Case {
        name: "other-events",
        html: PAGE0,
        queries: &[
            (
                r"E(() => P(new SubmitEvent('submit'), ['submitter', 'bubbles', 'cancelable']))",
                r#""submitter=null bubbles=false cancelable=false""#,
            ),
            (
                r"E(() => N(new SubmitEvent('submit', {submitter: b}).submitter))",
                r#""button#b""#,
            ),
            (
                r"E(() => new SubmitEvent('submit', {submitter: d}).submitter === d)",
                r"true",
            ),
            (
                r"E(() => new SubmitEvent('submit', {submitter: document}))",
                r#""TypeError: Failed to construct 'SubmitEvent': Failed to read the 'submitter' property from 'SubmitEventInit': Failed to convert value to 'HTMLElement'.""#,
            ),
            (
                r"E(() => new SubmitEvent('submit', {submitter: 5}))",
                r#""TypeError: Failed to construct 'SubmitEvent': Failed to read the 'submitter' property from 'SubmitEventInit': Failed to convert value to 'HTMLElement'.""#,
            ),
            (
                r"E(() => [SubmitEvent.length, SubmitEvent.name, C(new SubmitEvent('x')), Object.getOwnPropertyNames(SubmitEvent.prototype).join()])",
                r#""1,SubmitEvent,[object SubmitEvent] [object Event] [object Object],submitter,constructor""#,
            ),
            (
                r"E(() => P(new HashChangeEvent('hashchange'), ['oldURL', 'newURL']))",
                r#""oldURL= newURL=""#,
            ),
            (
                r"E(() => P(new HashChangeEvent('hashchange', {oldURL: 'a', newURL: 'b'}), ['oldURL', 'newURL']))",
                r#""oldURL=a newURL=b""#,
            ),
            (
                r"E(() => { const e = document.createEvent('HashChangeEvent'); e.initHashChangeEvent('hashchange', true, false, 'o', 'n'); return P(e, ['type', 'bubbles', 'oldURL', 'newURL']); })",
                r#""TypeError: e.initHashChangeEvent is not a function""#,
            ),
            (
                r"E(() => Object.getOwnPropertyNames(HashChangeEvent.prototype).join())",
                r#""oldURL,newURL,constructor""#,
            ),
            (
                r"E(() => P(new MessageEvent('message'), ['data', 'origin', 'lastEventId', 'source', 'ports']))",
                r#""data=null origin= lastEventId= source=null ports=""#,
            ),
            (
                r"E(() => P(new MessageEvent('message', {data: {a: 1}, origin: 'o', lastEventId: 'l', source: window, ports: []}), ['origin', 'lastEventId', 'source', 'ports']))",
                r#""origin=o lastEventId=l source=[object Window] ports=""#,
            ),
            (
                r"E(() => new MessageEvent('message', {data: 5}).data)",
                r"5",
            ),
            (
                r"E(() => new MessageEvent('message', {source: 5}))",
                r#""TypeError: Failed to construct 'MessageEvent': Failed to read the 'source' property from 'MessageEventInit': Failed to convert value to 'EventTarget'.""#,
            ),
            (
                r"E(() => new MessageEvent('message', {ports: [1]}))",
                r#""TypeError: Failed to construct 'MessageEvent': Failed to read the 'ports' property from 'MessageEventInit': Failed to convert value to 'MessagePort'.""#,
            ),
            (
                r"E(() => Object.isFrozen(new MessageEvent('message').ports))",
                r"true",
            ),
            (
                r"E(() => { const e = document.createEvent('MessageEvent'); e.initMessageEvent('message', true, false, 'dd', 'oo', 'll', null, []); return P(e, ['type', 'data', 'origin', 'lastEventId', 'source']); })",
                r#""type=message data=dd origin=oo lastEventId=ll source=null""#,
            ),
            (
                r"E(() => Object.getOwnPropertyNames(MessageEvent.prototype).join())",
                r#""data,origin,lastEventId,source,ports,userActivation,initMessageEvent,constructor""#,
            ),
            (
                r"E(() => P(new StorageEvent('storage'), ['key', 'oldValue', 'newValue', 'url', 'storageArea']))",
                r#""key=null oldValue=null newValue=null url= storageArea=null""#,
            ),
            (
                r"E(() => P(new StorageEvent('storage', {key: 'k', oldValue: 'o', newValue: 'n', url: 'u'}), ['key', 'oldValue', 'newValue', 'url']))",
                r#""key=k oldValue=o newValue=n url=u""#,
            ),
            (
                r"E(() => { const e = document.createEvent('StorageEvent'); e.initStorageEvent('storage', true, false, 'k', 'o', 'n', 'u', null); return P(e, ['type', 'key', 'oldValue', 'newValue', 'url']); })",
                r#""type=storage key=k oldValue=o newValue=n url=u""#,
            ),
            (
                r"E(() => Object.getOwnPropertyNames(StorageEvent.prototype).join())",
                r#""key,oldValue,newValue,url,storageArea,initStorageEvent,constructor""#,
            ),
            (
                r"E(() => { const e = document.createEvent('BeforeUnloadEvent'); return [e.constructor.name, e.returnValue, C(e), Object.getOwnPropertyNames(BeforeUnloadEvent.prototype).join(), e.cancelable, e.type].join('|'); })",
                r#""BeforeUnloadEvent||[object BeforeUnloadEvent] [object Event] [object Object]|returnValue,constructor|false|""#,
            ),
            (
                r"E(() => new BeforeUnloadEvent('x'))",
                r#""TypeError: Failed to construct 'BeforeUnloadEvent': Illegal constructor""#,
            ),
            (
                r"E(() => { const e = document.createEvent('BeforeUnloadEvent'); e.returnValue = 'abc'; return [e.returnValue, e.defaultPrevented].join(); })",
                r#""abc,false""#,
            ),
            (
                r"E(() => { const e = document.createEvent('DeviceMotionEvent'); return [e.constructor.name, e.acceleration, e.accelerationIncludingGravity, e.rotationRate, e.interval, Object.getOwnPropertyNames(DeviceMotionEvent.prototype).join()].join('|'); })",
                r#""DeviceMotionEvent||||0|acceleration,accelerationIncludingGravity,rotationRate,interval,constructor""#,
            ),
            (
                r"E(() => { const e = document.createEvent('DeviceOrientationEvent'); return [e.constructor.name, e.alpha, e.beta, e.gamma, e.absolute, Object.getOwnPropertyNames(DeviceOrientationEvent.prototype).join()].join('|'); })",
                r#""DeviceOrientationEvent||||false|alpha,beta,gamma,absolute,constructor""#,
            ),
            (
                r"E(() => { const e = new DeviceOrientationEvent('x', {alpha: 1, beta: 2, gamma: 3, absolute: true}); return [e.alpha, e.beta, e.gamma, e.absolute].join(); })",
                r#""1,2,3,true""#,
            ),
            (
                r"E(() => { const e = new DeviceMotionEvent('x', {interval: 5}); return [e.interval, e.acceleration].join(); })",
                r#""5,""#,
            ),
            (
                r"E(() => { const e = document.createEvent('DragEvent'); return [e.constructor.name, e.dataTransfer, C(e), Object.getOwnPropertyNames(DragEvent.prototype).join(), e.clientX].join('|'); })",
                r#""DragEvent||[object DragEvent] [object MouseEvent] [object UIEvent] [object Event] [object Object]|dataTransfer,constructor|0""#,
            ),
            (
                r"E(() => [new DragEvent('drag').dataTransfer, new DragEvent('drag', {clientX: 5}).clientX].join())",
                r#"",5""#,
            ),
            (
                r"E(() => { const e = document.createEvent('TransitionEvent'); return [e.constructor.name, e.propertyName, e.elapsedTime, e.pseudoElement].join('|'); })",
                r#""TransitionEvent||0|""#,
            ),
            (
                r"E(() => { const e = new TransitionEvent('transitionend', {propertyName: 'width', elapsedTime: 1.5, pseudoElement: '::before'}); return [e.propertyName, e.elapsedTime, e.pseudoElement].join(); })",
                r#""width,1.5,::before""#,
            ),
            (
                r"E(() => ['Event','Events','HTMLEvents','SVGEvents','CustomEvent','UIEvent','UIEvents','MouseEvent','MouseEvents','KeyboardEvent','FocusEvent','TextEvent','CompositionEvent','DragEvent','HashChangeEvent','MessageEvent','StorageEvent','BeforeUnloadEvent','DeviceMotionEvent','DeviceOrientationEvent','TransitionEvent','TouchEvent','PointerEvent','WheelEvent','InputEvent','SubmitEvent','ErrorEvent','ClipboardEvent','AnimationEvent','ProgressEvent','PopStateEvent','CloseEvent','PageTransitionEvent','KeyEvents','MutationEvent','MutationEvents','TrackEvent','XYZ','mouseevent','MOUSEEVENTS','keyboardevent','htmlevents','uievents','textevent','Mouseevent '].map(n => { try { const e = document.createEvent(n); return n + '=' + Object.getPrototypeOf(e).constructor.name; } catch (x) { return n + '!' + x.name; } }).join(' '))",
                r#""Event=Event Events=Event HTMLEvents=Event SVGEvents=Event CustomEvent=CustomEvent UIEvent=UIEvent UIEvents=UIEvent MouseEvent=MouseEvent MouseEvents=MouseEvent KeyboardEvent=KeyboardEvent FocusEvent=FocusEvent TextEvent=TextEvent CompositionEvent=CompositionEvent DragEvent=DragEvent HashChangeEvent=HashChangeEvent MessageEvent=MessageEvent StorageEvent=StorageEvent BeforeUnloadEvent=BeforeUnloadEvent DeviceMotionEvent=DeviceMotionEvent DeviceOrientationEvent=DeviceOrientationEvent TransitionEvent=TransitionEvent TouchEvent!NotSupportedError PointerEvent!NotSupportedError WheelEvent!NotSupportedError InputEvent!NotSupportedError SubmitEvent!NotSupportedError ErrorEvent!NotSupportedError ClipboardEvent!NotSupportedError AnimationEvent!NotSupportedError ProgressEvent!NotSupportedError PopStateEvent!NotSupportedError CloseEvent!NotSupportedError PageTransitionEvent!NotSupportedError KeyEvents!NotSupportedError MutationEvent!NotSupportedError MutationEvents!NotSupportedError TrackEvent!NotSupportedError XYZ!NotSupportedError mouseevent=MouseEvent MOUSEEVENTS=MouseEvent keyboardevent=KeyboardEvent htmlevents=Event uievents=UIEvent textevent=TextEvent Mouseevent !NotSupportedError""#,
            ),
            (
                r"E(() => ['MouseEvents', 'KeyboardEvent', 'UIEvents', 'FocusEvent'].map((n) => { const e = document.createEvent(n); return [n, e.type === '', e.bubbles, e.cancelable, e.composed, e.isTrusted, e.eventPhase, e.defaultPrevented].join(); }).join(' '))",
                r#""MouseEvents,true,false,false,false,false,0,false KeyboardEvent,true,false,false,false,false,0,false UIEvents,true,false,false,false,false,0,false FocusEvent,true,false,false,false,false,0,false""#,
            ),
            (
                r"E(() => { const e = document.createEvent('MouseEvents'); d.dispatchEvent(e); })",
                r#""DOMException InvalidStateError: Failed to execute 'dispatchEvent' on 'EventTarget': The event provided is uninitialized.""#,
            ),
            (
                r"E(() => { const e = document.createEvent('MouseEvents'); e.initEvent('foo', true, true); let r; d.addEventListener('foo', (ev) => { r = ev === e; }); d.dispatchEvent(e); return r + ' ' + e.constructor.name; })",
                r#""true MouseEvent""#,
            ),
            (
                r"E(() => document.createEvent())",
                r#""TypeError: Failed to execute 'createEvent' on 'Document': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => document.createEvent(undefined))",
                r#""DOMException NotSupportedError: Failed to execute 'createEvent' on 'Document': The provided event type ('undefined') is invalid.""#,
            ),
            (
                r"E(() => document.createEvent(null))",
                r#""DOMException NotSupportedError: Failed to execute 'createEvent' on 'Document': The provided event type ('null') is invalid.""#,
            ),
        ],
    },
    Case {
        name: "mouse-more",
        html: r"<!DOCTYPE html><body style='margin:0'><div id=tall style='height:2000px;width:3000px'></div><div id=d style='position:absolute;left:30px;top:40px;width:100px;height:50px'><span id=s>x</span></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(x) { if (x === null) return 'null'; if (x === undefined) return 'undefined'; if (x === window) return 'window'; if (x === document) return 'document'; if (x.nodeType === 1) return x.localName + (x.id ? '#' + x.id : ''); if (x.nodeType) return '#' + x.nodeType; return 'target'; }
function C(o) { const r = []; for (let p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) r.push(Object.prototype.toString.call(p)); return r.join(' '); }
function P(o, names) { return names.map((n) => n + '=' + (typeof o[n] === 'function' ? 'fn' : String(o[n]))).join(' '); }
</script></body>",
        queries: &[
            (
                r"[new WheelEvent('w', {wheelDeltaX: 5, deltaY: 3}), new WheelEvent('w', {wheelDeltaX: 5, wheelDeltaY: 7}), new WheelEvent('w', {deltaX: 4, deltaY: 0}), new WheelEvent('w', {deltaX: -4, deltaY: 0.6}), new WheelEvent('w', {deltaY: 2e10})].map((e) => P(e, ['wheelDeltaX', 'wheelDeltaY', 'wheelDelta'])).join(' | ')",
                r#""wheelDeltaX=5 wheelDeltaY=3 wheelDelta=3 | wheelDeltaX=5 wheelDeltaY=7 wheelDelta=7 | wheelDeltaX=4 wheelDeltaY=0 wheelDelta=4 | wheelDeltaX=-4 wheelDeltaY=0 wheelDelta=-4 | wheelDeltaX=0 wheelDeltaY=2147483647 wheelDelta=2147483647""#,
            ),
            (
                r"[new MouseEvent('x', {button: -2}).button, new MouseEvent('x', {button: -2}).which, new MouseEvent('x', {button: 3}).which, new MouseEvent('x', {button: 70000}).which, new MouseEvent('x', {button: 65535}).button].join()",
                r#""-2,4294967295,4,4465,0""#,
            ),
            (
                r"(() => { const r = []; for (const t of ['mouseover', 'mouseout', 'mouseenter', 'mouseleave', 'pointerover', 'pointerout', 'dragleave', 'dragenter', 'click', 'blur', 'focusout']) { let v; d.addEventListener(t, (e) => { v = t + ':' + [e.fromElement, e.toElement].map((n) => n ? n.id : 'null').join('/'); }); d.dispatchEvent(new MouseEvent(t, {relatedTarget: s})); r.push(v); } return r.join(' '); })()",
                r#""mouseover:s/d mouseout:d/s mouseenter:s/d mouseleave:d/s pointerover:s/d pointerout:s/d dragleave:s/d dragenter:s/d click:s/d blur:s/d focusout:s/d""#,
            ),
            (
                r"(() => { const r = []; for (const t of ['mouseover', 'mouseout']) { let v; s.addEventListener(t, (e) => { v = t + ':' + [e.fromElement, e.toElement].map((n) => n ? n.id : 'null').join('/'); }); s.dispatchEvent(new MouseEvent(t, {relatedTarget: window})); r.push(v); } return r.join(' '); })()",
                r#""mouseover:null/s mouseout:s/null""#,
            ),
            (
                r"(() => { const e = new MouseEvent('x', {relatedTarget: d}); return [e.fromElement && e.fromElement.id, e.toElement].join(); })()",
                r#""d,""#,
            ),
            (
                r"(() => { const e = new MouseEvent('mouseout', {relatedTarget: d}); return [e.fromElement && e.fromElement.id, e.toElement].join(); })()",
                r#"",[object HTMLDivElement]""#,
            ),
            (
                r"(() => { const e = new MouseEvent('x', {relatedTarget: window}); return [e.fromElement, e.toElement, e.relatedTarget === window].join(); })()",
                r#"",,true""#,
            ),
            (
                r"(() => { const e = new KeyboardEvent('x', {charCode: -5, keyCode: 3.9}); return [e.charCode, e.keyCode, e.which].join(); })()",
                r#""-5,3,3""#,
            ),
            (
                r"(() => { const e = new KeyboardEvent('keypress', {key: 'a', charCode: 97}); const f = new KeyboardEvent('keydown', {key: 'A', code: 'KeyA', keyCode: 65}); return [e.which, f.which, e.charCode, f.charCode].join(); })()",
                r#""0,65,97,0""#,
            ),
            (
                r"(() => { const e = new PointerEvent('x', {altitudeAngle: 0.5}); const f = new PointerEvent('x', {azimuthAngle: 0.5}); const g = new PointerEvent('x', {tiltX: 30}); return [e.tiltX, e.tiltY, e.azimuthAngle, f.altitudeAngle, f.azimuthAngle, g.altitudeAngle, g.azimuthAngle, g.tiltX].join(); })()",
                r#""61,0,0,1.5707963267948966,0.5,1.0471975511965979,0,30""#,
            ),
            (
                r"(() => { const e = new PointerEvent('x', {pointerType: 'mouse', button: 0, buttons: 1, isPrimary: true}); return [e.pointerType, e.pressure, e.width, e.height, e.which, e.detail].join(); })()",
                r#""mouse,0,1,1,1,0""#,
            ),
            (
                r"(() => { const e = new UIEvent('x', {detail: 1}); return [Object.prototype.hasOwnProperty.call(e, 'detail'), Object.keys(e).length, JSON.stringify(e), 'isTrusted' in e, Object.getOwnPropertyNames(e).join()].join('|'); })()",
                r#""false|1|{\"isTrusted\":false}|true|isTrusted""#,
            ),
            (
                r"(() => { const e = document.createEvent('MouseEvents'); e.initMouseEvent('click', true, true, window, 1, 1.9, 2.9, 3.9, 4.9, false, false, false, false, 2, null); return P(e, ['screenX', 'clientX', 'button', 'detail', 'buttons', 'which']); })()",
                r#""screenX=1 clientX=3 button=2 detail=1 buttons=0 which=3""#,
            ),
            (
                r"(() => { const e = new KeyboardEvent('keydown', {key: 'a', keyCode: 65}); e.initKeyboardEvent('keyup', true, true, window, 'b', 1, true, true, true, true); return P(e, ['type', 'key', 'keyCode', 'ctrlKey', 'altKey', 'shiftKey', 'metaKey', 'location', 'bubbles']); })()",
                r#""type=keyup key=a keyCode=65 ctrlKey=true altKey=true shiftKey=true metaKey=true location=1 bubbles=true""#,
            ),
        ],
    },
];
