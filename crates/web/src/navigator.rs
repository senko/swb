//! `navigator` (HTML §8.9,
//! <https://html.spec.whatwg.org/multipage/system-state.html#the-navigator-object>;
//! ADR 0028 section 13): the identity and the settings of the browser.
//!
//! `userAgent` is the user agent string of swb's requests (ADR 0025: it is
//! unchanged), and `appVersion` is that string without `Mozilla/`. The other
//! values do not claim to be Chromium and do not depend on the machine that
//! runs swb (only `platform` depends on the build target), so that pages and
//! tests see the same on every run:
//!
//! | Member | Value | Chromium (measured) |
//! |---|---|---|
//! | `appCodeName`, `appName`, `product`, `productSub`, `vendorSub` | the constants of the standard | the same |
//! | `platform` | the operating system and CPU of the build | `Linux x86_64` here |
//! | `vendor` | empty | `Google Inc.` |
//! | `language`, `languages` | `en-US`, `["en-US"]` | the same |
//! | `onLine` | true (swb does not watch the connection) | true |
//! | `cookieEnabled` | true | true |
//! | `hardwareConcurrency` | 4 (swb has no workers; a constant keeps runs equal) | the machine's cores |
//! | `maxTouchPoints` | 0 | 0 |
//! | `webdriver` | false (swb's automation is not `WebDriver`) | true under Playwright |
//! | `doNotTrack` | null | null |
//! | `pdfViewerEnabled`, `javaEnabled()` | false | false (headless) |
//!
//! `languages` is a new frozen array at each read, as in Chromium
//! (measured: `navigator.languages === navigator.languages` is false).

use swb_js::{Gc, Object};

use crate::bindings::{self as b, Cx, Platform};
use crate::generated as g;
use crate::state::{WebState, engine, state, state_mut};

/// The language of the browser (`navigator.language`).
const LANGUAGE: &str = "en-US";

/// The value of `navigator.hardwareConcurrency`.
const HARDWARE_CONCURRENCY: u64 = 4;

/// The operating system and CPU of the build, as `navigator.platform`.
fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "MacIntel"
    } else if cfg!(target_os = "windows") {
        "Win32"
    } else if cfg!(target_arch = "x86_64") {
        "Linux x86_64"
    } else {
        "Linux"
    }
}

/// The user agent string of the document's engine.
fn user_agent(cx: &Cx<'_>) -> b::Result<String> {
    Ok(engine(cx.rt)?.user_agent().to_owned())
}

/// The most bytes of beacons that are queued at once (Chromium: 64 KiB,
/// measured; the quota returns when a beacon is sent).
const BEACON_QUOTA: usize = 65_536;

/// `navigator.sendBeacon(url, data)` (Beacon "sendBeacon",
/// <https://w3c.github.io/beacon/#sendbeacon-method>): a `POST` with the
/// credentials of the page and no handler for the result. Chromium's rules,
/// measured: a body of a CORS-safelisted type goes in `no-cors` mode, any
/// other type in `cors` mode (with a preflight); the queued bytes may not
/// pass 64 KiB (the answer is `false`).
fn send_beacon(cx: &mut Cx<'_>, url: &str, data: swb_js::Value) -> b::Result<bool> {
    let bad_url = || b::Exception::type_error("The URL argument is ill-formed or unsupported.");
    let parsed = crate::fetch::engine_parse_url(cx.rt, url)?.ok_or_else(bad_url)?;
    let parsed = swb_net::Url::parse(&parsed).map_err(|_| bad_url())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(b::Exception::type_error(
            "Beacons are only supported over HTTP(S).",
        ));
    }
    let extracted = if data.is_nullish() {
        None
    } else {
        Some(crate::fetch::body::extract(cx.rt, data)?)
    };
    let size = extracted.as_ref().map_or(0, |e| e.bytes.len());
    if state_mut(cx.rt)?.beacon_bytes + size > BEACON_QUOTA {
        return Ok(false);
    }
    let mut request = swb_net::Request::get(parsed, swb_net::Destination::Other);
    request.method = swb_net::Method::Post;
    request.credentials = swb_net::CredentialsMode::Include;
    request.mode = swb_net::RequestMode::NoCors;
    if let Some(extracted) = extracted {
        if let Some(content_type) = extracted.content_type {
            let essence = content_type
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            let safelisted = matches!(
                essence.as_str(),
                "text/plain" | "application/x-www-form-urlencoded" | "multipart/form-data"
            );
            if !safelisted {
                request.mode = swb_net::RequestMode::Cors;
            }
            request.headers.append("content-type", content_type);
        }
        request.body = Some(extracted.bytes);
    }
    let Some(id) = crate::fetch::start_request(cx.rt, request)? else {
        return Ok(false);
    };
    let state = state_mut(cx.rt)?;
    state.beacon_bytes += size;
    state.fetches.beacons.insert(id, size);
    Ok(true)
}

impl g::navigator::NavigatorImpl for WebState {
    fn send_beacon(
        cx: &mut Cx<'_>,
        _this: Platform,
        url: String,
        data: swb_js::Value,
    ) -> b::Result<bool> {
        send_beacon(cx, &url, data)
    }

    fn max_touch_points(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<i32> {
        Ok(0)
    }

    fn webdriver(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<bool> {
        Ok(false)
    }

    fn do_not_track(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<Option<String>> {
        Ok(None)
    }

    fn app_code_name(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok("Mozilla".to_owned())
    }

    fn app_name(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok("Netscape".to_owned())
    }

    fn app_version(cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        let agent = user_agent(cx)?;
        Ok(agent.strip_prefix("Mozilla/").unwrap_or(&agent).to_owned())
    }

    fn platform(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok(platform().to_owned())
    }

    fn product(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok("Gecko".to_owned())
    }

    fn product_sub(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok("20030107".to_owned())
    }

    fn user_agent(cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        user_agent(cx)
    }

    fn vendor(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok(String::new())
    }

    fn vendor_sub(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok(String::new())
    }

    fn language(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok(LANGUAGE.to_owned())
    }

    fn languages(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<Vec<String>> {
        Ok(vec![LANGUAGE.to_owned()])
    }

    fn on_line(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<bool> {
        Ok(true)
    }

    fn cookie_enabled(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<bool> {
        Ok(true)
    }

    fn pdf_viewer_enabled(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<bool> {
        Ok(false)
    }

    fn hardware_concurrency(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<u64> {
        Ok(HARDWARE_CONCURRENCY)
    }

    fn java_enabled(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<bool> {
        Ok(false)
    }
}

/// The window's `Navigator` object, made at the first access.
pub(crate) fn navigator_object(cx: &mut Cx<'_>) -> b::Result<Gc<Object>> {
    if let Some(object) = state(cx.rt)?.navigator {
        return Ok(object);
    }
    let object = cx
        .rt
        .new_host_object(cx.realm(), g::NAVIGATOR, crate::state::NOT_A_NODE)?;
    state_mut(cx.rt)?.navigator = Some(object);
    Ok(object)
}
