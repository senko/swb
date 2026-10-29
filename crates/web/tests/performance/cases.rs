// The cases of tools/probes/web-performance.json with the values of Chromium 148.
// Generated from `just probe --json` (three runs, all equal); do not edit.

use crate::common::Case;

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "structure",
        html: r"<!DOCTYPE html><html><head></head><body><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function U(a) { return Array.from(a).join(); }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
</script></body></html>",
        queries: &[
            (
                r"[typeof Performance, typeof PerformanceEntry, typeof PerformanceMark, typeof PerformanceMeasure, typeof PerformanceObserver, typeof PerformanceObserverEntryList, typeof PerformanceTiming, typeof PerformanceNavigation, typeof PerformanceNavigationTiming, typeof PerformanceResourceTiming]",
                r#"["function", "function", "function", "function", "function", "function", "function", "function", "function", "function"]"#,
            ),
            (
                r"[Object.getPrototypeOf(PerformanceMark).name, Object.getPrototypeOf(PerformanceMeasure).name, Object.getPrototypeOf(PerformanceNavigationTiming).name, Object.getPrototypeOf(PerformanceResourceTiming).name, Object.getPrototypeOf(Performance).name]",
                r#"["PerformanceEntry", "PerformanceEntry", "PerformanceResourceTiming", "PerformanceEntry", "EventTarget"]"#,
            ),
            (
                r"[Object.getOwnPropertyNames(PerformanceEntry.prototype).sort().join(), Object.getOwnPropertyNames(PerformanceMark.prototype).sort().join(), Object.getOwnPropertyNames(PerformanceMeasure.prototype).sort().join(), Object.getOwnPropertyNames(PerformanceObserver.prototype).sort().join(), Object.getOwnPropertyNames(PerformanceObserverEntryList.prototype).sort().join()]",
                r#"["constructor,duration,entryType,name,startTime,toJSON", "constructor,detail", "constructor,detail", "constructor,disconnect,observe,takeRecords", "constructor,getEntries,getEntriesByName,getEntriesByType"]"#,
            ),
            (
                r"[Object.getOwnPropertyNames(PerformanceTiming.prototype).sort().join(), Object.getOwnPropertyNames(PerformanceNavigation.prototype).sort().join()]",
                r#"["connectEnd,connectStart,constructor,domComplete,domContentLoadedEventEnd,domContentLoadedEventStart,domInteractive,domLoading,domainLookupEnd,domainLookupStart,fetchStart,loadEventEnd,loadEventStart,navigationStart,redirectEnd,redirectStart,requestStart,responseEnd,responseStart,secureConnectionStart,toJSON,unloadEventEnd,unloadEventStart", "TYPE_BACK_FORWARD,TYPE_NAVIGATE,TYPE_RELOAD,TYPE_RESERVED,constructor,redirectCount,toJSON,type"]"#,
            ),
            (
                r"Object.getOwnPropertyNames(Performance.prototype).filter((n) => !['memory', 'eventCounts', 'interactionCount', 'onresourcetimingbufferfull'].includes(n)).sort().join()",
                r#""clearMarks,clearMeasures,clearResourceTimings,constructor,getEntries,getEntriesByName,getEntriesByType,mark,measure,navigation,now,setResourceTimingBufferSize,timeOrigin,timing,toJSON""#,
            ),
            (
                r"E(() => new PerformanceEntry())",
                r#""TypeError: Failed to construct 'PerformanceEntry': Illegal constructor""#,
            ),
            (
                r"E(() => new PerformanceMeasure())",
                r#""TypeError: Failed to construct 'PerformanceMeasure': Illegal constructor""#,
            ),
            (
                r"E(() => new Performance())",
                r#""TypeError: Failed to construct 'Performance': Illegal constructor""#,
            ),
            (
                r"E(() => new PerformanceTiming())",
                r#""TypeError: Failed to construct 'PerformanceTiming': Illegal constructor""#,
            ),
            (
                r"E(() => new PerformanceNavigation())",
                r#""TypeError: Failed to construct 'PerformanceNavigation': Illegal constructor""#,
            ),
            (
                r"E(() => new PerformanceNavigationTiming())",
                r#""TypeError: Failed to construct 'PerformanceNavigationTiming': Illegal constructor""#,
            ),
            (
                r"E(() => new PerformanceObserverEntryList())",
                r#""TypeError: Failed to construct 'PerformanceObserverEntryList': Illegal constructor""#,
            ),
            (
                r"E(() => PerformanceMark('x'))",
                r#""TypeError: Failed to construct 'PerformanceMark': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"[String(performance), String(PerformanceMark.prototype), String(performance.timing), String(performance.navigation), Object.prototype.toString.call(new PerformanceObserver(() => {}))]",
                r#"["[object Performance]", "[object PerformanceMark]", "[object PerformanceTiming]", "[object PerformanceNavigation]", "[object PerformanceObserver]"]"#,
            ),
            (
                r"[PerformanceMark.length, PerformanceMark.name, PerformanceObserver.length, PerformanceNavigation.TYPE_NAVIGATE, PerformanceNavigation.TYPE_RELOAD, PerformanceNavigation.TYPE_BACK_FORWARD, PerformanceNavigation.TYPE_RESERVED, performance.navigation.TYPE_RELOAD]",
                r#"[1, "PerformanceMark", 1, 0, 1, 2, 255, 1]"#,
            ),
            (
                r"[performance instanceof EventTarget, typeof Object.getOwnPropertyDescriptor(window, 'performance').set, Object.getOwnPropertyDescriptor(window, 'performance').enumerable, Object.getOwnPropertyDescriptor(Performance.prototype, 'timing').get.name, Object.getOwnPropertyDescriptor(Performance.prototype, 'timing').set]",
                r#"[true, "function", true, "get timing", null]"#,
            ),
            (
                r"[performance.timing === performance.timing, performance.navigation === performance.navigation, performance === window.performance]",
                r"[true, true, true]",
            ),
            (
                r"E(() => { performance.timing = 5; return typeof performance.timing; })",
                r#""object""#,
            ),
            (
                r"E(() => performance.now.call({}))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(PerformanceEntry.prototype, 'name').get.call({}))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => PerformanceEntry.prototype.toJSON.call({}))",
                r#""TypeError: Illegal invocation""#,
            ),
        ],
    },
    Case {
        name: "now",
        html: r"<!DOCTYPE html><html><head></head><body><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function U(a) { return Array.from(a).join(); }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
</script></body></html>",
        queries: &[
            (
                r"(() => { const a = performance.now(); const b = performance.now(); return [typeof a, b >= a, a >= 0, performance.now.length, typeof performance.timeOrigin, performance.timeOrigin > 1e12]; })()",
                r#"["number", true, true, 0, "number", true]"#,
            ),
            (
                r"Math.abs(performance.timeOrigin + performance.now() - Date.now()) < 5000",
                r"true",
            ),
            (
                r"Object.keys(performance.toJSON()).join()",
                r#""timeOrigin,timing,navigation""#,
            ),
            (
                r"(() => { const j = performance.toJSON(); return [typeof j.timeOrigin, j.timing === performance.timing, j.navigation === performance.navigation, JSON.stringify(j).includes('navigationStart')]; })()",
                r#"["number", true, true, true]"#,
            ),
            (
                r"(() => { const t = performance.timing; return [t.navigationStart > 1e12, t.navigationStart === Math.floor(performance.timeOrigin), t.fetchStart >= t.navigationStart, t.responseEnd >= t.fetchStart, t.domLoading >= t.navigationStart, t.domInteractive >= t.domLoading, t.domComplete >= t.domInteractive, t.loadEventEnd >= t.loadEventStart, t.loadEventStart >= t.domContentLoadedEventEnd, t.redirectStart, t.redirectEnd, t.secureConnectionStart]; })()",
                r"[true, true, true, true, true, true, true, true, true, 0, 0, 0]",
            ),
            (
                r"Object.keys(performance.timing.toJSON()).sort().join()",
                r#""connectEnd,connectStart,domComplete,domContentLoadedEventEnd,domContentLoadedEventStart,domInteractive,domLoading,domainLookupEnd,domainLookupStart,fetchStart,loadEventEnd,loadEventStart,navigationStart,redirectEnd,redirectStart,requestStart,responseEnd,responseStart,secureConnectionStart,unloadEventEnd,unloadEventStart""#,
            ),
            (
                r"Object.entries(performance.timing.toJSON()).filter(([k, v]) => v === 0 && !k.startsWith('unload')).map(([k]) => k).sort().join()",
                r#""redirectEnd,redirectStart,secureConnectionStart""#,
            ),
            (
                r"(() => { const t = performance.timing; return Object.keys(Object.getPrototypeOf(t)).length; })()",
                r"22",
            ),
            (
                r"[performance.navigation.type, performance.navigation.redirectCount, Object.keys(performance.navigation.toJSON()).join(), JSON.stringify(performance.navigation)]",
                r#"[0, 0, "type,redirectCount", "{\"type\":0,\"redirectCount\":0}"]"#,
            ),
            (
                r"E(() => performance.setResourceTimingBufferSize())",
                r#""TypeError: Failed to execute 'setResourceTimingBufferSize' on 'Performance': 1 argument required, but only 0 present.""#,
            ),
            (
                r"[performance.setResourceTimingBufferSize(10), performance.clearResourceTimings(), performance.getEntriesByType('resource').length]",
                r"[null, null, 0]",
            ),
        ],
    },
    Case {
        name: "navigation-entry",
        html: r"<!DOCTYPE html><html><head></head><body><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function U(a) { return Array.from(a).join(); }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
</script></body></html>",
        queries: &[
            (
                r"(() => { const e = performance.getEntriesByType('navigation'); return [e.length, e[0].constructor.name, e[0] === performance.getEntriesByType('navigation')[0], e[0] === performance.getEntries()[0], performance.getEntriesByName(location.href)[0] === e[0], performance.getEntriesByName(location.href, 'navigation').length, performance.getEntriesByName(location.href, 'mark').length]; })()",
                r#"[1, "PerformanceNavigationTiming", true, true, true, 1, 0]"#,
            ),
            (
                r"(() => { const n = performance.getEntriesByType('navigation')[0]; return [n.entryType, n.name === location.href, n.startTime, n.type, n.redirectCount, n.initiatorType, n.responseStatus, n.transferSize > 0, n.encodedBodySize, n.decodedBodySize, n.renderBlockingStatus, n.contentType, n.workerStart, n.criticalCHRestart, n.deliveryType, n.contentEncoding, n.redirectStart, n.redirectEnd]; })()",
                r#"["navigation", true, 0, "navigate", 0, "navigation", 200, true, 433, 433, "non-blocking", "text/html", 0, 0, "", "", 0, 0]"#,
            ),
            (
                r"(() => { const n = performance.getEntriesByType('navigation')[0]; return [n.domainLookupStart >= n.fetchStart, n.domainLookupEnd >= n.domainLookupStart, n.connectStart >= n.domainLookupEnd, n.connectEnd >= n.connectStart, n.secureConnectionStart, n.requestStart >= n.fetchStart, n.responseStart >= n.requestStart, n.responseEnd >= n.responseStart]; })()",
                r"[true, true, true, true, 0, true, true, true]",
            ),
            (
                r"(() => { const n = performance.getEntriesByType('navigation')[0]; return [n.domInteractive >= 0, n.domContentLoadedEventStart >= n.domInteractive, n.domContentLoadedEventEnd >= n.domContentLoadedEventStart, n.domComplete >= n.domContentLoadedEventEnd, n.loadEventStart >= n.domComplete, n.loadEventEnd >= n.loadEventStart, n.duration === n.loadEventEnd, n.duration >= 0]; })()",
                r"[true, true, true, true, true, true, true, true]",
            ),
            (
                r"Object.keys(performance.getEntriesByType('navigation')[0].toJSON()).slice(0, 4).join()",
                r#""name,entryType,startTime,duration""#,
            ),
            (
                r"(() => { const j = performance.getEntriesByType('navigation')[0].toJSON(); return [j.entryType, j.name === location.href, j.type, j.duration >= 0, 'domInteractive' in j, 'loadEventEnd' in j, 'transferSize' in j]; })()",
                r#"["navigation", true, "navigate", true, true, true, true]"#,
            ),
            (
                r"Object.getOwnPropertyNames(PerformanceNavigationTiming.prototype).filter((n) => !['activationStart', 'confidence', 'notRestoredReasons'].includes(n)).sort().join()",
                r#""constructor,criticalCHRestart,domComplete,domContentLoadedEventEnd,domContentLoadedEventStart,domInteractive,loadEventEnd,loadEventStart,redirectCount,toJSON,type,unloadEventEnd,unloadEventStart""#,
            ),
            (
                r"Object.getOwnPropertyNames(PerformanceResourceTiming.prototype).filter((n) => !/^worker(Router|Cache|Matched|Final)|serverTiming/.test(n)).sort().join()",
                r#""connectEnd,connectStart,constructor,contentEncoding,contentType,decodedBodySize,deliveryType,domainLookupEnd,domainLookupStart,encodedBodySize,fetchStart,finalResponseHeadersStart,firstInterimResponseStart,initiatorType,nextHopProtocol,redirectEnd,redirectStart,renderBlockingStatus,requestStart,responseEnd,responseStart,responseStatus,secureConnectionStart,toJSON,transferSize,workerStart""#,
            ),
            (
                r"performance.getEntries().map((e) => e.entryType).join()",
                r#""navigation,visibility-state""#,
            ),
        ],
    },
    Case {
        name: "navigation-departures",
        html: r"<!DOCTYPE html><html><head></head><body><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function U(a) { return Array.from(a).join(); }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
</script></body></html>",
        queries: &[
            (
                r"(() => { const t = performance.timing; return [t.unloadEventStart === 0, t.unloadEventEnd === 0]; })()",
                r"[false, false]",
            ),
            (
                r"(() => { const n = performance.getEntriesByType('navigation')[0]; return [n.unloadEventStart === 0, n.unloadEventEnd === 0, n.responseEnd > 0, n.nextHopProtocol === '', typeof n.activationStart, typeof n.notRestoredReasons, typeof n.confidence, typeof n.serverTiming]; })()",
                r#"[false, false, true, true, "number", "object", "object", "object"]"#,
            ),
            (
                r"[typeof performance.eventCounts, typeof performance.memory, typeof performance.interactionCount, typeof performance.onresourcetimingbufferfull]",
                r#"["object", "object", "number", "object"]"#,
            ),
            (
                r"JSON.stringify(PerformanceObserver.supportedEntryTypes)",
                r#""[\"element\",\"event\",\"first-input\",\"largest-contentful-paint\",\"layout-shift\",\"long-animation-frame\",\"longtask\",\"mark\",\"measure\",\"navigation\",\"paint\",\"resource\",\"visibility-state\"]""#,
            ),
            (
                r"PerformanceObserver.supportedEntryTypes === PerformanceObserver.supportedEntryTypes",
                r"true",
            ),
            (
                r"Object.keys(performance.getEntriesByType('navigation')[0].toJSON()).length",
                r"47",
            ),
        ],
    },
    Case {
        name: "marks",
        html: r"<!DOCTYPE html><html><head></head><body><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function U(a) { return Array.from(a).join(); }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
</script></body></html>",
        queries: &[
            (
                r"(() => { const m = performance.mark('a'); return [m instanceof PerformanceMark, m instanceof PerformanceEntry, m.name, m.entryType, m.duration, typeof m.startTime, m.startTime >= 0, m.detail, JSON.stringify(Object.keys(m.toJSON())), Object.getOwnPropertyNames(m).length, performance.getEntriesByName('a')[0] === m, performance.getEntriesByName('a').length]; })()",
                r#"[true, true, "a", "mark", 0, "number", true, null, "[\"name\",\"entryType\",\"startTime\",\"duration\"]", 0, true, 1]"#,
            ),
            (
                r"(() => { const m = performance.mark('b', { detail: { x: 1 }, startTime: 5 }); return [m.startTime, JSON.stringify(m.detail), m.detail === m.detail, JSON.stringify(m)]; })()",
                r#"[5, "{\"x\":1}", true, "{\"name\":\"b\",\"entryType\":\"mark\",\"startTime\":5,\"duration\":0}"]"#,
            ),
            (
                r"E(() => performance.mark('c', { startTime: -1 }))",
                r#""TypeError: Failed to execute 'mark' on 'Performance': 'c' cannot have a negative start time.""#,
            ),
            (
                r"E(() => performance.mark('c', { startTime: NaN }))",
                r#""TypeError: Failed to execute 'mark' on 'Performance': Failed to read the 'startTime' property from 'PerformanceMarkOptions': The provided double value is non-finite.""#,
            ),
            (
                r"E(() => performance.mark('c', { startTime: 'x' }))",
                r#""TypeError: Failed to execute 'mark' on 'Performance': Failed to read the 'startTime' property from 'PerformanceMarkOptions': The provided double value is non-finite.""#,
            ),
            (
                r"E(() => performance.mark())",
                r#""TypeError: Failed to execute 'mark' on 'Performance': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => performance.mark('navigationStart'))",
                r#""DOMException SyntaxError: Failed to execute 'mark' on 'Performance': 'navigationStart' is part of the PerformanceTiming interface, and cannot be used as a mark name.""#,
            ),
            (
                r"E(() => performance.mark('loadEventEnd'))",
                r#""DOMException SyntaxError: Failed to execute 'mark' on 'Performance': 'loadEventEnd' is part of the PerformanceTiming interface, and cannot be used as a mark name.""#,
            ),
            (
                r"E(() => performance.mark('', { detail: function () {} }).name)",
                r#""DOMException DataCloneError: Failed to execute 'mark' on 'Performance': function () {} could not be cloned.""#,
            ),
            (
                r"E(() => performance.mark('x', 5))",
                r#""TypeError: Failed to execute 'mark' on 'Performance': The provided value is not of type 'PerformanceMarkOptions'.""#,
            ),
            (
                r"E(() => performance.mark('x', null).startTime >= 0)",
                r"true",
            ),
            (
                r"E(() => performance.mark('x2', undefined).startTime >= 0)",
                r"true",
            ),
            (
                r"E(() => performance.mark('u', { detail: undefined }).detail)",
                r"null",
            ),
            (r"E(() => performance.mark('u2').detail)", r"null"),
            (
                r"E(() => performance.mark('u3', { detail: null }).detail)",
                r"null",
            ),
            (
                r"E(() => Object.prototype.toString.call(performance.mark('u4', { detail: new Date(0) }).detail))",
                r#""[object Date]""#,
            ),
            (
                r"E(() => performance.mark('u5', { detail: { f() {} } }))",
                r#""DOMException DataCloneError: Failed to execute 'mark' on 'Performance': f() {} could not be cloned.""#,
            ),
            (
                r"E(() => { const d = { a: [1, { b: 2 }] }; const m = performance.mark('u6', { detail: d }); return [m.detail !== d, JSON.stringify(m.detail)]; })",
                r#"[true, "{\"a\":[1,{\"b\":2}]}"]"#,
            ),
            (
                r"E(() => { const m = new PerformanceMark('ctor', { startTime: 7, detail: 'd' }); return [m.name, m.startTime, m.detail, performance.getEntriesByName('ctor').length, m.entryType, m instanceof PerformanceMark]; })",
                r#"["ctor", 7, "d", 0, "mark", true]"#,
            ),
            (
                r"E(() => new PerformanceMark())",
                r#""TypeError: Failed to construct 'PerformanceMark': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => new PerformanceMark('navigationStart'))",
                r#""DOMException SyntaxError: Failed to construct 'PerformanceMark': 'navigationStart' is part of the PerformanceTiming interface, and cannot be used as a mark name.""#,
            ),
            (
                r"E(() => new PerformanceMark('n', { startTime: -1 }))",
                r#""TypeError: Failed to construct 'PerformanceMark': 'n' cannot have a negative start time.""#,
            ),
            (
                r"E(() => { class M extends PerformanceMark {} const m = new M('sub'); return [m instanceof M, m.name, Object.getPrototypeOf(m) === M.prototype]; })",
                r#"[true, "sub", true]"#,
            ),
            (
                r"E(() => performance.mark('nn', { startTime: 0 }).startTime)",
                r"0",
            ),
            (
                r"E(() => performance.mark('nn', { startTime: 1e9 }).startTime)",
                r"1000000000",
            ),
            (
                r"E(() => performance.mark(undefined).name)",
                r#""undefined""#,
            ),
            (r"E(() => performance.mark(null).name)", r#""null""#),
            (r"E(() => performance.mark(12).name)", r#""12""#),
            (
                r"E(() => performance.mark({ toString() { return 'obj'; } }).name)",
                r#""obj""#,
            ),
            (r"E(() => performance.mark('s\ud800').name.length)", r"2"),
            (
                r"E(() => [Object.getOwnPropertyDescriptor(PerformanceEntry.prototype, 'name').get.call(performance.mark('de')), Object.getOwnPropertyDescriptor(PerformanceMark.prototype, 'detail').get.call(performance.mark('de2'))])",
                r#"["de", null]"#,
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(PerformanceMark.prototype, 'detail').get.call(performance.measure('de3')))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => { const m = performance.mark('tj', { detail: { a: 1 }, startTime: 3 }); const j = m.toJSON(); return [JSON.stringify(j), Object.keys(j).join()]; })",
                r#"["{\"name\":\"tj\",\"entryType\":\"mark\",\"startTime\":3,\"duration\":0}", "name,entryType,startTime,duration"]"#,
            ),
        ],
    },
    Case {
        name: "measures",
        html: r"<!DOCTYPE html><html><head></head><body><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function U(a) { return Array.from(a).join(); }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
</script></body></html>",
        queries: &[
            (
                r"E(() => { performance.mark('a', { startTime: 10 }); performance.mark('b', { startTime: 4 }); return 'ok'; })",
                r#""ok""#,
            ),
            (
                r"(() => { const m = performance.measure('m1', 'a'); return [m instanceof PerformanceMeasure, m.entryType, m.name, m.startTime, typeof m.duration, JSON.stringify(m.detail), Object.getPrototypeOf(m) === PerformanceMeasure.prototype]; })()",
                r#"[true, "measure", "m1", 10, "number", "null", true]"#,
            ),
            (
                r"E(() => performance.measure('m2', 'nomark'))",
                r#""DOMException SyntaxError: Failed to execute 'measure' on 'Performance': The mark 'nomark' does not exist.""#,
            ),
            (
                r"E(() => performance.measure('m2', 'a', 'nomark'))",
                r#""DOMException SyntaxError: Failed to execute 'measure' on 'Performance': The mark 'nomark' does not exist.""#,
            ),
            (
                r"E(() => performance.measure('m2', { start: 'nomark' }))",
                r#""DOMException SyntaxError: Failed to execute 'measure' on 'Performance': The mark 'nomark' does not exist.""#,
            ),
            (
                r"E(() => performance.measure('m2', { end: 'nomark' }))",
                r#""DOMException SyntaxError: Failed to execute 'measure' on 'Performance': The mark 'nomark' does not exist.""#,
            ),
            (
                r"E(() => performance.measure('m2', { start: 1, end: 2, duration: 3 }))",
                r#""TypeError: Failed to execute 'measure' on 'Performance': If a non-empty PerformanceMeasureOptions object was passed, it must not have all of its 'start', 'duration', and 'end' properties defined""#,
            ),
            (
                r"E(() => performance.measure('m2', { duration: 3 }))",
                r#""TypeError: Failed to execute 'measure' on 'Performance': If a non-empty PerformanceMeasureOptions object was passed, at least one of its 'start' or 'end' properties must be present.""#,
            ),
            (
                r"E(() => performance.measure('m2', { detail: 1 }))",
                r#""TypeError: Failed to execute 'measure' on 'Performance': If a non-empty PerformanceMeasureOptions object was passed, at least one of its 'start' or 'end' properties must be present.""#,
            ),
            (
                r"E(() => performance.measure('m2', {}, 'a').name)",
                r#""m2""#,
            ),
            (
                r"E(() => performance.measure('m2', { start: 'a' }, 'a'))",
                r#""TypeError: Failed to execute 'measure' on 'Performance': If a non-empty PerformanceMeasureOptions object was passed, |end_mark| must not be passed.""#,
            ),
            (
                r"E(() => performance.measure('m2', { end: 1 }, 'a'))",
                r#""TypeError: Failed to execute 'measure' on 'Performance': If a non-empty PerformanceMeasureOptions object was passed, |end_mark| must not be passed.""#,
            ),
            (
                r"E(() => { const m = performance.measure('m3', { start: 1, duration: 3 }); return [m.startTime, m.duration]; })",
                r"[1, 3]",
            ),
            (
                r"E(() => { const m = performance.measure('m4', { end: 10, duration: 3 }); return [m.startTime, m.duration]; })",
                r"[7, 3]",
            ),
            (
                r"E(() => { const m = performance.measure('m5', { start: 10, end: 4 }); return [m.startTime, m.duration]; })",
                r"[10, -6]",
            ),
            (
                r"E(() => { const m = performance.measure('m6', 'a', 'b'); return [m.startTime, m.duration]; })",
                r"[10, -6]",
            ),
            (
                r"E(() => { const m = performance.measure('m6b', { start: 'a', end: 'b' }); return [m.startTime, m.duration]; })",
                r"[10, -6]",
            ),
            (
                r"E(() => { const m = performance.measure('m6c', { start: 'b', end: 7.5 }); return [m.startTime, m.duration]; })",
                r"[4, 3.5]",
            ),
            (
                r"E(() => { const m = performance.measure('m6d', { start: 2, end: 'a' }); return [m.startTime, m.duration]; })",
                r"[2, 8]",
            ),
            (
                r"E(() => { const m = performance.measure('m6e', { start: 'a', duration: 2 }); return [m.startTime, m.duration]; })",
                r"[10, 2]",
            ),
            (
                r"E(() => { const m = performance.measure('m6f', { end: 'a', duration: 2 }); return [m.startTime, m.duration]; })",
                r"[8, 2]",
            ),
            (
                r"E(() => { const m = performance.measure('m6g', 'a', undefined); return [m.startTime, typeof m.duration]; })",
                r#"[10, "number"]"#,
            ),
            (
                r"E(() => { const m = performance.measure('m6h', undefined, 'a'); return [m.startTime, m.duration]; })",
                r"[0, 10]",
            ),
            (
                r"E(() => { const m = performance.measure('m6i', null, 'a'); return [m.startTime, m.duration]; })",
                r"[0, 10]",
            ),
            (
                r"E(() => { const m = performance.measure('m7', 'navigationStart'); return [m.startTime, m.duration >= 0]; })",
                r"[0, true]",
            ),
            (
                r"E(() => { const m = performance.measure('m8', 'fetchStart', 'responseEnd'); return [m.startTime, m.duration >= 0]; })",
                r"[0, true]",
            ),
            (
                r"E(() => { const m = performance.measure('m8b', 'domLoading'); return [m.startTime >= 0, m.duration >= 0]; })",
                r"[true, true]",
            ),
            (
                r"E(() => { const m = performance.measure('m8c', 'domInteractive', 'domComplete'); return [m.startTime >= 0, m.duration >= 0]; })",
                r"[true, true]",
            ),
            (
                r"E(() => { const m = performance.measure('m10', { start: 2, detail: [1] }); return [m.startTime, JSON.stringify(m.detail), m.detail === m.detail]; })",
                r#"[2, "[1]", true]"#,
            ),
            (
                r"E(() => { const m = performance.measure('m11', undefined, undefined); return [m.startTime, m.duration >= 0]; })",
                r"[0, true]",
            ),
            (
                r"E(() => { const m = performance.measure('m12', null); return [m.startTime]; })",
                r"[0]",
            ),
            (
                r"E(() => { const m = performance.measure('m12b'); return [m.startTime, m.duration >= 0, m.detail]; })",
                r"[0, true, null]",
            ),
            (
                r"E(() => performance.measure())",
                r#""TypeError: Failed to execute 'measure' on 'Performance': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => performance.measure('m13', 5))",
                r#""DOMException SyntaxError: Failed to execute 'measure' on 'Performance': The mark '5' does not exist.""#,
            ),
            (
                r"E(() => performance.measure('m13', { start: 'a' }).startTime)",
                r"10",
            ),
            (
                r"E(() => performance.measure('m13', { start: -1 }))",
                r#""TypeError: Failed to execute 'measure' on 'Performance': 'm13' cannot have a negative time stamp.""#,
            ),
            (
                r"E(() => performance.measure('m14', { start: 3, detail: function () {} }))",
                r#""DOMException DataCloneError: Failed to execute 'measure' on 'Performance': function () {} could not be cloned.""#,
            ),
            (
                r"E(() => { const m = performance.measure('m15', { start: 2, end: 5, detail: { k: 'v' } }); return [m.startTime, m.duration, JSON.stringify(m.detail), JSON.stringify(m.toJSON())]; })",
                r#"[2, 3, "{\"k\":\"v\"}", "{\"name\":\"m15\",\"entryType\":\"measure\",\"startTime\":2,\"duration\":3}"]"#,
            ),
            (
                r"E(() => { const t = { get start() { L.push('start'); return 1; }, get end() { L.push('end'); return 2; }, get duration() { L.push('duration'); return undefined; }, get detail() { L.push('detail'); return 0; } }; L.length = 0; performance.measure('m16', t); return JSON.stringify(L); })",
                r#""[\"detail\",\"duration\",\"end\",\"start\"]""#,
            ),
        ],
    },
    Case {
        name: "entries",
        html: r"<!DOCTYPE html><html><head></head><body><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function U(a) { return Array.from(a).join(); }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
</script></body></html>",
        queries: &[
            (
                r"E(() => { performance.mark('p1', { startTime: 50 }); performance.measure('p2', { start: 50, duration: 0 }); performance.mark('p3', { startTime: 50 }); performance.measure('p4', { start: 50, duration: 3 }); return performance.getEntries().filter((e) => e.startTime === 50).map((e) => e.name).join(); })",
                r#""p1,p2,p3,p4""#,
            ),
            (
                r"E(() => { performance.mark('s', { startTime: 30 }); performance.mark('s2', { startTime: 10 }); performance.measure('s3', { start: 20, duration: 1 }); return performance.getEntries().filter((e) => ['s', 's2', 's3'].includes(e.name)).map((e) => e.name).join(); })",
                r#""s2,s3,s""#,
            ),
            (
                r"performance.getEntriesByType('mark').map((e) => e.name).join()",
                r#""s2,s,p1,p3""#,
            ),
            (
                r"performance.getEntriesByType('measure').map((e) => e.name).join()",
                r#""s3,p2,p4""#,
            ),
            (
                r"performance.getEntries().filter((e) => e.entryType === 'mark' || e.entryType === 'measure').map((e) => e.entryType + ':' + e.name).join()",
                r#""mark:s2,measure:s3,mark:s,mark:p1,measure:p2,mark:p3,measure:p4""#,
            ),
            (
                r"E(() => [performance.getEntriesByName('p1', 'mark').length, performance.getEntriesByName('p1', 'measure').length, performance.getEntriesByName('p1', 'x').length, performance.getEntriesByType('nothing').length, performance.getEntriesByName('nothing').length])",
                r"[1, 0, 0, 0, 0]",
            ),
            (
                r"E(() => performance.getEntriesByName())",
                r#""TypeError: Failed to execute 'getEntriesByName' on 'Performance': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => performance.getEntriesByType())",
                r#""TypeError: Failed to execute 'getEntriesByType' on 'Performance': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => performance.getEntriesByName('p2', undefined).length)",
                r"1",
            ),
            (
                r"E(() => performance.getEntriesByName('p2', null).length)",
                r"0",
            ),
            (
                r"E(() => performance.getEntriesByName(undefined).length)",
                r"0",
            ),
            (
                r"E(() => { const a = performance.getEntriesByType('mark'); const b = performance.getEntriesByType('mark'); return [a !== b, Array.isArray(a), a[0] === b[0]]; })",
                r"[true, true, true]",
            ),
            (
                r"E(() => { performance.clearMarks('p1'); return performance.getEntriesByType('mark').map((e) => e.name).join(); })",
                r#""s2,s,p3""#,
            ),
            (
                r"E(() => { performance.clearMarks('nothing'); return performance.getEntriesByType('mark').length; })",
                r"3",
            ),
            (
                r"E(() => { performance.clearMarks(); return [performance.getEntriesByType('mark').length, performance.getEntriesByType('measure').length]; })",
                r"[0, 3]",
            ),
            (
                r"E(() => { performance.clearMeasures('p2'); const n = performance.getEntriesByType('measure').filter((e) => e.name === 'p2').length; return n; })",
                r"0",
            ),
            (
                r"E(() => { performance.clearMeasures(); return performance.getEntriesByType('measure').length; })",
                r"0",
            ),
            (
                r"E(() => { performance.mark('c1'); performance.clearMarks(undefined); return performance.getEntriesByType('mark').length; })",
                r"0",
            ),
            (
                r"E(() => { performance.mark('c2'); performance.clearMarks(null); return performance.getEntriesByType('mark').length; })",
                r"1",
            ),
            (
                r"E(() => [performance.clearMarks(), performance.clearMeasures(), performance.clearMarks.length, performance.clearMeasures.length, performance.getEntries.length, performance.getEntriesByName.length, performance.getEntriesByType.length, performance.mark.length, performance.measure.length])",
                r"[null, null, 0, 0, 0, 1, 1, 1, 1]",
            ),
            (
                r"E(() => { for (let i = 0; i < 1000; i++) performance.mark('bulk' + i, { startTime: 1000 - i }); const e = performance.getEntriesByType('mark'); return [e.length, e[0].name, e[999].name]; })",
                r#"[1000, "bulk999", "bulk0"]"#,
            ),
            (
                r"E(() => { const e = performance.getEntriesByType('mark'); return e.every((x, i) => i === 0 || e[i - 1].startTime <= x.startTime); })",
                r"true",
            ),
        ],
    },
    Case {
        name: "observer",
        html: r"<!DOCTYPE html><html><head></head><body><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function U(a) { return Array.from(a).join(); }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
</script></body></html>",
        queries: &[
            (
                r"[typeof PerformanceObserver, PerformanceObserver.length, Object.getOwnPropertyNames(PerformanceObserver.prototype).sort().join(), Object.getOwnPropertyNames(PerformanceObserver).sort().join(), Object.isFrozen(PerformanceObserver.supportedEntryTypes), Object.getOwnPropertyDescriptor(PerformanceObserver, 'supportedEntryTypes').set, PerformanceObserver.supportedEntryTypes.includes('mark'), PerformanceObserver.supportedEntryTypes.includes('measure'), PerformanceObserver.supportedEntryTypes.includes('navigation')]",
                r#"["function", 1, "constructor,disconnect,observe,takeRecords", "length,name,prototype,supportedEntryTypes", true, null, true, true, true]"#,
            ),
            (
                r"E(() => new PerformanceObserver())",
                r#""TypeError: Failed to construct 'PerformanceObserver': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => new PerformanceObserver(5))",
                r#""TypeError: Failed to construct 'PerformanceObserver': parameter 1 is not of type 'Function'.""#,
            ),
            (
                r"E(() => PerformanceObserver(() => {}))",
                r#""TypeError: Failed to construct 'PerformanceObserver': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new PerformanceObserver({ handleEvent() {} }))",
                r#""TypeError: Failed to construct 'PerformanceObserver': parameter 1 is not of type 'Function'.""#,
            ),
            (
                r"E(() => new PerformanceObserver(() => {}).observe())",
                r#""TypeError: Failed to execute 'observe' on 'PerformanceObserver': An observe() call must include either entryTypes or type arguments.""#,
            ),
            (
                r"E(() => new PerformanceObserver(() => {}).observe({}))",
                r#""TypeError: Failed to execute 'observe' on 'PerformanceObserver': An observe() call must include either entryTypes or type arguments.""#,
            ),
            (
                r"E(() => new PerformanceObserver(() => {}).observe({ entryTypes: [] }))",
                r#""undefined""#,
            ),
            (
                r"E(() => new PerformanceObserver(() => {}).observe({ entryTypes: ['mark'], type: 'mark' }))",
                r#""TypeError: Failed to execute 'observe' on 'PerformanceObserver': An observe() call must not include both entryTypes and type arguments.""#,
            ),
            (
                r"E(() => new PerformanceObserver(() => {}).observe({ entryTypes: 'mark' }))",
                r#""TypeError: Failed to execute 'observe' on 'PerformanceObserver': Failed to read the 'entryTypes' property from 'PerformanceObserverInit': The provided value cannot be converted to a sequence.""#,
            ),
            (
                r"E(() => new PerformanceObserver(() => {}).observe({ entryTypes: ['nothing'] }))",
                r#""undefined""#,
            ),
            (
                r"E(() => new PerformanceObserver(() => {}).observe({ type: 'nothing' }))",
                r#""undefined""#,
            ),
            (
                r"E(() => new PerformanceObserver(() => {}).observe({ type: 'mark', buffered: true }))",
                r#""undefined""#,
            ),
            (
                r"E(() => new PerformanceObserver(() => {}).takeRecords().length)",
                r"0",
            ),
            (
                r"E(() => new PerformanceObserver(() => {}).disconnect())",
                r#""undefined""#,
            ),
            (
                r"E(() => { const o = new PerformanceObserver(() => {}); o.observe({ type: 'mark' }); o.observe({ entryTypes: ['mark'] }); })",
                r#""DOMException InvalidModificationError: Failed to execute 'observe' on 'PerformanceObserver': This PerformanceObserver has performed observe({type:...}, therefore it cannot perform observe({entryTypes:...})""#,
            ),
            (
                r"E(() => { const o = new PerformanceObserver(() => {}); o.observe({ entryTypes: ['mark'] }); o.observe({ type: 'mark' }); })",
                r#""DOMException InvalidModificationError: Failed to execute 'observe' on 'PerformanceObserver': This observer has performed observe({entryTypes:...}, therefore it cannot perform observe({type:...})""#,
            ),
            (
                r"E(() => { const o = new PerformanceObserver(() => {}); o.observe({ type: 'mark' }); o.observe({ type: 'measure' }); })",
                r#""undefined""#,
            ),
            (
                r"E(() => new PerformanceObserver(() => {}).observe.call({}, { type: 'mark' }))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"(async () => { L.length = 0; const o = new PerformanceObserver((list, obs, opts) => { log([list.constructor.name, list.getEntries().length, list.getEntries().map((e) => e.name).join(), obs === o, typeof opts, JSON.stringify(opts), Object.getOwnPropertyNames(PerformanceObserverEntryList.prototype).sort().join()]); }); o.observe({ entryTypes: ['mark', 'measure'] }); performance.mark('o1'); performance.mark('o2'); log('sync'); await wait(0); log('after0'); await wait(50); o.disconnect(); return JSON.stringify(L); })()",
                r#""[\"sync\",[\"PerformanceObserverEntryList\",2,\"o1,o2\",true,\"object\",\"{\\\"droppedEntriesCount\\\":0}\",\"constructor,getEntries,getEntriesByName,getEntriesByType\"],\"after0\"]""#,
            ),
            (
                r"(async () => { L.length = 0; const o = new PerformanceObserver(function () { log(['this', this === o]); }); o.observe({ type: 'mark' }); performance.mark('t1'); await wait(50); o.disconnect(); return JSON.stringify(L); })()",
                r#""[[\"this\",true]]""#,
            ),
            (
                r"(async () => { L.length = 0; const o = new PerformanceObserver((list) => { log(list.getEntries().map((e) => e.name).join()); }); o.observe({ entryTypes: ['mark'] }); performance.mark('p1x'); setTimeout(() => log('t0'), 0); Promise.resolve().then(() => log('micro')); await wait(100); o.disconnect(); return JSON.stringify(L); })()",
                r#""[\"micro\",\"p1x\",\"t0\"]""#,
            ),
            (
                r"(async () => { L.length = 0; const o = new PerformanceObserver((list) => { log('cb ' + list.getEntries().length); }); o.observe({ type: 'mark', buffered: true }); log('sync'); await wait(50); o.disconnect(); return JSON.stringify(L); })()",
                r#""[\"sync\",\"cb 4\"]""#,
            ),
            (
                r"(async () => { L.length = 0; const o = new PerformanceObserver((list) => { log('cb ' + list.getEntries().length); }); o.observe({ entryTypes: ['mark'], buffered: true }); await wait(50); o.disconnect(); return JSON.stringify(L); })()",
                r#""[]""#,
            ),
            (
                r"(async () => { L.length = 0; const o = new PerformanceObserver(() => { log('cb'); }); o.observe({ type: 'mark' }); performance.mark('q1'); const r = o.takeRecords(); const r2 = o.takeRecords(); await wait(50); o.disconnect(); return JSON.stringify([r.map((e) => e.name), r2.length, L]); })()",
                r#""[[\"q1\"],0,[]]""#,
            ),
            (
                r"(async () => { L.length = 0; const o = new PerformanceObserver(() => { log('cb'); }); o.observe({ type: 'mark' }); performance.mark('r1'); o.disconnect(); await wait(50); return JSON.stringify(L); })()",
                r#""[]""#,
            ),
            (
                r"(async () => { L.length = 0; const o = new PerformanceObserver((list) => { log('cb ' + list.getEntries().map((e) => e.name).join()); }); o.observe({ type: 'mark' }); performance.mark('s1'); await wait(50); performance.mark('s2'); performance.measure('ignored', 's1'); await wait(50); o.disconnect(); return JSON.stringify(L); })()",
                r#""[\"cb s1\",\"cb s2\"]""#,
            ),
            (
                r"(async () => { L.length = 0; const o = new PerformanceObserver((list) => { log('cb ' + list.getEntries().map((e) => e.entryType + e.name).join()); }); o.observe({ type: 'mark' }); o.observe({ type: 'measure' }); performance.mark('t1'); performance.measure('t2', 't1'); await wait(50); o.disconnect(); return JSON.stringify(L); })()",
                r#""[\"cb markt1,measuret2\"]""#,
            ),
            (
                r"(async () => { L.length = 0; const o = new PerformanceObserver((list) => { log('cb ' + list.getEntries().map((e) => e.entryType + e.name).join()); }); o.observe({ entryTypes: ['mark'] }); o.observe({ entryTypes: ['measure'] }); performance.mark('t1'); performance.measure('t2', 't1'); await wait(50); o.disconnect(); return JSON.stringify(L); })()",
                r#""[\"cb measuret2\"]""#,
            ),
            (
                r"(async () => { L.length = 0; const o = new PerformanceObserver((list) => { log('cb1 ' + list.getEntries().map((e) => e.name).join()); }); const o2 = new PerformanceObserver((list) => { log('cb2 ' + list.getEntries().map((e) => e.name).join()); }); o.observe({ type: 'mark' }); o2.observe({ type: 'mark' }); performance.mark('v1'); await wait(50); o.disconnect(); o2.disconnect(); return JSON.stringify(L); })()",
                r#""[\"cb1 v1\",\"cb2 v1\"]""#,
            ),
            (
                r"(async () => { L.length = 0; const o = new PerformanceObserver(() => { log('cb1'); throw new Error('boom'); }); o.observe({ type: 'mark' }); performance.mark('w1'); await wait(50); performance.mark('w2'); await wait(50); o.disconnect(); return JSON.stringify(L); })()",
                r#""[\"cb1\",\"cb1\"]""#,
            ),
            (
                r"(async () => { L.length = 0; const errors = []; const h = (e) => errors.push(e.message); addEventListener('error', h); const o = new PerformanceObserver(() => { throw new Error('boom'); }); o.observe({ type: 'mark' }); performance.mark('w3'); await wait(50); o.disconnect(); removeEventListener('error', h); return JSON.stringify(errors); })()",
                r#""[\"Uncaught Error: boom\"]""#,
            ),
            (
                r"(async () => { L.length = 0; window.once = 0; const o = new PerformanceObserver((list) => { log('cb ' + list.getEntries().map((e) => e.name).join()); if (!window.once) { window.once = 1; performance.mark('x2'); } }); o.observe({ type: 'mark' }); performance.mark('x1'); await wait(50); o.disconnect(); return JSON.stringify(L); })()",
                r#""[\"cb x1\",\"cb x2\"]""#,
            ),
            (
                r"(async () => { L.length = 0; const o = new PerformanceObserver((list) => { log('cb ' + list.getEntries().map((e) => e.name).join()); }); o.observe({ type: 'mark' }); performance.mark('y1'); performance.clearMarks(); await wait(50); o.disconnect(); return JSON.stringify(L); })()",
                r#""[\"cb y1\"]""#,
            ),
            (
                r"(async () => { L.length = 0; const o = new PerformanceObserver((list) => { const a = list.getEntries(); log([a.length, list.getEntriesByType('mark').length, list.getEntriesByName('z1').length, list.getEntriesByName('z1', 'measure').length, a[0] === performance.getEntriesByName('z1')[0]]); }); o.observe({ type: 'mark' }); performance.mark('z1'); await wait(50); o.disconnect(); return JSON.stringify(L); })()",
                r#""[[1,1,1,0,true]]""#,
            ),
            (
                r"(async () => { L.length = 0; const o = new PerformanceObserver((list) => { log(list.getEntries().map((e) => e.name).join()); }); o.observe({ type: 'mark' }); performance.mark('k2', { startTime: 20 }); performance.mark('k1', { startTime: 10 }); await wait(50); o.disconnect(); return JSON.stringify(L); })()",
                r#""[\"k1,k2\"]""#,
            ),
            (
                r"(async () => { L.length = 0; const o = new PerformanceObserver((list) => { log('cb ' + list.getEntries().length); }); o.observe({ type: 'mark' }); for (let i = 0; i < 3; i++) performance.mark('l' + i); await wait(50); o.disconnect(); return JSON.stringify(L); })()",
                r#""[\"cb 3\"]""#,
            ),
            (
                r"(async () => { L.length = 0; const o = new PerformanceObserver((list) => { log('cb ' + list.getEntries().length); }); o.observe({ type: 'navigation', buffered: true }); await wait(50); o.disconnect(); return JSON.stringify(L); })()",
                r#""[\"cb 1\"]""#,
            ),
            (
                r"(async () => { L.length = 0; const o = new PerformanceObserver((list) => { log('cb ' + list.getEntries().length); }); o.observe({ entryTypes: ['navigation'] }); await wait(50); o.disconnect(); return JSON.stringify(L); })()",
                r#""[]""#,
            ),
            (
                r"(async () => { L.length = 0; const o = new PerformanceObserver((list) => { log('cb ' + list.getEntries().length); }); o.observe({ type: 'mark' }); o.observe({ type: 'mark', buffered: true }); await wait(50); o.disconnect(); return JSON.stringify(L.length > 0); })()",
                r#""true""#,
            ),
            (
                r"(async () => { L.length = 0; const o = new PerformanceObserver((list) => { log(list.getEntries().length); }); o.observe({ type: 'mark' }); o.disconnect(); o.observe({ type: 'measure' }); performance.mark('d1'); performance.measure('d2', 'd1'); await wait(50); o.disconnect(); return JSON.stringify(L); })()",
                r#""[1]""#,
            ),
            (
                r"(async () => { L.length = 0; const o = new PerformanceObserver((list) => { log(list.getEntries().length); }); o.observe({ entryTypes: ['mark'] }); o.disconnect(); o.observe({ type: 'mark' }); return 'ok'; })()",
                r"error: InvalidModificationError: Failed to execute 'observe' on 'PerformanceObserver': This observer has performed observe({entryTypes:...}, therefore it cannot perform observe({type:...})",
            ),
        ],
    },
];
