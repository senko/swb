//! Shared code for the engine tests: directories with test pages, and
//! comparisons of swb's layout with stored Chromium geometry (the scoring
//! rules follow docs/testing.md, "Scores").

#![allow(dead_code)] // Each test binary uses a different subset.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use swb_engine::{ElementBox, FontContext, Page, PageConfig, Rect, Size, Url};
use swb_net::Fetcher;

/// A directory with test pages, removed at the end of the test.
pub(crate) struct Site {
    dir: PathBuf,
}

impl Site {
    pub(crate) fn new(test: &str) -> Site {
        let dir = std::env::temp_dir().join(format!("swb-page-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("cannot create the test directory");
        Site { dir }
    }

    /// Writes a file and returns its URL.
    pub(crate) fn file(&self, name: &str, content: &[u8]) -> Url {
        let path = self.dir.join(name);
        std::fs::write(&path, content).expect("cannot write a test file");
        Url::from_file_path(path).expect("an absolute path")
    }

    pub(crate) fn page(&self, name: &str, html: &str) -> Url {
        self.file(name, html.as_bytes())
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The repository root.
pub(crate) fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Loads `url` with `fetcher` and the bundled test fonts, and returns the
/// element boxes.
pub(crate) fn load_boxes(fetcher: Arc<dyn Fetcher>, url: Url, viewport: Size) -> Vec<ElementBox> {
    let config = PageConfig {
        fetcher,
        notify: Arc::new(|| {}),
        network_threads: 4,
    };
    let mut page = Page::new(config, FontContext::for_tests(), viewport, 1.0);
    page.navigate(url);
    assert!(
        page.wait_until_loaded(Duration::from_secs(60)),
        "page did not load"
    );
    page.update_layout();
    swb_engine::element_boxes(&page)
}

/// Reads a box dump (`*.boxes.json`).
pub(crate) fn read_dump(path: &Path) -> Vec<ElementBox> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let json: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    json["elements"]
        .as_array()
        .expect("elements array")
        .iter()
        .map(|e| ElementBox {
            tag: e["tag"].as_str().unwrap_or("").to_ascii_lowercase(),
            rect: e["rect"].as_array().map(|r| {
                let v = |i: usize| r[i].as_f64().unwrap_or(0.0) as f32;
                Rect::new(v(0), v(1), v(2), v(3))
            }),
            parent: e["parent"].as_u64().map(|p| p as usize),
        })
        .collect()
}

/// Geometry comparison result.
#[derive(Debug)]
pub(crate) struct Comparison {
    /// Fraction of reference elements with a box whose swb box matches in
    /// position and size.
    pub(crate) geometry: f64,
    /// Human-readable mismatches (at most 20).
    pub(crate) mismatches: Vec<String>,
}

/// Compares swb's boxes with the reference (Chromium) boxes. Elements pair
/// by index if the tag sequences are equal, otherwise by a diff of the tag
/// sequences.
pub(crate) fn compare(
    reference: &[ElementBox],
    actual: &[ElementBox],
    tolerance: f32,
) -> Comparison {
    let pairs = align(reference, actual);
    let mut total = 0usize;
    let mut passed = 0usize;
    let mut mismatches = Vec::new();
    for (ri, ai) in pairs {
        let Some(expected) = reference[ri].rect else {
            continue;
        };
        total += 1;
        let got = ai.and_then(|i| actual[i].rect);
        let ok = got.is_some_and(|g| {
            (g.x - expected.x).abs() <= tolerance
                && (g.y - expected.y).abs() <= tolerance
                && (g.width - expected.width).abs() <= tolerance
                && (g.height - expected.height).abs() <= tolerance
        });
        if ok {
            passed += 1;
        } else if mismatches.len() < 20 {
            mismatches.push(format!(
                "#{ri} <{}>: expected {:?}, got {:?}",
                reference[ri].tag,
                fmt_rect(expected),
                got.map(fmt_rect)
            ));
        }
    }
    let geometry = if total == 0 {
        1.0
    } else {
        passed as f64 / total as f64
    };
    Comparison {
        geometry,
        mismatches,
    }
}

fn fmt_rect(r: Rect) -> (f32, f32, f32, f32) {
    (r.x, r.y, r.width, r.height)
}

/// Pairs every reference element with an element of `actual`, or `None`.
fn align(reference: &[ElementBox], actual: &[ElementBox]) -> Vec<(usize, Option<usize>)> {
    let ref_tags: Vec<&str> = reference.iter().map(|e| e.tag.as_str()).collect();
    let act_tags: Vec<&str> = actual.iter().map(|e| e.tag.as_str()).collect();
    if ref_tags == act_tags {
        return (0..reference.len()).map(|i| (i, Some(i))).collect();
    }
    let mut paired = vec![None; reference.len()];
    let diff = similar::capture_diff_slices(similar::Algorithm::Myers, &ref_tags, &act_tags);
    for op in diff {
        if let similar::DiffOp::Equal {
            old_index,
            new_index,
            len,
        } = op
        {
            for k in 0..len {
                paired[old_index + k] = Some(new_index + k);
            }
        }
    }
    paired.into_iter().enumerate().collect()
}
