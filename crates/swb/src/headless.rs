//! Headless mode: load a page without a window, then save a screenshot or
//! print debugging dumps; or serve the automation protocol.

use std::io::Write as _;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use swb_automation::HeadlessBrowser;
use swb_engine::{Evaluation, JsonForm, Page, PageConfig, Size, Url};
use swb_layout::FragmentRef;
use swb_net::Fetcher;
use swb_text::FontContext;

/// What to produce in headless mode.
pub(crate) struct HeadlessOptions<'a> {
    /// The fetcher of the page (the benchmark fetches the document again).
    pub(crate) fetcher: Arc<dyn Fetcher>,
    pub(crate) url: Url,
    pub(crate) viewport: Size,
    pub(crate) scale: f32,
    pub(crate) timeout: Duration,
    pub(crate) screenshot: Option<&'a Path>,
    pub(crate) full_page: bool,
    pub(crate) with_chrome: bool,
    pub(crate) dump_dom: bool,
    pub(crate) dump_layout: bool,
    pub(crate) dump_boxes: Option<&'a Path>,
    pub(crate) bench: Option<usize>,
    /// The JavaScript setting.
    pub(crate) javascript: bool,
    /// Expressions to evaluate after the load (`--eval`).
    pub(crate) eval: &'a [String],
}

/// Runs a headless page with the automation server until a client sends
/// `browser.close`. Prints the server address on standard output.
pub(crate) fn serve(
    fetcher: Arc<dyn Fetcher>,
    fonts: FontContext,
    viewport: Size,
    scale: f32,
    port: u16,
    url: Option<Url>,
    javascript: bool,
) -> Result<()> {
    // `main` runs this on a page thread (`stack::on_page_thread`).
    let mut browser = HeadlessBrowser::new(fetcher, fonts, viewport, scale, port, true)
        .with_context(|| format!("cannot start the automation server on port {port}"))?;
    browser.page().set_javascript(javascript);
    crate::print_server_address(browser.port())?;
    if let Some(url) = url {
        browser.page().navigate(url);
    }
    browser.run();
    Ok(())
}

/// Loads the page and produces the requested outputs.
pub(crate) fn run(fonts: FontContext, options: &HeadlessOptions<'_>) -> Result<()> {
    let config = PageConfig {
        fetcher: Arc::clone(&options.fetcher),
        notify: Arc::new(|| {}),
        network_threads: PageConfig::DEFAULT_NETWORK_THREADS,
        // `main` runs this on a page thread (`stack::on_page_thread`).
        scripts_allowed: true,
    };
    let mut page = Page::new(config, fonts, options.viewport, options.scale);
    page.set_javascript(options.javascript);
    page.navigate(options.url.clone());
    if !page.wait_until_loaded(options.timeout) {
        log::warn!("timed out waiting for the page to load");
    }
    if let Some(error) = page.error() {
        // The `--eval` contract: a page that does not load prints no
        // lines, and swb exits with an error.
        if !options.eval.is_empty() {
            anyhow::bail!("the page did not load: {error}");
        }
        log::warn!("{error}");
    }
    evaluate(&mut page, options.eval)?;
    page.update_layout();

    if options.dump_dom {
        let doc = page.document().context("no document")?;
        print!("{}", swb_dom::dump_tree(doc));
    }
    if options.dump_layout {
        print!("{}", dump_layout(&page));
    }
    if let Some(runs) = options.bench {
        crate::bench::run(&mut page, &options.fetcher, &options.url, runs)?;
    }
    if let Some(path) = options.dump_boxes {
        let dump = swb_automation::box_dump(&page, options.url.as_str());
        let json = serde_json::to_string_pretty(&dump).context("cannot serialize the boxes")?;
        std::fs::write(path, json + "\n")
            .with_context(|| format!("cannot write {}", path.display()))?;
    }
    if let Some(path) = options.screenshot.filter(|_| options.with_chrome) {
        swb_engine::device_size(options.viewport, options.scale)?;
        let pixmap = crate::gui::render_window(&mut page, options.viewport, options.scale)?;
        pixmap
            .save_png(path)
            .with_context(|| format!("cannot write {}", path.display()))?;
        return Ok(());
    }
    if let Some(path) = options.screenshot {
        let pixmap = page.screenshot(options.full_page)?;
        pixmap
            .save_png(path)
            .with_context(|| format!("cannot write {}", path.display()))?;
        log::info!("saved {}", path.display());
    }
    Ok(())
}

/// Evaluates each expression in order and prints one line per expression
/// (docs/testing.md, "The `--eval` contract"): `{"value": V}` with V in
/// the probe's JSON form (`NaN`, `Infinity`, `-0.0` allowed), or
/// `{"error": "Name: message"}`.
fn evaluate(page: &mut Page, expressions: &[String]) -> Result<()> {
    let mut out = std::io::stdout().lock();
    for expression in expressions {
        let line = answer_line(page.evaluate(expression, JsonForm::Probe));
        writeln!(out, "{line}").context("cannot write to standard output")?;
    }
    out.flush().context("cannot write to standard output")
}

/// The line of one `--eval` answer; `None` is a page without scripts.
fn answer_line(evaluation: Option<Evaluation>) -> String {
    match evaluation {
        Some(Evaluation::Value(text)) => format!("{{\"value\": {text}}}"),
        Some(Evaluation::Error(message)) => {
            format!("{{\"error\": {}}}", serde_json::Value::String(message))
        }
        None => "{\"error\": \"JavaScript is off (use --js)\"}".to_owned(),
    }
}

/// One line per box fragment: depth, element, and absolute border box.
fn dump_layout(page: &Page) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let (Some(doc), Some(tree)) = (page.document(), page.fragments()) else {
        return out;
    };
    tree.walk(|fragment, origin| match fragment {
        FragmentRef::Box(b) => {
            let r = b.border_rect.translate(origin);
            let name = b
                .node
                .and_then(|n| doc.element(n))
                .map_or_else(|| "(anonymous)".to_owned(), |e| e.local_name().to_string());
            let pseudo = b.pseudo.map_or(String::new(), |p| format!("::{p:?}"));
            let _ = writeln!(
                out,
                "{name}{pseudo} {:.2},{:.2} {:.2}x{:.2}",
                r.x, r.y, r.width, r.height
            );
        }
        FragmentRef::Text(t) => {
            let r = t.rect.translate(origin);
            let _ = writeln!(
                out,
                "  text {:.2},{:.2} {:.2}x{:.2} {:?}",
                r.x, r.y, r.width, r.height, t.text
            );
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answer_lines_follow_the_eval_contract() {
        let value = Evaluation::Value("[NaN,-0.0,\"a\"]".to_owned());
        assert_eq!(answer_line(Some(value)), "{\"value\": [NaN,-0.0,\"a\"]}");
        let error = Evaluation::Error("TypeError: \"x\" is not a function".to_owned());
        assert_eq!(
            answer_line(Some(error)),
            "{\"error\": \"TypeError: \\\"x\\\" is not a function\"}"
        );
        let off: serde_json::Value = serde_json::from_str(&answer_line(None)).unwrap();
        assert!(off["error"].is_string());
    }
}
