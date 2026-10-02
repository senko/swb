//! `--bench N`: times each pipeline stage for a loaded page.
//!
//! HTML parsing runs N times on the document's bytes. The other stages
//! (stylesheets, style, layout, display list, raster of the viewport) run N
//! times by discarding all results before each render. The output is JSON
//! (format: docs/performance.md).

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::json;
use swb_engine::{Page, Pixmap, StageTimings, Url};
use swb_net::{Destination, Fetcher, Request, fetch_following_redirects};

/// Runs the benchmark for the loaded `page` and prints the result.
pub(crate) fn run(
    page: &mut Page,
    fetcher: &Arc<dyn Fetcher>,
    url: &Url,
    runs: usize,
) -> Result<()> {
    if runs == 0 {
        bail!("--bench needs at least one run");
    }
    page.update_layout();
    let mut pixmap = viewport_pixmap(page)?;
    // The times at the end of loading: the last run of each stage while
    // the page loaded (stages run again when stylesheets and images
    // arrive), and the first raster.
    page.render(&mut pixmap);
    let first = page.timings();

    let response = fetch_following_redirects(
        fetcher.as_ref(),
        Request::get(url.clone(), Destination::Document),
    )
    .context("cannot fetch the document again")?;
    let charset = response.content_type().and_then(|c| c.charset);
    let parse: Vec<Duration> = (0..runs)
        .map(|_| {
            let started = Instant::now();
            let document = swb_dom::parse_html_bytes(&response.body, charset.as_deref());
            drop(document);
            started.elapsed()
        })
        .collect();

    let mut samples: Vec<StageTimings> = Vec::with_capacity(runs);
    for _ in 0..runs {
        page.restart_pipeline();
        page.render(&mut pixmap);
        samples.push(page.timings());
    }
    let stage = |first: Duration, values: Vec<Duration>| json!({ "first_ms": ms(first), "median_ms": ms(median(values)) });
    let pick = |f: fn(&StageTimings) -> Duration| samples.iter().map(f).collect::<Vec<_>>();
    let elements = page.document().map_or(0, |doc| {
        doc.descendants(swb_dom::NodeId::DOCUMENT)
            .filter(|&n| doc.node(n).is_element())
            .count()
    });
    let result = json!({
        "url": url.as_str(),
        "runs": runs,
        "elements": elements,
        "viewport": [page.viewport().width, page.viewport().height],
        "stages": {
            "parse": stage(first.parse, parse),
            "stylesheets": stage(first.stylesheets, pick(|t| t.stylesheets)),
            "style": stage(first.style, pick(|t| t.style)),
            "layout": stage(first.layout, pick(|t| t.layout)),
            "display_list": stage(first.display_list, pick(|t| t.display_list)),
            "raster": stage(first.raster, pick(|t| t.raster)),
        },
    });
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

fn viewport_pixmap(page: &Page) -> Result<Pixmap> {
    let (w, h) = swb_engine::device_size(page.viewport(), page.scale())?;
    Pixmap::new(w, h).with_context(|| format!("cannot allocate a {w}x{h} pixmap"))
}

/// Milliseconds with 3 decimals.
fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 1e6).round() / 1e3
}

fn median(mut values: Vec<Duration>) -> Duration {
    values.sort_unstable();
    values.get(values.len() / 2).copied().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_and_milliseconds() {
        let d = Duration::from_micros;
        assert_eq!(median(vec![d(3), d(1), d(2)]), d(2));
        assert_eq!(median(Vec::new()), Duration::ZERO);
        assert_eq!(ms(Duration::from_micros(1234)), 1.234);
    }
}
