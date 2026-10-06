//! The protocol methods, executed on the page thread. The method list and
//! the parameters are documented in `docs/automation.md`.

use std::time::{Duration, Instant};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use swb_engine::{
    Key, LoadState, Modifiers, MouseButton, NodeId, Page, Point, Rect, Size, Url, check_scale,
    check_viewport_size,
};
use swb_net::CookieJar;

use crate::protocol::RpcError;

/// What a method call produced.
pub(crate) enum Outcome {
    /// The result or the error.
    Done(Result<Value, RpcError>),
    /// Answer when the page is loaded, or at the deadline.
    WaitForLoad(Instant),
    /// The client asked to close the browser.
    Close,
}

type MethodResult = Result<Value, RpcError>;

/// The default timeout of `page.waitForLoad`.
const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// The longest accepted timeout: one hour.
const MAX_TIMEOUT_MS: u64 = 3_600_000;

/// Executes a method.
pub(crate) fn execute(page: &mut Page, method: &str, params: Value) -> Outcome {
    let result = match method {
        "page.navigate" => navigate(page, params),
        "page.reload" => {
            page.reload();
            Ok(json!({}))
        }
        "page.back" => Ok(json!({ "navigated": page.go_back() })),
        "page.forward" => Ok(json!({ "navigated": page.go_forward() })),
        "page.stop" => {
            page.stop();
            Ok(json!({}))
        }
        "page.waitForLoad" => return wait_for_load(params),
        "page.info" => Ok(info(page)),
        "page.setViewport" => set_viewport(page, params),
        "page.screenshot" => screenshot(page, params),
        "page.scrollTo" => scroll_to(page, params),
        "page.scrollBy" => scroll_by(page, params),
        "dom.querySelectorAll" => query_selector_all(page, params),
        "dom.querySelector" => query_selector(page, params),
        "dom.text" => with_node(
            page,
            params,
            |page, node| json!({ "text": page.element_text(node) }),
        ),
        "dom.outerHtml" => with_node(page, params, |page, node| {
            let html = page
                .document()
                .map_or_else(String::new, |doc| swb_dom::outer_html(doc, node));
            json!({ "html": html })
        }),
        "dom.attributes" => with_node(page, params, attributes),
        "dom.value" => with_node(page, params, |page, node| {
            json!({
                "value": page.control_value(node),
                "checked": page.control_checked(node),
            })
        }),
        "dom.box" => with_node(
            page,
            params,
            |page, node| json!({ "rect": page.element_box(node).map(rect_json) }),
        ),
        "dom.boxes" => {
            page.update_layout();
            let url = page.url().map_or("", Url::as_str).to_owned();
            Ok(crate::box_dump(page, &url))
        }
        "dom.scrollTo" => scroll_element_to(page, params),
        "dom.scrollInfo" => with_node(page, params, scroll_info),
        "input.click" => click(page, params),
        "input.wheel" => wheel(page, params),
        "input.mouseMove" => mouse_move(page, params),
        "input.mouseDown" => mouse_down(page, params),
        "input.mouseUp" => mouse_up(page, params),
        "input.key" => key(page, params),
        "input.type" => type_text(page, params),
        "selection.get" => Ok(json!({ "text": page.selected_text() })),
        "selection.selectAll" => Ok(json!({ "changed": page.select_all() })),
        "selection.clear" => Ok(json!({ "changed": page.clear_selection() })),
        "cookies.get" => Ok(cookies(page)),
        "cookies.clear" => {
            if let Some(jar) = page.cookie_jar() {
                jar.clear();
            }
            Ok(json!({}))
        }
        "browser.close" => return Outcome::Close,
        _ => Err(RpcError::new(
            RpcError::METHOD_NOT_FOUND,
            format!("unknown method {method}"),
        )),
    };
    Outcome::Done(result)
}

/// Parses the parameters of a method. Null counts as an empty object.
fn parse<T: DeserializeOwned>(params: Value) -> Result<T, RpcError> {
    let params = if params.is_null() { json!({}) } else { params };
    serde_json::from_value(params).map_err(|e| RpcError::invalid_params(e.to_string()))
}

#[derive(Deserialize)]
struct UrlParams {
    url: String,
}

fn navigate(page: &mut Page, params: Value) -> MethodResult {
    let p: UrlParams = parse(params)?;
    let url = Url::parse(&p.url).map_err(|e| RpcError::invalid_params(format!("bad URL: {e}")))?;
    page.navigate(url);
    Ok(json!({}))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WaitParams {
    #[serde(default = "default_timeout")]
    timeout_ms: u64,
}

fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT_MS
}

fn wait_for_load(params: Value) -> Outcome {
    match parse::<WaitParams>(params) {
        Ok(p) => {
            let timeout = Duration::from_millis(p.timeout_ms.min(MAX_TIMEOUT_MS));
            Outcome::WaitForLoad(Instant::now() + timeout)
        }
        Err(e) => Outcome::Done(Err(e)),
    }
}

/// All cookies of the session, oldest first. Empty if the fetcher has no
/// cookie jar (fixture replay).
fn cookies(page: &Page) -> Value {
    let cookies: Vec<Value> = page
        .cookie_jar()
        .map(CookieJar::cookies)
        .unwrap_or_default()
        .into_iter()
        .map(|c| {
            json!({
                "name": c.name,
                "value": c.value,
                "domain": c.domain,
                "hostOnly": c.host_only,
                "path": c.path,
                "secure": c.secure,
                "httpOnly": c.http_only,
                "sameSite": c.same_site.as_str(),
                "expires": c.expires,
            })
        })
        .collect();
    json!({ "cookies": cookies })
}

fn info(page: &mut Page) -> Value {
    let content = page.content_size();
    let scroll = page.scroll_position();
    let viewport = page.viewport();
    json!({
        "url": page.url().map(Url::as_str),
        "title": page.title(),
        "loadState": match page.load_state() {
            LoadState::Idle => "idle",
            LoadState::LoadingDocument => "loadingDocument",
            LoadState::LoadingResources => "loadingResources",
            LoadState::Complete => "complete",
            LoadState::Failed => "failed",
        },
        "error": page.error(),
        "scroll": { "x": scroll.x, "y": scroll.y },
        "viewport": { "width": viewport.width, "height": viewport.height, "scale": page.scale() },
        "contentSize": { "width": content.width, "height": content.height },
        "canGoBack": page.can_go_back(),
        "canGoForward": page.can_go_forward(),
        "focusedNode": page.focused_element().map(NodeId::index),
        "hoveredLink": page.hovered_link().map(Url::as_str),
        "cursor": page.cursor().as_str(),
    })
}

#[derive(Deserialize)]
struct ViewportParams {
    width: f32,
    height: f32,
    #[serde(default = "default_scale")]
    scale: f32,
}

fn default_scale() -> f32 {
    1.0
}

fn set_viewport(page: &mut Page, params: Value) -> MethodResult {
    let p: ViewportParams = parse(params)?;
    let size = Size::new(p.width, p.height);
    check_viewport_size(size)
        .and_then(|()| check_scale(p.scale))
        .map_err(|e| RpcError::invalid_params(e.to_string()))?;
    page.set_viewport(size, p.scale);
    Ok(json!({}))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScreenshotParams {
    #[serde(default)]
    full_page: bool,
}

fn screenshot(page: &mut Page, params: Value) -> MethodResult {
    let p: ScreenshotParams = parse(params)?;
    let pixmap = page
        .screenshot(p.full_page)
        .map_err(|e| RpcError::failed(e.to_string()))?;
    let png = pixmap
        .encode_png()
        .map_err(|e| RpcError::failed(format!("cannot encode PNG: {e}")))?;
    Ok(json!({
        "width": pixmap.width(),
        "height": pixmap.height(),
        "png": data_encoding::BASE64.encode(&png),
    }))
}

#[derive(Deserialize)]
struct PointParams {
    x: f32,
    y: f32,
}

#[derive(Deserialize)]
struct DeltaParams {
    dx: f32,
    dy: f32,
}

fn scroll_to(page: &mut Page, params: Value) -> MethodResult {
    let p: PointParams = parse(params)?;
    page.scroll_to(Point::new(finite(p.x)?, finite(p.y)?));
    Ok(scroll_json(page))
}

fn scroll_by(page: &mut Page, params: Value) -> MethodResult {
    let p: DeltaParams = parse(params)?;
    page.scroll_by(finite(p.dx)?, finite(p.dy)?);
    Ok(scroll_json(page))
}

fn scroll_json(page: &Page) -> Value {
    let scroll = page.scroll_position();
    json!({ "scroll": { "x": scroll.x, "y": scroll.y } })
}

/// Rejects coordinates that do not fit in an `f32`.
fn finite(v: f32) -> Result<f32, RpcError> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(RpcError::invalid_params("a coordinate is out of range"))
    }
}

#[derive(Deserialize)]
struct SelectorParams {
    selector: String,
}

fn matching(page: &Page, params: Value) -> Result<Vec<NodeId>, RpcError> {
    let p: SelectorParams = parse(params)?;
    if page.document().is_none() {
        return Err(RpcError::failed("no document is loaded"));
    }
    page.query_selector_all(&p.selector)
        .ok_or_else(|| RpcError::invalid_params(format!("invalid selector: {}", p.selector)))
}

fn query_selector_all(page: &Page, params: Value) -> MethodResult {
    let nodes: Vec<usize> = matching(page, params)?
        .into_iter()
        .map(NodeId::index)
        .collect();
    Ok(json!({ "nodeIds": nodes }))
}

fn query_selector(page: &Page, params: Value) -> MethodResult {
    let node = matching(page, params)?.first().map(|n| n.index());
    Ok(json!({ "nodeId": node }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NodeParams {
    node_id: usize,
}

/// Resolves a node ID of the current document.
fn node(page: &Page, id: usize) -> Result<NodeId, RpcError> {
    let doc = page
        .document()
        .ok_or_else(|| RpcError::failed("no document is loaded"))?;
    NodeId::from_index(id)
        .filter(|&n| doc.get(n).is_some())
        .ok_or_else(|| RpcError::failed(format!("no node {id}")))
}

fn with_node(
    page: &mut Page,
    params: Value,
    f: impl FnOnce(&mut Page, NodeId) -> Value,
) -> MethodResult {
    let p: NodeParams = parse(params)?;
    let node = node(page, p.node_id)?;
    Ok(f(page, node))
}

fn attributes(page: &mut Page, node: NodeId) -> Value {
    let attributes: serde_json::Map<String, Value> = page
        .document()
        .and_then(|doc| doc.element(node))
        .map(|e| {
            e.attributes()
                .iter()
                .map(|a| (a.name.local.to_string(), Value::from(a.value.as_str())))
                .collect()
        })
        .unwrap_or_default();
    json!({ "attributes": attributes })
}

fn rect_json(r: Rect) -> Value {
    json!([r.x, r.y, r.width, r.height])
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScrollElementParams {
    node_id: usize,
    x: f32,
    y: f32,
}

/// Scrolls an element (a scroll container, or the element that scrolls the
/// page: the root element, in quirks mode the body if it is not a scroll
/// container), as the DOM's `scrollTo`.
fn scroll_element_to(page: &mut Page, params: Value) -> MethodResult {
    let p: ScrollElementParams = parse(params)?;
    let node = node(page, p.node_id)?;
    let offset = Point::new(finite(p.x)?, finite(p.y)?);
    page.scroll_element_to(node, offset).ok_or_else(no_box)?;
    Ok(scroll_info(page, node))
}

/// The scroll state of an element: its offset, the size of its
/// scrollable overflow and of its scrollport (`null` without a box).
fn scroll_info(page: &mut Page, node: NodeId) -> Value {
    match page.element_scroll(node) {
        Some(s) => json!({
            "scroll": { "x": s.offset.x, "y": s.offset.y },
            "scrollWidth": s.scroll_size.width,
            "scrollHeight": s.scroll_size.height,
            "clientWidth": s.client_size.width,
            "clientHeight": s.client_size.height,
            "scrollable": s.scrollable,
        }),
        None => Value::Null,
    }
}

#[derive(Deserialize)]
struct WheelParams {
    x: f32,
    y: f32,
    dx: f32,
    dy: f32,
}

/// A mouse wheel event at a point in viewport coordinates.
fn wheel(page: &mut Page, params: Value) -> MethodResult {
    let p: WheelParams = parse(params)?;
    let scrolled = page.wheel(finite(p.x)?, finite(p.y)?, finite(p.dx)?, finite(p.dy)?);
    Ok(json!({ "scrolled": scrolled }))
}

#[derive(Deserialize, Default, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Button {
    #[default]
    Left,
    Middle,
    Right,
    Back,
    Forward,
}

impl From<Button> for MouseButton {
    fn from(b: Button) -> MouseButton {
        match b {
            Button::Left => MouseButton::Primary,
            Button::Middle => MouseButton::Middle,
            Button::Right => MouseButton::Secondary,
            Button::Back => MouseButton::Back,
            Button::Forward => MouseButton::Forward,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MouseParams {
    x: Option<f32>,
    y: Option<f32>,
    node_id: Option<usize>,
    #[serde(default)]
    button: Button,
    #[serde(default = "one")]
    click_count: u32,
    #[serde(default)]
    modifiers: Vec<String>,
}

fn one() -> u32 {
    1
}

impl MouseParams {
    /// The point in viewport coordinates: `x` and `y`, or the center of
    /// the node's box (scrolled into view first, if the viewport or a
    /// scroll container hides the center).
    fn point(&self, page: &mut Page) -> Result<Point, RpcError> {
        match (self.x, self.y, self.node_id) {
            (Some(x), Some(y), None) => Ok(Point::new(finite(x)?, finite(y)?)),
            (None, None, Some(id)) => {
                let node = node(page, id)?;
                match click_target(page, node) {
                    Some((point, true)) => Ok(point),
                    Some(_) => {
                        page.scroll_into_view(node);
                        click_target(page, node).map(|(p, _)| p).ok_or_else(no_box)
                    }
                    None => Err(no_box()),
                }
            }
            _ => Err(RpcError::invalid_params("give x and y, or nodeId")),
        }
    }
}

/// Where a click on `node` goes, in viewport coordinates, and whether the
/// center of its box is visible (inside the viewport and the scrollports
/// of its scroll containers). If the center is hidden but a part of the box
/// is visible, the point is the center of that part. `None` if the node has
/// no box.
fn click_target(page: &mut Page, node: NodeId) -> Option<(Point, bool)> {
    let rect = page.element_box(node)?;
    let scroll = page.scroll_position();
    let viewport = page.viewport();
    let view = Rect::new(scroll.x, scroll.y, viewport.width, viewport.height);
    let clip = page.scroll_clip(node);
    let visible = match clip {
        None => rect.intersection(&view),
        Some(clip) => clip
            .and_then(|c| c.intersection(&rect))
            .and_then(|r| r.intersection(&view)),
    };
    let center = Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    // The intersection excludes boxes without area: their center counts
    // if it is in the view and in the clip of the scroll containers (edges
    // included).
    let in_clip =
        |c: Rect| (c.x..=c.right()).contains(&center.x) && (c.y..=c.bottom()).contains(&center.y);
    let center_visible = match visible {
        Some(v) => v.contains(center),
        None => {
            (rect.width <= 0.0 || rect.height <= 0.0)
                && view.contains(center)
                && clip.is_none_or(|c| c.is_some_and(in_clip))
        }
    };
    let point = match visible {
        Some(v) if !center_visible => Point::new(v.x + v.width / 2.0, v.y + v.height / 2.0),
        _ => center,
    };
    Some((
        Point::new(point.x - scroll.x, point.y - scroll.y),
        center_visible,
    ))
}

fn no_box() -> RpcError {
    RpcError::failed("the node has no box")
}

fn modifiers(names: &[String]) -> Result<Modifiers, RpcError> {
    let mut m = Modifiers::default();
    for name in names {
        match name.as_str() {
            "Shift" => m.shift = true,
            "Control" | "Ctrl" => m.ctrl = true,
            "Alt" => m.alt = true,
            "Meta" => m.meta = true,
            other => {
                return Err(RpcError::invalid_params(format!(
                    "unknown modifier {other}"
                )));
            }
        }
    }
    Ok(m)
}

fn click(page: &mut Page, params: Value) -> MethodResult {
    let p: MouseParams = parse(params)?;
    let point = p.point(page)?;
    let modifiers = modifiers(&p.modifiers)?;
    let button = MouseButton::from(p.button);
    for count in 1..=p.click_count.clamp(1, 3) {
        page.mouse_down(point.x, point.y, button, modifiers, count);
        page.mouse_up(point.x, point.y, button);
    }
    Ok(json!({}))
}

fn mouse_move(page: &mut Page, params: Value) -> MethodResult {
    let p: MouseParams = parse(params)?;
    let point = p.point(page)?;
    page.mouse_move(point.x, point.y);
    Ok(json!({}))
}

fn mouse_down(page: &mut Page, params: Value) -> MethodResult {
    let p: MouseParams = parse(params)?;
    let point = p.point(page)?;
    let modifiers = modifiers(&p.modifiers)?;
    page.mouse_down(
        point.x,
        point.y,
        p.button.into(),
        modifiers,
        p.click_count.max(1),
    );
    Ok(json!({}))
}

fn mouse_up(page: &mut Page, params: Value) -> MethodResult {
    let p: MouseParams = parse(params)?;
    let point = p.point(page)?;
    page.mouse_up(point.x, point.y, p.button.into());
    Ok(json!({}))
}

#[derive(Deserialize)]
struct KeyParams {
    key: String,
    #[serde(default)]
    modifiers: Vec<String>,
}

#[derive(Deserialize)]
struct TypeParams {
    text: String,
}

/// Types text into the focused text field or text area, as keyboard input
/// would: line breaks press Enter (a new line in a text area, implicit
/// submission in a text field) and tabs press Tab (the focus moves to the
/// next element). Typing stops when Enter starts a navigation (the rest
/// would go to the old document) or when the focus leaves editable
/// fields.
fn type_text(page: &mut Page, params: Value) -> MethodResult {
    let p: TypeParams = parse(params)?;
    if !page.has_editable_focus() {
        return Err(RpcError::failed("no editable element is focused"));
    }
    let text = p.text.replace("\r\n", "\n").replace('\r', "\n");
    let mut rest = text.as_str();
    loop {
        let end = rest.find(['\n', '\t']).unwrap_or(rest.len());
        let (chunk, tail) = rest.split_at(end);
        page.insert_text(chunk);
        let Some(separator) = tail.chars().next() else {
            break;
        };
        let key = if separator == '\n' {
            Key::Enter
        } else {
            Key::Tab
        };
        page.key_down(&key, Modifiers::NONE);
        if page.load_state() == LoadState::LoadingDocument || !page.has_editable_focus() {
            break;
        }
        rest = &tail[separator.len_utf8()..];
    }
    Ok(json!({}))
}

fn key(page: &mut Page, params: Value) -> MethodResult {
    let p: KeyParams = parse(params)?;
    let key = Key::from_dom(&p.key)
        .ok_or_else(|| RpcError::invalid_params(format!("unknown key {}", p.key)))?;
    let handled = page.key_down(&key, modifiers(&p.modifiers)?);
    Ok(json!({ "handled": handled }))
}
