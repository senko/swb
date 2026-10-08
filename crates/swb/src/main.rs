//! The swb web browser: command-line parsing and the choice of mode.
//!
//! - Window (default): the GUI in `gui`.
//! - `--headless`: render one page to a screenshot or a dump, or time the
//!   pipeline stages with `--bench` (`headless`, `bench`).
//! - `--remote-port`: serve the automation protocol, with or without a
//!   window.

mod bench;
mod gui;
mod headless;
mod url_input;

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Parser;
use swb_engine::{Size, check_scale, check_viewport_size};
use swb_net::{ExtendingFetcher, Fetcher, NetworkFetcher, RecordingFetcher, ReplayFetcher};
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

    /// Headless: run each pipeline stage N times and print the times as
    /// JSON (see docs/performance.md).
    #[arg(long, value_name = "N")]
    bench: Option<usize>,

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
    #[arg(long, value_name = "DIR", conflicts_with_all = ["record", "record_missing"])]
    replay: Option<PathBuf>,

    /// Record all responses into this fixture directory.
    #[arg(long, value_name = "DIR", conflicts_with = "record_missing")]
    record: Option<PathBuf>,

    /// Serve requests from this fixture directory, and fetch and add the
    /// ones it does not have.
    #[arg(long, value_name = "DIR")]
    record_missing: Option<PathBuf>,

    /// Use the bundled test fonts instead of the system fonts.
    #[arg(long)]
    test_fonts: bool,

    /// Start the automation server on this port of 127.0.0.1 (0 picks a
    /// free port; the port is printed). In headless mode, swb then runs
    /// until a client sends `browser.close`. See docs/automation.md.
    #[arg(
        long,
        value_name = "PORT",
        conflicts_with_all = ["screenshot", "dump_dom", "dump_layout", "dump_boxes", "bench", "with_chrome", "full_page"]
    )]
    remote_port: Option<u16>,
}

fn parse_size(s: &str) -> Result<Size, String> {
    let (w, h) = s.split_once('x').ok_or("expected WIDTHxHEIGHT")?;
    let w: f32 = w.trim().parse().map_err(|_| "bad width")?;
    let h: f32 = h.trim().parse().map_err(|_| "bad height")?;
    let size = Size::new(w, h);
    check_viewport_size(size).map_err(|e| e.to_string())?;
    Ok(size)
}

fn parse_scale(s: &str) -> Result<f32, String> {
    let scale: f32 = s.trim().parse().map_err(|_| "bad scale")?;
    check_scale(scale).map_err(|e| e.to_string())?;
    Ok(scale)
}

/// Prints the address of the automation server on standard output. Tools
/// read this line to find the port (docs/automation.md).
fn print_server_address(port: u16) -> Result<()> {
    println!("swb: automation server listening on ws://127.0.0.1:{port}/");
    std::io::stdout()
        .flush()
        .context("cannot write to standard output")
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args = Args::parse();

    let fetcher: Arc<dyn Fetcher> = match (&args.replay, &args.record, &args.record_missing) {
        (Some(dir), _, _) => Arc::new(
            ReplayFetcher::load(dir)
                .with_context(|| format!("cannot load fixture {}", dir.display()))?,
        ),
        (None, Some(dir), _) => Arc::new(
            RecordingFetcher::new(NetworkFetcher::new(), dir)
                .with_context(|| format!("cannot record into {}", dir.display()))?,
        ),
        (None, None, Some(dir)) => Arc::new(
            ExtendingFetcher::new(NetworkFetcher::new(), dir)
                .with_context(|| format!("cannot extend fixture {}", dir.display()))?,
        ),
        (None, None, None) => Arc::new(NetworkFetcher::new()),
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

    if args.headless
        && let Some(port) = args.remote_port
    {
        return headless::serve(fetcher, fonts, args.size, args.scale, port, url);
    }
    if args.headless {
        let Some(url) = url else {
            bail!("headless mode needs a URL (or --remote-port)");
        };
        let options = headless::HeadlessOptions {
            fetcher,
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
            bench: args.bench,
        };
        return headless::run(fonts, &options);
    }
    gui::run(fetcher, fonts, url, args.remote_port)
}
