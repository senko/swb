//! `swb-webidl`: writes the generated binding glue of every set
//! (`just bindings`). With `--check`, it writes nothing and exits with
//! status 1 if a generated file differs from the IDL.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let check = std::env::args().any(|a| a == "--check");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    match run(&root, check) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(message) => {
            eprintln!("swb-webidl: {message}");
            ExitCode::from(2)
        }
    }
}

/// Generates every set; `Ok(false)` if `check` found a difference.
fn run(root: &Path, check: bool) -> Result<bool, String> {
    let sets = swb_webidl::binding_sets(root).map_err(|e| e.to_string())?;
    let mut fresh = true;
    for set in &sets {
        let files = swb_webidl::generate_set(root, set)?;
        let dir = root.join(&set.out_dir);
        let stale = stale_files(&dir, &files)?;
        for (name, text) in &files {
            let path = dir.join(name);
            let old = std::fs::read_to_string(&path).unwrap_or_default();
            if old == *text {
                continue;
            }
            fresh = false;
            if check {
                eprintln!("out of date: {}", set.out_dir.join(name).display());
            } else {
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
                println!("wrote {}", set.out_dir.join(name).display());
            }
        }
        for path in stale {
            fresh = false;
            if check {
                eprintln!("not generated: {}", path.display());
            } else {
                std::fs::remove_file(&path).map_err(|e| e.to_string())?;
                println!("removed {}", path.display());
            }
        }
    }
    Ok(fresh || !check)
}

/// The `.rs` files of `dir` that the set does not generate.
fn stale_files(dir: &Path, files: &[(String, String)]) -> Result<Vec<PathBuf>, String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut stale = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        let generated = path
            .file_name()
            .is_some_and(|n| files.iter().any(|(name, _)| n.to_string_lossy() == *name));
        if path.extension().is_some_and(|e| e == "rs") && !generated {
            stale.push(path);
        }
    }
    Ok(stale)
}
