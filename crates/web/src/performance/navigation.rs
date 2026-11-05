//! Navigation Timing and Resource Timing for scripts (M8 feature 8a2):
//! `performance.timing` (`PerformanceTiming` by name), `performance.navigation`,
//! `PerformanceResourceTiming` and `PerformanceNavigationTiming`, over the
//! load times that `super` records.
//!
//! - <https://w3c.github.io/navigation-timing/>
//! - <https://w3c.github.io/resource-timing/>

use swb_js::{Gc, Object, Value};

use super::{NavigationTimes, TIMING_NAMES, TimingName};
use crate::bindings::{self as b, Cx, Platform};
use crate::generated as g;
use crate::generated::types::{NavigationTimingType, RenderBlockingStatusType};
use crate::host::NavigationKind;
use crate::state::{NOT_A_NODE, WebState, engine, state};

/// `performance.timing`: whole milliseconds since the epoch.
fn timing_ms(cx: &mut Cx<'_>, rel: Option<f64>) -> b::Result<u64> {
    let Some(rel) = rel else {
        return Ok(0);
    };
    let origin = state(cx.rt)?.doc.clock.time_origin();
    Ok((origin + rel).floor().max(0.0) as u64)
}

/// A `PerformanceTiming` attribute by name.
fn timing_attribute(cx: &mut Cx<'_>, name: &str) -> b::Result<u64> {
    let zero = matches!(
        name,
        "unloadEventStart"
            | "unloadEventEnd"
            | "redirectStart"
            | "redirectEnd"
            | "secureConnectionStart"
    );
    let rel = match state(cx.rt)?.doc.perf.times.of_timing_name(name) {
        TimingName::At(time) if !zero => Some(time),
        _ => None,
    };
    timing_ms(cx, rel)
}

macro_rules! timing_attributes {
    ($($method:ident => $name:literal),* $(,)?) => {
        impl g::performance_timing::PerformanceTimingImpl for WebState {
            $(
                fn $method(cx: &mut Cx<'_>, _this: Platform) -> b::Result<u64> {
                    timing_attribute(cx, $name)
                }
            )*

            fn to_json(cx: &mut Cx<'_>, _this: Platform) -> b::Result<Gc<Object>> {
                let realm = cx.realm();
                let object = cx.rt.new_plain_object(realm)?;
                for name in TIMING_NAMES {
                    let value = timing_attribute(cx, name)?;
                    b::to_js::dictionary_member(cx.rt, object, name, Value::number(value as f64))?;
                }
                Ok(object)
            }
        }
    };
}

timing_attributes! {
    navigation_start => "navigationStart",
    unload_event_start => "unloadEventStart",
    unload_event_end => "unloadEventEnd",
    redirect_start => "redirectStart",
    redirect_end => "redirectEnd",
    fetch_start => "fetchStart",
    domain_lookup_start => "domainLookupStart",
    domain_lookup_end => "domainLookupEnd",
    connect_start => "connectStart",
    connect_end => "connectEnd",
    secure_connection_start => "secureConnectionStart",
    request_start => "requestStart",
    response_start => "responseStart",
    response_end => "responseEnd",
    dom_loading => "domLoading",
    dom_interactive => "domInteractive",
    dom_content_loaded_event_start => "domContentLoadedEventStart",
    dom_content_loaded_event_end => "domContentLoadedEventEnd",
    dom_complete => "domComplete",
    load_event_start => "loadEventStart",
    load_event_end => "loadEventEnd",
}

impl g::performance_navigation::PerformanceNavigationImpl for WebState {
    fn type_(cx: &mut Cx<'_>, _this: Platform) -> b::Result<u16> {
        Ok(match engine(cx.rt)?.navigation_info().kind {
            NavigationKind::Navigate => 0,
            NavigationKind::Reload => 1,
            NavigationKind::BackForward => 2,
        })
    }

    fn redirect_count(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<u16> {
        Ok(0)
    }

    fn to_json(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        let kind = <Self as g::performance_navigation::PerformanceNavigationImpl>::type_(cx, this)?;
        let realm = cx.realm();
        let object = cx.rt.new_plain_object(realm)?;
        b::to_js::dictionary_member(cx.rt, object, "type", Value::number(f64::from(kind)))?;
        b::to_js::dictionary_member(cx.rt, object, "redirectCount", Value::number(0.0))?;
        Ok(object)
    }
}

/// A value of the navigation entry, read from the load times.
fn nav_time(cx: &mut Cx<'_>, pick: impl Fn(&NavigationTimes) -> Option<f64>) -> b::Result<f64> {
    Ok(pick(&state(cx.rt)?.doc.perf.times).unwrap_or(0.0))
}

/// The members of the navigation entry beyond the four of every entry, as
/// `toJSON` lists them (the attributes of `PerformanceResourceTiming`, then
/// those of `PerformanceNavigationTiming`).
#[allow(clippy::too_many_lines, reason = "one line per member of the entry")]
pub(super) fn navigation_members(cx: &mut Cx<'_>, object: Gc<Object>) -> b::Result<()> {
    use g::performance_navigation_timing::PerformanceNavigationTimingImpl as Nav;
    use g::performance_resource_timing::PerformanceResourceTimingImpl as Res;
    let this = Platform {
        object,
        data: NOT_A_NODE,
    };
    let strings = [
        (
            "initiatorType",
            <WebState as Res>::initiator_type(cx, this)?,
        ),
        ("deliveryType", <WebState as Res>::delivery_type(cx, this)?),
        (
            "nextHopProtocol",
            <WebState as Res>::next_hop_protocol(cx, this)?,
        ),
        (
            "renderBlockingStatus",
            <WebState as Res>::render_blocking_status(cx, this)?
                .as_str()
                .to_owned(),
        ),
        ("contentType", <WebState as Res>::content_type(cx, this)?),
        (
            "contentEncoding",
            <WebState as Res>::content_encoding(cx, this)?,
        ),
    ];
    for (key, text) in strings {
        let value = b::to_js::string(cx.rt, &text)?;
        b::to_js::dictionary_member(cx.rt, object, key, value)?;
    }
    let numbers = [
        ("workerStart", <WebState as Res>::worker_start(cx, this)?),
        (
            "redirectStart",
            <WebState as Res>::redirect_start(cx, this)?,
        ),
        ("redirectEnd", <WebState as Res>::redirect_end(cx, this)?),
        ("fetchStart", <WebState as Res>::fetch_start(cx, this)?),
        (
            "domainLookupStart",
            <WebState as Res>::domain_lookup_start(cx, this)?,
        ),
        (
            "domainLookupEnd",
            <WebState as Res>::domain_lookup_end(cx, this)?,
        ),
        ("connectStart", <WebState as Res>::connect_start(cx, this)?),
        (
            "secureConnectionStart",
            <WebState as Res>::secure_connection_start(cx, this)?,
        ),
        ("connectEnd", <WebState as Res>::connect_end(cx, this)?),
        ("requestStart", <WebState as Res>::request_start(cx, this)?),
        (
            "responseStart",
            <WebState as Res>::response_start(cx, this)?,
        ),
        (
            "firstInterimResponseStart",
            <WebState as Res>::first_interim_response_start(cx, this)?,
        ),
        (
            "finalResponseHeadersStart",
            <WebState as Res>::final_response_headers_start(cx, this)?,
        ),
        ("responseEnd", <WebState as Res>::response_end(cx, this)?),
        (
            "transferSize",
            <WebState as Res>::transfer_size(cx, this)? as f64,
        ),
        (
            "encodedBodySize",
            <WebState as Res>::encoded_body_size(cx, this)? as f64,
        ),
        (
            "decodedBodySize",
            <WebState as Res>::decoded_body_size(cx, this)? as f64,
        ),
        (
            "responseStatus",
            f64::from(<WebState as Res>::response_status(cx, this)?),
        ),
        (
            "unloadEventStart",
            <WebState as Nav>::unload_event_start(cx, this)?,
        ),
        (
            "unloadEventEnd",
            <WebState as Nav>::unload_event_end(cx, this)?,
        ),
        (
            "domInteractive",
            <WebState as Nav>::dom_interactive(cx, this)?,
        ),
        (
            "domContentLoadedEventStart",
            <WebState as Nav>::dom_content_loaded_event_start(cx, this)?,
        ),
        (
            "domContentLoadedEventEnd",
            <WebState as Nav>::dom_content_loaded_event_end(cx, this)?,
        ),
        ("domComplete", <WebState as Nav>::dom_complete(cx, this)?),
        (
            "loadEventStart",
            <WebState as Nav>::load_event_start(cx, this)?,
        ),
        ("loadEventEnd", <WebState as Nav>::load_event_end(cx, this)?),
    ];
    for (key, number) in numbers {
        b::to_js::dictionary_member(cx.rt, object, key, Value::number(number))?;
    }
    let kind = <WebState as Nav>::type_(cx, this)?;
    let kind = b::to_js::string(cx.rt, kind.as_str())?;
    b::to_js::dictionary_member(cx.rt, object, "type", kind)?;
    let count = f64::from(<WebState as Nav>::redirect_count(cx, this)?);
    b::to_js::dictionary_member(cx.rt, object, "redirectCount", Value::number(count))?;
    let restart = <WebState as Nav>::critical_ch_restart(cx, this)?;
    b::to_js::dictionary_member(cx.rt, object, "criticalCHRestart", Value::number(restart))?;
    Ok(())
}

impl g::performance_resource_timing::PerformanceResourceTimingImpl for WebState {
    fn initiator_type(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok("navigation".to_owned())
    }

    fn delivery_type(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok(String::new())
    }

    fn next_hop_protocol(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok(String::new())
    }

    fn worker_start(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn redirect_start(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn redirect_end(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn fetch_start(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn domain_lookup_start(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn domain_lookup_end(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn connect_start(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn connect_end(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn secure_connection_start(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn request_start(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn final_response_headers_start(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn first_interim_response_start(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn response_start(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn response_end(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn transfer_size(cx: &mut Cx<'_>, _this: Platform) -> b::Result<u64> {
        Ok(engine(cx.rt)?.navigation_info().body_bytes)
    }

    fn encoded_body_size(cx: &mut Cx<'_>, _this: Platform) -> b::Result<u64> {
        Ok(engine(cx.rt)?.navigation_info().body_bytes)
    }

    fn decoded_body_size(cx: &mut Cx<'_>, _this: Platform) -> b::Result<u64> {
        Ok(engine(cx.rt)?.navigation_info().body_bytes)
    }

    fn response_status(cx: &mut Cx<'_>, _this: Platform) -> b::Result<u16> {
        Ok(engine(cx.rt)?.navigation_info().status)
    }

    fn render_blocking_status(
        _cx: &mut Cx<'_>,
        _this: Platform,
    ) -> b::Result<RenderBlockingStatusType> {
        Ok(RenderBlockingStatusType::NonBlocking)
    }

    fn content_type(cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok(engine(cx.rt)?.content_type().to_owned())
    }

    fn content_encoding(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok(String::new())
    }

    fn to_json(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        <WebState as g::performance_entry::PerformanceEntryImpl>::to_json(cx, this)
    }
}

impl g::performance_navigation_timing::PerformanceNavigationTimingImpl for WebState {
    fn unload_event_start(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn unload_event_end(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn dom_interactive(cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        nav_time(cx, |t| t.dom_interactive)
    }

    fn dom_content_loaded_event_start(cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        nav_time(cx, |t| t.content_loaded_start)
    }

    fn dom_content_loaded_event_end(cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        nav_time(cx, |t| t.content_loaded_end)
    }

    fn dom_complete(cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        nav_time(cx, |t| t.dom_complete)
    }

    fn load_event_start(cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        nav_time(cx, |t| t.load_start)
    }

    fn load_event_end(cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        nav_time(cx, |t| t.load_end)
    }

    fn type_(cx: &mut Cx<'_>, _this: Platform) -> b::Result<NavigationTimingType> {
        Ok(match engine(cx.rt)?.navigation_info().kind {
            NavigationKind::Navigate => NavigationTimingType::Navigate,
            NavigationKind::Reload => NavigationTimingType::Reload,
            NavigationKind::BackForward => NavigationTimingType::BackForward,
        })
    }

    fn redirect_count(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<u16> {
        Ok(0)
    }

    fn critical_ch_restart(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn to_json(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        <WebState as g::performance_entry::PerformanceEntryImpl>::to_json(cx, this)
    }
}
