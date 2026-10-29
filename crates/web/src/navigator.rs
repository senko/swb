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

impl g::navigator::NavigatorImpl for WebState {
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
