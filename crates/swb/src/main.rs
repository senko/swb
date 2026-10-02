//! The swb web browser.

mod gui;
mod headless;
mod url_input;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Parser;
use swb_engine::Size;
use swb_net::{Fetcher, NetworkFetcher, RecordingFetcher, ReplayFetcher};
use swb_text::FontContext;

/// A web browser written from scratch.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// URL or file to open.
    url: Option<String>,

    /// Run without a window.
    #[arg(long)]
    headless: bool,

    /// Headless: save a PNG screenshot of the viewport to this file.
    #[arg(long, value_name = "FILE")]
    screenshot: Option<PathBuf>,

    /// Headless: capture the whole page height in the screenshot.
    #[arg(long)]
    full_page: bool,

    /// Headless: render the whole browser window (toolbar included) in the
    /// screenshot; `--size` is then the window size.
    #[arg(long)]
    with_chrome: bool,

    /// Headless: print the DOM tree.
    #[arg(long)]
    dump_dom: bool,

    /// Headless: print the layout (box fragments with positions).
    #[arg(long)]
    dump_layout: bool,

    /// Headless: write element boxes as JSON (for comparisons with Chromium).
    #[arg(long, value_name = "FILE")]
    dump_boxes: Option<PathBuf>,

    /// Viewport size in CSS px, as `WIDTHxHEIGHT`.
    #[arg(long, default_value = "1280x800", value_parser = parse_size)]
    size: Size,

    /// Device pixel ratio for headless rendering (above 0, at most 8).
    #[arg(long, default_value = "1", value_parser = parse_scale)]
    scale: f32,

    /// Seconds to wait for the page to load in headless mode.
    #[arg(long, default_value_t = 30)]
    timeout: u64,

    /// Serve all requests from this fixture directory (no network).
    #[arg(long, value_name = "DIR", conflicts_with = "record")]
    replay: Option<PathBuf>,

    /// Record all responses into this fixture directory.
    #[arg(long, value_name = "DIR")]
    record: Option<PathBuf>,

    /// Use the bundled test fonts instead of the system fonts.
    #[arg(long)]
    test_fonts: bool,
}

/// The largest accepted `--size` in either dimension.
const MAX_VIEWPORT_SIDE: f32 = 16_384.0;

fn parse_size(s: &str) -> Result<Size, String> {
    let (w, h) = s.split_once('x').ok_or("expected WIDTHxHEIGHT")?;
    let w: f32 = w.trim().parse().map_err(|_| "bad width")?;
    let h: f32 = h.trim().parse().map_err(|_| "bad height")?;
    let valid = |v: f32| (1.0..=MAX_VIEWPORT_SIDE).contains(&v);
    if !valid(w) || !valid(h) {
        return Err(format!(
            "width and height must be between 1 and {MAX_VIEWPORT_SIDE}"
        ));
    }
    Ok(Size::new(w, h))
}

fn parse_scale(s: &str) -> Result<f32, String> {
    let scale: f32 = s.trim().parse().map_err(|_| "bad scale")?;
    if scale > 0.0 && scale <= 8.0 {
        Ok(scale)
    } else {
        Err("the scale must be above 0 and at most 8".to_owned())
    }
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args = Args::parse();

    let fetcher: Arc<dyn Fetcher> = match (&args.replay, &args.record) {
        (Some(dir), _) => Arc::new(
            ReplayFetcher::load(dir)
                .with_context(|| format!("cannot load fixture {}", dir.display()))?,
        ),
        (None, Some(dir)) => Arc::new(
            RecordingFetcher::new(NetworkFetcher::new(), dir)
                .with_context(|| format!("cannot record into {}", dir.display()))?,
        ),
        (None, None) => Arc::new(NetworkFetcher::new()),
    };
    let fonts = if args.test_fonts {
        FontContext::for_tests()
    } else {
        FontContext::system()
    };
    let url = args
        .url
        .as_deref()
        .map(url_input::parse_command_line_url)
        .transpose()
        .map_err(anyhow::Error::msg)?;

    if args.headless {
        let Some(url) = url else {
            bail!("headless mode needs a URL");
        };
        let options = headless::HeadlessOptions {
            url,
            viewport: args.size,
            scale: args.scale,
            timeout: Duration::from_secs(args.timeout),
            screenshot: args.screenshot.as_deref(),
            full_page: args.full_page,
            with_chrome: args.with_chrome,
            dump_dom: args.dump_dom,
            dump_layout: args.dump_layout,
            dump_boxes: args.dump_boxes.as_deref(),
        };
        return headless::run(fetcher, fonts, &options);
    }
    gui::run(fetcher, fonts, url)
}
