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
mod stack;
mod url_input;

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{ArgMatches, CommandFactory, FromArgMatches, Parser};
use swb_engine::{ClockMode, Size, check_scale, check_viewport_size};
use swb_net::{ExtendingFetcher, Fetcher, NetworkFetcher, RecordingFetcher, ReplayFetcher};
use swb_text::FontContext;

/// The global allocator (ADR 0029). Windows with MSVC keeps the system allocator.
#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static ALLOCATOR: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

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

    /// Run the scripts of pages (JavaScript on). Off by default; the
    /// setting applies to the documents that load after it is set. The
    /// GUI keeps it off if the stack limit of the process is below 8 MiB.
    #[arg(long, conflicts_with = "javascript")]
    js: bool,

    /// JavaScript on or off (`--js` is `--javascript on`).
    #[arg(long, value_name = "on|off", value_parser = parse_on_off)]
    javascript: Option<bool>,

    /// Headless: after the page loaded, evaluate EXPR in the page's global
    /// scope and print one JSON line, `{"value": V}` or `{"error": "..."}`
    /// (repeatable; needs `--js`; write `--eval=EXPR` for an EXPR that
    /// starts with `-`). See docs/testing.md, "The `--eval` contract".
    #[arg(
        long = "eval",
        value_name = "EXPR",
        allow_hyphen_values = true,
        requires = "headless"
    )]
    eval: Vec<String>,

    /// Headless: the frame that the `--eval` arguments after this one
    /// evaluate in: CSS selectors of `iframe` elements from the top-level
    /// document, joined by ` >> ` (`#a >> #b`). An empty PATH is the
    /// top-level document again. A path that finds no frame, and a frame
    /// without a script runtime (past a budget of the page, or its runtime
    /// reached the heap limit), give an error line.
    #[arg(
        long = "eval-frame",
        value_name = "PATH",
        allow_hyphen_values = true,
        requires = "headless"
    )]
    eval_frame: Vec<String>,

    /// Headless: after the page loaded, send these input steps (a JSON
    /// list; see `swb_automation::run_steps`) through the `input.*` methods
    /// of the automation API, before the evaluations and the outputs. For
    /// the probe tool (docs/testing.md).
    #[arg(long, value_name = "JSON", requires = "headless")]
    steps: Option<String>,

    /// Use the bundled test fonts instead of the system fonts.
    #[arg(long)]
    test_fonts: bool,

    /// Start the page clock at this time, in milliseconds since the Unix
    /// epoch (`Date.now()` starts there and runs on), instead of the system
    /// time.
    #[arg(long, value_name = "MS", value_parser = parse_now)]
    now: Option<f64>,

    /// Headless: run the page clock in virtual time. Timers do not wait:
    /// when no task runs and no request is pending, the clock jumps to the
    /// next timer (docs/testing.md).
    #[arg(long, requires = "headless")]
    virtual_time: bool,

    /// Headless: after the page loaded, run its timers and other tasks for
    /// this many milliseconds of page time before the evaluations and the
    /// outputs.
    #[arg(long, value_name = "MS", default_value_t = 0, requires = "headless")]
    settle_ms: u64,

    /// Start the automation server on this port of 127.0.0.1 (0 picks a
    /// free port; the port is printed). In headless mode, swb then runs
    /// until a client sends `browser.close`. See docs/automation.md.
    #[arg(
        long,
        value_name = "PORT",
        conflicts_with_all = ["screenshot", "dump_dom", "dump_layout", "dump_boxes", "bench", "with_chrome", "full_page", "eval"]
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

fn parse_on_off(s: &str) -> Result<bool, String> {
    match s {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err("expected `on` or `off`".to_owned()),
    }
}

fn parse_now(s: &str) -> Result<f64, String> {
    let ms: f64 = s
        .trim()
        .parse()
        .map_err(|_| "expected milliseconds since the epoch")?;
    // The range of ECMAScript time values (§21.4.1.22, TimeClip).
    if !ms.is_finite() || ms.abs() > 8.64e15 {
        return Err("the time is outside the range of Date".to_owned());
    }
    Ok(ms)
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

/// The `--eval` expressions in the order of the command line, each with the
/// frame path that the last `--eval-frame` before it set (`None`: the
/// top-level document).
fn eval_requests(matches: &ArgMatches, args: &Args) -> Vec<headless::EvalRequest> {
    let indices = |id: &str| {
        matches
            .indices_of(id)
            .map_or_else(Vec::new, Iterator::collect)
    };
    let mut items: Vec<(usize, bool, &String)> = indices("eval")
        .into_iter()
        .zip(&args.eval)
        .map(|(i, e)| (i, false, e))
        .chain(
            indices("eval_frame")
                .into_iter()
                .zip(&args.eval_frame)
                .map(|(i, p)| (i, true, p)),
        )
        .collect();
    items.sort_by_key(|(i, ..)| *i);
    let mut frame: Option<String> = None;
    let mut requests = Vec::new();
    for (_, is_frame, value) in items {
        if is_frame {
            frame = Some(value.clone()).filter(|p| !p.trim().is_empty());
        } else {
            requests.push(headless::EvalRequest {
                frame: frame.clone(),
                expression: value.clone(),
            });
        }
    }
    requests
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let matches = Args::command().get_matches();
    let args = Args::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    let evals = eval_requests(&matches, &args);

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

    let javascript = args.js || args.javascript == Some(true);
    let mode = if args.virtual_time {
        ClockMode::Virtual
    } else {
        ClockMode::Real
    };
    let clock = (mode, args.now);

    if args.headless
        && let Some(port) = args.remote_port
    {
        return stack::on_page_thread(|| {
            headless::serve(
                fetcher, fonts, args.size, args.scale, port, url, javascript, clock,
            )
        })?;
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
            javascript,
            eval: &evals,
            steps: args.steps.as_deref(),
            clock,
            settle: Duration::from_millis(args.settle_ms),
        };
        return stack::on_page_thread(|| headless::run(fonts, &options))?;
    }
    gui::run(fetcher, fonts, url, args.remote_port, javascript, args.now)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requests(arguments: &[&str]) -> Vec<(Option<String>, String)> {
        let matches = Args::command().get_matches_from(arguments);
        let args = Args::from_arg_matches(&matches).expect("the arguments are valid");
        eval_requests(&matches, &args)
            .into_iter()
            .map(|r| (r.frame, r.expression))
            .collect()
    }

    #[test]
    fn eval_frame_sets_the_frame_of_the_evals_after_it() {
        let got = requests(&[
            "swb",
            "--headless",
            "--js",
            "--eval=a",
            "--eval-frame=#f >> #g",
            "--eval=b",
            "--eval=-c",
            "--eval-frame=",
            "--eval=d",
            "--eval-frame=#h",
            "--eval=e",
            "about:blank",
        ]);
        let frame = |p: &str| Some(p.to_owned());
        assert_eq!(
            got,
            vec![
                (None, "a".to_owned()),
                (frame("#f >> #g"), "b".to_owned()),
                (frame("#f >> #g"), "-c".to_owned()),
                (None, "d".to_owned()),
                (frame("#h"), "e".to_owned()),
            ]
        );
    }

    #[test]
    fn evals_without_a_frame_have_none() {
        let got = requests(&[
            "swb",
            "--headless",
            "--js",
            "--eval=1",
            "--eval=2",
            "about:blank",
        ]);
        assert_eq!(got.len(), 2);
        assert!(got.iter().all(|(frame, _)| frame.is_none()));
    }
}
