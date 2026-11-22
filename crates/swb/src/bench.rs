//! `--bench N`: times each pipeline stage for a loaded page.
//!
//! HTML parsing runs N times on the document's bytes. The other stages
//! (stylesheets, style, layout, display list, raster of the viewport) run N
//! times by discarding all results before each render. After them, the
//! interaction stages move the pointer over a grid (hover) and turn the
//! wheel (scroll), each step with a render, through the page's input paths
//! as the GUI's frame does. The hover and animation steps render with
//! damage into the kept pixmap (`Page::render_update`), as the GUI does, so
//! that their raster times are those of the damage (ADR 0044 section 18).
//! The output is JSON (format: docs/performance.md).

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::json;
use swb_engine::{
    FlushCounters, FrameDamage, Page, Pixmap, Point, RestylePaths, StageTimings, Url,
};
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
    // The style and layout runs of the load, before the runs below.
    let flushes = page.flush_counters();

    let response = fetch_following_redirects(
        fetcher.as_ref(),
        Request::get(url.clone(), Destination::Document),
    )
    .context("cannot fetch the document again")?;
    let charset = response.content_type().and_then(|c| c.charset);
    let scripting = page.document_has_scripting();
    let parse: Vec<Duration> = (0..runs)
        .map(|_| {
            let started = Instant::now();
            let document =
                swb_dom::parse_html_bytes_with(&response.body, charset.as_deref(), scripting);
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
        "flushes": flushes_json(&flushes),
        "interaction": interaction(page, &mut pixmap, runs),
    });
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

/// The style and layout runs of the load (ADR 0028 section 9): all runs,
/// the runs that script queries forced, and the element style runs.
fn flushes_json(c: &FlushCounters) -> serde_json::Value {
    let runs = |n: u32, d: Duration| json!({ "runs": n, "ms": ms(d) });
    json!({
        "style": runs(c.style_runs, c.style_time),
        "layout": runs(c.layout_runs, c.layout_time),
        "forced_style": runs(c.forced_style_runs, c.forced_style_time),
        "forced_layout": runs(c.forced_layout_runs, c.forced_layout_time),
        "element_style": runs(c.element_style_runs, c.element_style_time),
        "stylist": runs(c.stylist_runs, c.stylist_time),
        "sheet_changes": c.sheet_changes,
        "container_passes": c.container_passes,
        "container_pass_limits": c.container_pass_limits,
        "state_restyles": restyle_paths_json(&c.state_restyles),
        "dom_restyles": restyle_paths_json(&c.dom_restyles),
    })
}

/// The paths of the restyles after state changes (ADR 0041) or after DOM
/// changes (ADR 0042).
fn restyle_paths_json(r: &RestylePaths) -> serde_json::Value {
    json!({
        "targeted": r.targeted,
        "nothing": r.nothing,
        "restyled_elements": r.restyled_elements,
        "full_other_states": r.other_states,
        "full_everything": r.everything,
        "full_root_font_size": r.root_font_size,
        "full_containers": r.containers,
        "full_counters": r.counters,
        "full_too_large": r.too_large,
        "full_structure": r.structure,
    })
}

/// The hover grid: columns and rows of points over the viewport.
const HOVER_GRID: (u32, u32) = (8, 6);

/// The most rounds over the hover grid (a round is 48 steps).
const MAX_HOVER_ROUNDS: usize = 5;

/// The wheel step of the scroll stage, in CSS px, and the most steps in
/// one direction.
const WHEEL_STEP: f32 = 100.0;
const MAX_WHEEL_STEPS: usize = 20;

/// One step of an interaction stage: its time (the input and the render)
/// and the style and layout runs it caused.
struct Step {
    time: Duration,
    flushes: FlushCounters,
    /// What the render drew, and the times of its raster and of the
    /// comparison of the lists; `None` without a render.
    render: Option<(FrameDamage, Duration, Duration)>,
}

impl Step {
    /// Runs `input` (which returns true if the page needs a repaint), then
    /// renders if it does, as the GUI does: in full with `Render::Full`,
    /// else only the damage into `pixmap`, which holds the last frame. The
    /// GUI's composition of the window (toolbar, copies) is not part of the
    /// time.
    fn run(
        page: &mut Page,
        pixmap: &mut Pixmap,
        how: Render,
        input: impl FnOnce(&mut Page) -> bool,
    ) -> Step {
        let before = page.flush_counters();
        let started = Instant::now();
        let render = input(page).then(|| {
            let damage = match how {
                Render::Full => {
                    page.render(pixmap);
                    FrameDamage {
                        full: true,
                        rects: Vec::new(),
                        viewport_pixels: u64::from(pixmap.width()) * u64::from(pixmap.height()),
                    }
                }
                Render::Damage => page.render_update(pixmap),
            };
            let timings = page.timings();
            (damage, timings.raster, timings.damage)
        });
        let time = started.elapsed();
        Step {
            time,
            flushes: delta(&before, &page.flush_counters()),
            render,
        }
    }

    /// True if the step ran a style pass, full or targeted (ADR 0041): the
    /// input changed a style.
    fn restyled(&self) -> bool {
        self.flushes.style_runs > 0
    }
}

/// How a step renders.
#[derive(Clone, Copy)]
enum Render {
    /// The whole viewport.
    Full,
    /// The damage, into the kept pixmap.
    Damage,
}

/// The raster of the steps that rendered: the median raster and diff
/// times, the frames by kind and the median pixels of the region frames.
fn raster_json(steps: &[&Step]) -> serde_json::Value {
    let renders: Vec<&(FrameDamage, Duration, Duration)> =
        steps.iter().filter_map(|s| s.render.as_ref()).collect();
    let regions: Vec<&FrameDamage> = renders
        .iter()
        .map(|r| &r.0)
        .filter(|d| !d.full && !d.rects.is_empty())
        .collect();
    let mut pixels: Vec<u64> = regions.iter().map(|d| d.pixels()).collect();
    pixels.sort_unstable();
    json!({
        "raster_ms": ms(median(renders.iter().map(|r| r.1).collect())),
        "raster_p90_ms": ms(percentile(renders.iter().map(|r| r.1).collect(), 0.9)),
        "damage_ms": ms(median(renders.iter().map(|r| r.2).collect())),
        "full_frames": renders.iter().filter(|r| r.0.full).count(),
        "region_frames": regions.len(),
        "empty_frames": renders.iter().filter(|r| !r.0.full && r.0.rects.is_empty()).count(),
        "region_pixels_median": pixels.get(pixels.len() / 2).copied().unwrap_or(0),
    })
}

/// The runs and times between two readings of the counters (only the
/// fields that the interaction stages report).
fn delta(before: &FlushCounters, after: &FlushCounters) -> FlushCounters {
    FlushCounters {
        style_runs: after.style_runs - before.style_runs,
        style_time: after.style_time.saturating_sub(before.style_time),
        layout_runs: after.layout_runs - before.layout_runs,
        layout_time: after.layout_time.saturating_sub(before.layout_time),
        display_list_runs: after.display_list_runs - before.display_list_runs,
        paint_refreshes: after.paint_refreshes - before.paint_refreshes,
        paint_time: after.paint_time.saturating_sub(before.paint_time),
        paint_fallbacks: after.paint_fallbacks - before.paint_fallbacks,
        state_restyles: after.state_restyles.since(&before.state_restyles),
        ..FlushCounters::default()
    }
}

/// The interaction stages (JSON: docs/performance.md, "Measuring").
fn interaction(page: &mut Page, pixmap: &mut Pixmap, runs: usize) -> serde_json::Value {
    let rounds = runs.min(MAX_HOVER_ROUNDS);
    json!({
        "animation": animation_stage(page, pixmap),
        "hover": hover_stage(page, pixmap, rounds),
        "scroll": scroll_stage(page, pixmap),
    })
}

/// The most frames of the animation stage, and its time limit.
const ANIMATION_FRAMES: usize = 30;
const ANIMATION_LIMIT: Duration = Duration::from_secs(3);

/// Animation frames (ADR 0044 section 8): with the page's frames painted,
/// up to [`ANIMATION_FRAMES`] frames of real time, each the page's pump at
/// its next frame (the rendering update) and a render if the frame is
/// outdated, as the GUI's frame does. Nothing without a running animation
/// (`--animations off`). The median time of a frame and of the stages that
/// ran in it.
fn animation_stage(page: &mut Page, pixmap: &mut Pixmap) -> serde_json::Value {
    page.set_paints_frames(true);
    let mut steps = Vec::new();
    let mut stages: Vec<StageTimings> = Vec::new();
    let started = Instant::now();
    while page.animations_running()
        && steps.len() < ANIMATION_FRAMES
        && started.elapsed() < ANIMATION_LIMIT
    {
        if let Some(next) = page.pump().next {
            std::thread::sleep(next.saturating_duration_since(Instant::now()));
        }
        let step = Step::run(page, pixmap, Render::Damage, |p| p.frame_outdated());
        if step.restyled() {
            stages.push(page.timings());
            steps.push(step);
        }
    }
    page.set_paints_frames(false);
    let median_of = |f: fn(&StageTimings) -> Duration| ms(median(stages.iter().map(f).collect()));
    let rendered: Vec<&Step> = steps.iter().collect();
    json!({
        "render": raster_json(&rendered),
        "frames": steps.len(),
        "median_ms": ms(median(steps.iter().map(|s| s.time).collect())),
        // The style and layout times of the runs in the frame, so a frame
        // without a layout shows 0 (the stage timings keep the last run).
        "style_ms": ms(median(steps.iter().map(|s| s.flushes.style_time).collect())),
        "layout_ms": ms(median(steps.iter().map(|s| s.flushes.layout_time).collect())),
        "display_list_ms": median_of(|t| t.display_list),
        "raster_ms": median_of(|t| t.raster),
        "restyled_elements": steps.iter().map(|s| s.flushes.state_restyles.restyled_elements).sum::<u64>(),
        "paint_only_frames": steps.iter().filter(|s| s.flushes.paint_refreshes > 0).count(),
        "layout_runs": steps.iter().map(|s| s.flushes.layout_runs).sum::<u32>(),
        "paint_ms": ms(steps.iter().map(|s| s.flushes.paint_time).sum()),
    })
}

/// Hover: a grid of points inset from the edges of the viewport, visited
/// row by row for `rounds` rounds; each step is `Page::mouse_move` and a
/// render. A step "changed" if it ran a style pass (full or targeted). The
/// stage reports the paths of the restyles after state changes
/// (`state_restyles`) only, not those after DOM changes (`dom_restyles`,
/// which the load and the scripts of the page cause; see `flushes`).
fn hover_stage(page: &mut Page, pixmap: &mut Pixmap, rounds: usize) -> serde_json::Value {
    let viewport = page.viewport();
    let (columns, rows) = HOVER_GRID;
    let points: Vec<(f32, f32)> = (0..rows)
        .flat_map(|row| (0..columns).map(move |column| (column, row)))
        .map(|(column, row)| {
            (
                viewport.width * (column as f32 + 0.5) / columns as f32,
                viewport.height * (row as f32 + 0.5) / rows as f32,
            )
        })
        .collect();
    // Start with the pointer outside the page and everything up to date.
    page.mouse_leave();
    page.render(pixmap);
    let mut steps = Vec::with_capacity(points.len() * rounds);
    for _ in 0..rounds {
        for &(x, y) in &points {
            steps.push(Step::run(page, pixmap, Render::Damage, |p| {
                p.mouse_move(x, y)
            }));
        }
    }
    let changed: Vec<&Step> = steps.iter().filter(|s| s.restyled()).collect();
    let per_change = |total: f64| (!changed.is_empty()).then(|| total / changed.len() as f64);
    let sum = |f: fn(&Step) -> f64| changed.iter().map(|s| f(s)).sum::<f64>();
    let times = |steps: &[&Step]| steps.iter().map(|s| s.time).collect::<Vec<_>>();
    let all: Vec<&Step> = steps.iter().collect();
    json!({
        "render": raster_json(&changed),
        "grid": [columns, rows],
        "rounds": rounds,
        "steps": steps.len(),
        "changed": changed.len(),
        "changed_median_ms": ms(median(times(&changed))),
        "changed_p90_ms": ms(percentile(times(&changed), 0.9)),
        "median_ms": ms(median(times(&all))),
        "p90_ms": ms(percentile(times(&all), 0.9)),
        "style_runs_per_change": per_change(sum(|s| f64::from(s.flushes.style_runs))),
        "layout_runs_per_change": per_change(sum(|s| f64::from(s.flushes.layout_runs))),
        "style_ms_per_change": per_change(sum(|s| s.flushes.style_time.as_secs_f64() * 1e3)),
        "layout_ms_per_change": per_change(sum(|s| s.flushes.layout_time.as_secs_f64() * 1e3)),
        "paint_only_changes": changed.iter().filter(|s| s.flushes.paint_refreshes > 0).count(),
        "paint_fallbacks": changed.iter().map(|s| u64::from(s.flushes.paint_fallbacks)).sum::<u64>(),
        "paint_ms_per_change": per_change(sum(|s| s.flushes.paint_time.as_secs_f64() * 1e3)),
        "state_restyles": restyle_paths_json(&steps.iter().fold(
            RestylePaths::default(),
            |total, s| total.plus(&s.flushes.state_restyles)
        )),
    })
}

/// Scroll: wheel steps of [`WHEEL_STEP`] px down from the top until the end
/// of the page or [`MAX_WHEEL_STEPS`] steps, then the same number up
/// (or until the top); each step is `Page::wheel` at the middle of the
/// viewport and a render.
fn scroll_stage(page: &mut Page, pixmap: &mut Pixmap) -> serde_json::Value {
    let viewport = page.viewport();
    let (x, y) = (viewport.width / 2.0, viewport.height / 2.0);
    page.scroll_to(Point::new(0.0, 0.0));
    page.render(pixmap);
    let mut steps = Vec::new();
    for direction in [1.0, -1.0] {
        for _ in 0..MAX_WHEEL_STEPS {
            let before = page.scroll_position();
            steps.push(Step::run(page, pixmap, Render::Full, |p| {
                p.wheel(x, y, 0.0, direction * WHEEL_STEP)
            }));
            if page.scroll_position() == before {
                // At the end of the page: this step did not scroll.
                steps.pop();
                break;
            }
        }
    }
    let times: Vec<Duration> = steps.iter().map(|s| s.time).collect();
    let total = steps.iter().fold(FlushCounters::default(), |mut total, s| {
        total.style_runs += s.flushes.style_runs;
        total.style_time += s.flushes.style_time;
        total.layout_runs += s.flushes.layout_runs;
        total.layout_time += s.flushes.layout_time;
        total
    });
    json!({
        "steps": steps.len(),
        "median_ms": ms(median(times.clone())),
        "p90_ms": ms(percentile(times, 0.9)),
        "style_runs": total.style_runs,
        "layout_runs": total.layout_runs,
        "style_ms": ms(total.style_time),
        "layout_ms": ms(total.layout_time),
    })
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

/// The value at the nearest rank `fraction` (0 to 1) of the sorted
/// `values`; zero for no values.
fn percentile(mut values: Vec<Duration>, fraction: f64) -> Duration {
    values.sort_unstable();
    let rank = (values.len() as f64 * fraction).ceil() as usize;
    values
        .get(rank.saturating_sub(1).min(values.len().saturating_sub(1)))
        .copied()
        .unwrap_or_default()
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

    #[test]
    fn percentile_is_the_nearest_rank() {
        let d = Duration::from_millis;
        let values: Vec<Duration> = (1..=10).map(d).collect();
        assert_eq!(percentile(values.clone(), 0.9), d(9));
        assert_eq!(percentile(values.clone(), 1.0), d(10));
        assert_eq!(percentile(values, 0.0), d(1));
        assert_eq!(percentile(vec![d(5)], 0.9), d(5));
        assert_eq!(percentile(Vec::new(), 0.9), Duration::ZERO);
    }

    /// The interaction stages on a page with a hover rule and a long body:
    /// the hover steps over the box restyle, the scroll steps move.
    #[test]
    fn interaction_stages_on_a_small_page() {
        let html = "<!doctype html><style>body{margin:0}\
            #a{height:300px;background:#ccc}#a:hover{background:#999}\
            #b{height:3000px}</style><div id=a></div><div id=b></div>";
        let path = std::env::temp_dir().join(format!("swb-bench-{}.html", std::process::id()));
        std::fs::write(&path, html).expect("the temporary directory is writable");
        let url = Url::from_file_path(&path).expect("an absolute path");
        let result = crate::stack::on_page_thread(|| {
            let config = swb_engine::PageConfig {
                fetcher: Arc::new(swb_net::NetworkFetcher::new()),
                notify: Arc::new(|| {}),
                network_threads: 1,
                scripts_allowed: true,
            };
            let mut page = Page::new(
                config,
                swb_engine::FontContext::for_tests(),
                swb_engine::Size::new(800.0, 600.0),
                1.0,
            );
            page.navigate(url);
            assert!(page.wait_until_loaded(Duration::from_secs(10)));
            let mut pixmap = viewport_pixmap(&page).expect("a small pixmap");
            interaction(&mut page, &mut pixmap, 2)
        })
        .expect("the page thread runs");
        std::fs::remove_file(&path).ok();
        let hover = &result["hover"];
        assert_eq!(hover["steps"], 96);
        // Moving between the box and the rest changes the hover state at
        // the edges only: some steps, not all.
        let changed = hover["changed"].as_u64().expect("a count");
        assert!((1..96).contains(&changed), "changed: {changed}");
        assert!(hover["style_runs_per_change"].as_f64().expect("a number") >= 1.0);
        let scroll = &result["scroll"];
        // 3300 px of content in a 600 px viewport: 20 steps down, then up.
        assert_eq!(scroll["steps"], 40);
    }
}
