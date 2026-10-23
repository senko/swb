//! The freshness test: the checked-in generated glue of every set
//! (`crates/web/src/generated/`, `crates/web/tests/generated/`) equals
//! what the generator makes from the IDL files now. On a failure, run
//! `just bindings` and commit the result.

use std::path::Path;

#[test]
fn generated_glue_is_up_to_date() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let sets = swb_webidl::binding_sets(&root).expect("the IDL directory is readable");
    let mut stale = Vec::new();
    for set in &sets {
        let files = swb_webidl::generate_set(&root, set).expect("the IDL files generate");
        let dir = root.join(&set.out_dir);
        for (name, text) in &files {
            let on_disk = std::fs::read_to_string(dir.join(name)).unwrap_or_default();
            if on_disk != *text {
                stale.push(set.out_dir.join(name).display().to_string());
            }
        }
        for entry in std::fs::read_dir(&dir).expect("the output directory exists") {
            let path = entry.expect("a directory entry").path();
            let generated = path
                .file_name()
                .is_some_and(|n| files.iter().any(|(name, _)| n.to_string_lossy() == *name));
            if path.extension().is_some_and(|e| e == "rs") && !generated {
                stale.push(format!("{} (not generated)", path.display()));
            }
        }
    }
    assert!(
        stale.is_empty(),
        "generated glue out of date (run `just bindings`): {}",
        stale.join(", ")
    );
}
