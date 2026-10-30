//! `Headers`, `Request`, `Response`, `AbortController` and `AbortSignal`
//! (M8 feature 10a, ADR 0028 section 12): the cases of the Chromium probe
//! `tools/probes/web-fetch.json` (header lists and their guards, the
//! constructors and their `TypeError` texts, the body methods and
//! `bodyUsed`, `clone()`, the static `Response` methods, the signals, their
//! events, `timeout()`, `any()` and the `signal` option of
//! `addEventListener`), each with and without the GC stress mode. The cases
//! that need a network (`fetch()`, CORS) are in
//! `crates/engine/tests/fetch_api.rs`.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

#[path = "fetch/cases.rs"]
mod cases;

mod common;

use common::{Departure, probe_failures};

/// The values where swb departs from Chromium on purpose: (case, query
/// index, swb's value, why).
const SWB_VALUES: &[Departure] = &[
    (
        "request",
        3,
        r#"[["application/x-www-form-urlencoded;charset=UTF-8", true, "a=1&b=2"], [null, true, "hi"], [null, true, "hi"], ["text/plain;charset=UTF-8", true, "str"], ["text/plain;charset=UTF-8", true, "5"], ["text/plain;charset=UTF-8", true, "[object Object]"], [null, true, ""], [null, true, ""]]"#,
        "`body` is always `null`: swb has no streams (ADR 0028 section 12)",
    ),
    (
        "response",
        0,
        r#"[201, "Made", true, "default", "", false, false, true, "text/plain;charset=UTF-8", "1", "[object Response]"]"#,
        "`body` is always `null`: swb has no streams (ADR 0028 section 12)",
    ),
    (
        "response",
        4,
        r#"[["function", "function"], "type,url,redirected,status,ok,statusText,headers,body,bodyUsed,arrayBuffer,blob,bytes,clone,formData,json,text,constructor", "length,name,prototype,error,json,redirect", "method,url,headers,destination,referrer,referrerPolicy,mode,credentials,cache,redirect,integrity,keepalive,signal,duplex,isHistoryNavigation,body,bodyUsed,arrayBuffer,blob,bytes,clone,formData,json,text,constructor"]"#,
        "the generator sorts the operations (`bytes` before `clone`), and the Body mixin's `body` and `bodyUsed` come in IDL order; Chromium puts `bytes` last and has `targetAddressSpace`",
    ),
];

#[test]
fn probe_cases_match_chromium() {
    let failures = probe_failures(cases::CASES, SWB_VALUES);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
