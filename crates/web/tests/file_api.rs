//! `Blob`, `File`, `FormData` and `FileReader` (M8 feature 10b, ADR 0028
//! section 12): the cases of the Chromium probe
//! `tools/probes/web-blob-formdata.json` that need no network (the
//! constructors and their errors, slices, the readers of a blob, the entry
//! list methods of `FormData`, the order of the events of `FileReader`),
//! each with and without the GC stress mode. The cases with the network
//! (multipart bodies, Blob URLs, `sendBeacon`) are in
//! `crates/engine/tests/probe_net.rs`.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

#[path = "file_api/cases.rs"]
mod cases;

mod common;

use common::{Departure, probe_failures};

/// The values where swb departs from Chromium on purpose: (case, query
/// index, swb's value, why).
const SWB_VALUES: &[Departure] = &[(
    "blob",
    1,
    r#"["size,type,arrayBuffer,bytes,slice,text,constructor","name,lastModified,webkitRelativePath,constructor",true,"readyState,result,error,onloadstart,onprogress,onload,onabort,onerror,onloadend,EMPTY,LOADING,DONE,abort,readAsArrayBuffer,readAsBinaryString,readAsDataURL,readAsText,constructor","append,delete,get,getAll,has,set,entries,forEach,keys,values,constructor","length,name,prototype,canParse,createObjectURL,parse,revokeObjectURL"]"#,
    "the generator sorts the operations; swb has no `Blob.stream()` (no streams, ADR 0028 section 12) and no `File.lastModifiedDate`",
)];

#[test]
fn probe_cases_match_chromium() {
    let failures = probe_failures(cases::CASES, SWB_VALUES);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
