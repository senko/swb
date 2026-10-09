//! Shared by the examples: the file arguments of the command line.

use std::path::{Path, PathBuf};

/// The files that the arguments name: a directory stands for its `*.js`
/// files in name order, anything else for itself.
pub(crate) fn js_files(args: &[String]) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for arg in args {
        let path = Path::new(arg);
        if path.is_dir() {
            let mut entries: Vec<PathBuf> = std::fs::read_dir(path)?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|p| p.extension().is_some_and(|e| e == "js"))
                .collect();
            entries.sort();
            out.extend(entries);
        } else {
            out.push(path.to_path_buf());
        }
    }
    Ok(out)
}
