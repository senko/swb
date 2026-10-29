//! `Storage` (M8 feature 8b, ADR 0028 sections 12 and 16): the cases of the
//! Chromium probe `tools/probes/web-storage.json` (the interface, the
//! methods and their conversions, the named properties, the quota, strings,
//! the events), each with and without the GC stress mode. The documents of
//! these cases are `file:` pages, which have a storage area as in Chromium.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

#[path = "storage/cases.rs"]
mod cases;
mod common;

use common::{Departure, probe_failures};

/// The values where swb departs from Chromium on purpose.
const SWB_VALUES: &[Departure] = &[
    // Web IDL's [[OwnPropertyKeys]] leaves out the names that the prototype
    // hides (ADR 0028 section 5, feature 4c1); Chromium lists them.
    (
        "storage-named",
        9,
        r#"["function","v",1,[],false,null]"#,
        "`Object.keys` omits a name that the prototype hides (Web IDL)",
    ),
    (
        "storage-named",
        10,
        r#"["function",2,"v2","number",[],false,null]"#,
        "`Object.keys` omits a name that the prototype hides (Web IDL)",
    ),
    (
        "storage-proto-names",
        1,
        r#"["function","v",1,[]]"#,
        "`Object.keys` omits a name that the prototype hides (Web IDL)",
    ),
    (
        "storage-proto-names",
        2,
        r#"["function",2,"v2","number",[],false,null]"#,
        "`Object.keys` omits a name that the prototype hides (Web IDL)",
    ),
    // Web IDL's [[Set]] calls the named setter for a name that the
    // prototype has; Chromium makes an own property (or, for the accessor
    // `length`, ignores the write).
    (
        "storage-proto-names",
        3,
        r#"["function",1,[]]"#,
        "the named setter takes `localStorage.getItem = x` (Web IDL; ADR 0028 feature 4c1)",
    ),
    (
        "storage-proto-names",
        4,
        r#"["number",1,[]]"#,
        "the named setter takes `localStorage.length = 7`",
    ),
    (
        "storage-proto-names",
        5,
        r#"["function",1,[]]"#,
        "the named setter takes `localStorage.toString = x`",
    ),
    (
        "storage-proto-names",
        6,
        r#"["function",1,[],"[object Storage]"]"#,
        "`Object.keys` omits a name that the prototype hides (Web IDL)",
    ),
    (
        "storage-proto-names",
        7,
        "[true,1,[]]",
        "`Object.keys` omits a name that the prototype hides (Web IDL)",
    ),
    // `QuotaExceededError` is a `DOMException` in swb (no subclass, no
    // `quota` and `requested`).
    (
        "storage-quota",
        1,
        r#"["err","QuotaExceededError","Failed to execute 'setItem' on 'Storage': Setting the value of 'a' exceeded the quota.",22,"DOMException",true,true,"[object DOMException]",1,null]"#,
        "swb has no `QuotaExceededError` class: the constructor and the tag are those of `DOMException`",
    ),
    (
        "storage-quota",
        19,
        r#"["QuotaExceededError","Failed to execute 'setItem' on 'Storage': Setting the value of 'a' exceeded the quota.",22,[],"QuotaExceededError: Failed to execute 'setItem' on 'Storage': Setting the value of 'a' exceeded the quota.","undefined","undefined",null]"#,
        "no `stack`, `quota` or `requested` on the exception",
    ),
    // Strings are UTF-8: a lone surrogate becomes U+FFFD (ADR 0028
    // section 3).
    (
        "storage-values",
        1,
        "[1,65533,false,true]",
        "a lone surrogate in a value becomes U+FFFD",
    ),
    (
        "storage-values",
        2,
        r#"[1,1,65533,"v","v"]"#,
        "a lone surrogate in a key becomes U+FFFD",
    ),
    (
        "storage-values",
        3,
        r#"["x�y","x�y",3,65533]"#,
        "lone surrogates become U+FFFD",
    ),
    (
        "storage-values",
        4,
        r#"[1,"2","2"]"#,
        "a lone surrogate and U+FFFD are the same key",
    ),
    (
        "storage-values",
        5,
        r#"[[65533],"lone",1]"#,
        "a lone surrogate in a key becomes U+FFFD",
    ),
];

#[test]
fn probe_cases_match_chromium() {
    let failures = probe_failures(cases::CASES, SWB_VALUES);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
