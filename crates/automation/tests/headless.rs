//! Drives a headless browser through the automation protocol: the
//! senko.net fixture and local pages. Nothing uses the network.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{Value, json};
use swb_automation::{Client, ClientError, HeadlessBrowser, RpcError};
use swb_engine::{FontContext, Size};
use swb_net::{Fetcher, NetworkFetcher, ReplayFetcher};

const TIMEOUT: Duration = Duration::from_secs(30);

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Starts a headless browser (1280×800, test fonts) on a free port on its
/// own thread. Returns the thread and the port.
fn spawn(fetcher: Arc<dyn Fetcher>) -> (JoinHandle<()>, u16) {
    let (sender, port) = mpsc::channel();
    let thread = thread::spawn(move || {
        let size = Size::new(1280.0, 800.0);
        let browser =
            HeadlessBrowser::new(fetcher, FontContext::for_tests(), size, 1.0, 0).unwrap();
        sender.send(browser.port()).unwrap();
        browser.run();
    });
    (thread, port.recv().unwrap())
}

/// Starts a headless browser and connects a client.
fn start(fetcher: Arc<dyn Fetcher>) -> (Client, JoinHandle<()>) {
    let (thread, port) = spawn(fetcher);
    (Client::connect(port).unwrap(), thread)
}

fn stop(client: Client, thread: JoinHandle<()>) {
    client.close_browser().unwrap();
    thread.join().unwrap();
}

fn senko_net() -> (Client, JoinHandle<()>) {
    let fixture = ReplayFetcher::load(repo_root().join("fixtures/pages/senko-net")).unwrap();
    let (mut client, thread) = start(Arc::new(fixture));
    client.navigate("https://senko.net/").unwrap();
    assert!(client.wait_for_load(TIMEOUT).unwrap());
    (client, thread)
}

/// The box `[x, y, width, height]` of a node (`dom.box`), in document
/// coordinates.
fn rect(client: &mut Client, node: u64) -> [f64; 4] {
    let value = client.call("dom.box", json!({ "nodeId": node })).unwrap()["rect"].clone();
    std::array::from_fn(|i| value[i].as_f64().unwrap())
}

/// The viewport coordinates of the center of a node's box.
fn center(client: &mut Client, node: u64) -> (f32, f32) {
    let [x, y, width, height] = rect(client, node).map(|v| v as f32);
    let scroll_y = client.info().unwrap()["scroll"]["y"].as_f64().unwrap() as f32;
    (x + width / 2.0, y + height / 2.0 - scroll_y)
}

/// A fetcher that fails every request with a timeout after a delay.
struct Slow(Duration);

impl Fetcher for Slow {
    fn fetch(&self, _: &swb_net::Request) -> Result<swb_net::Response, swb_net::NetError> {
        thread::sleep(self.0);
        Err(swb_net::NetError::Timeout("slow".to_owned()))
    }
}

fn rpc_code(result: Result<Value, ClientError>) -> i64 {
    match result {
        Err(ClientError::Rpc(e)) => e.code,
        other => panic!("expected an RPC error, got {other:?}"),
    }
}

#[test]
fn loads_and_inspects_senko_net() {
    let (mut client, thread) = senko_net();
    let info = client.info().unwrap();
    assert_eq!(info["title"], "Senko's corner of the Web");
    assert_eq!(info["loadState"], "complete");
    assert_eq!(info["url"], "https://senko.net/");
    let links = client.query_selector_all("main a").unwrap();
    assert_eq!(links.len(), 28);
    let first = client.query_selector("p").unwrap().unwrap();
    assert_eq!(client.text(first).unwrap(), "Hey there!");
    let attributes = client
        .call("dom.attributes", json!({ "nodeId": links[0] }))
        .unwrap();
    assert_eq!(
        attributes["attributes"]["href"],
        "https://en.wikipedia.org/wiki/Zagreb"
    );
    let html = client
        .call("dom.outerHtml", json!({ "nodeId": links[0] }))
        .unwrap();
    assert_eq!(
        html["html"],
        "<a href=\"https://en.wikipedia.org/wiki/Zagreb\">Zagreb</a>"
    );
    // The box dump has the format of the Chromium reference.
    let boxes = client.call("dom.boxes", Value::Null).unwrap();
    let reference: Value = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("fixtures/pages/senko-net/reference/boxes.json"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        boxes["elements"].as_array().unwrap().len(),
        reference["elements"].as_array().unwrap().len()
    );
    stop(client, thread);
}

#[test]
fn hover_underlines_links() {
    let (mut client, thread) = senko_net();
    let link = client.query_selector("main a").unwrap().unwrap();
    let before = client.screenshot(false).unwrap();
    let (x, y) = center(&mut client, link);
    client.mouse_move(x, y).unwrap();
    let info = client.info().unwrap();
    assert_eq!(info["hoveredLink"], "https://en.wikipedia.org/wiki/Zagreb");
    assert_eq!(info["cursor"], "pointer");
    let hovered = client.screenshot(false).unwrap();
    assert_ne!(before, hovered);
    let image = swb_paint::decode(&hovered).unwrap();
    let pixmap = image.raster().unwrap();
    assert_eq!((pixmap.width(), pixmap.height()), (1280, 800));
    // Over plain text, the text cursor.
    let paragraph = client.query_selector("p").unwrap().unwrap();
    let [x, y, ..] = rect(&mut client, paragraph);
    client.mouse_move(x as f32 + 5.0, y as f32 + 10.0).unwrap();
    assert_eq!(client.info().unwrap()["cursor"], "text");
    stop(client, thread);
}

#[test]
fn links_navigate_and_history_goes_back() {
    let (mut client, thread) = senko_net();
    let link = client.query_selector("main a").unwrap().unwrap();
    client.click_node(link).unwrap();
    // The target is not in the fixture: the page shows an error.
    assert!(client.wait_for_load(TIMEOUT).unwrap());
    let info = client.info().unwrap();
    assert_eq!(info["url"], "https://en.wikipedia.org/wiki/Zagreb");
    assert_eq!(info["loadState"], "failed");
    assert_eq!(info["canGoBack"], true);
    let back = client.call("page.back", Value::Null).unwrap();
    assert_eq!(back["navigated"], true);
    assert!(client.wait_for_load(TIMEOUT).unwrap());
    assert_eq!(client.info().unwrap()["url"], "https://senko.net/");
    stop(client, thread);
}

#[test]
fn keyboard_focus_and_selection() {
    let (mut client, thread) = senko_net();
    assert!(client.key("Tab", &[]).unwrap());
    let focused = client.info().unwrap()["focusedNode"].as_u64();
    assert_eq!(focused, client.query_selector("main a").unwrap());
    assert!(client.key("a", &["Control"]).unwrap());
    let selection = client.call("selection.get", Value::Null).unwrap();
    let text = selection["text"].as_str().unwrap();
    assert!(
        text.starts_with("Senko's corner of the Web\n\nHey there!\n\nI'm Senko Rašić."),
        "{text}"
    );
    assert!(text.ends_with("~ est. 2000 ~"), "{text}");
    client.call("selection.clear", Value::Null).unwrap();
    let selection = client.call("selection.get", Value::Null).unwrap();
    assert_eq!(selection["text"], "");
    // A double click selects a word: "there" in "Hey there!", about 45 px
    // from the start of the line.
    let first = client.query_selector("p").unwrap().unwrap();
    let [x, y, ..] = rect(&mut client, first);
    let (x, y) = (x + 45.0, y + 10.0);
    client
        .call("input.click", json!({ "x": x, "y": y, "clickCount": 2 }))
        .unwrap();
    let selection = client.call("selection.get", Value::Null).unwrap();
    assert_eq!(selection["text"], "there");
    stop(client, thread);
}

#[test]
fn scrolling_and_viewport() {
    let (mut client, thread) = senko_net();
    client
        .call("page.setViewport", json!({ "width": 400, "height": 300 }))
        .unwrap();
    let result = client
        .call("page.scrollBy", json!({ "dx": 0, "dy": 100 }))
        .unwrap();
    assert_eq!(result["scroll"]["y"], 100.0);
    let info = client.info().unwrap();
    assert_eq!(info["viewport"]["width"], 400.0);
    // The narrow layout (max-width: 480px media query) is taller than the
    // wide one (879 px).
    assert!(info["contentSize"]["height"].as_f64().unwrap() > 1400.0);
    let full = client.screenshot(true).unwrap();
    let image = swb_paint::decode(&full).unwrap();
    let pixmap = image.raster().unwrap();
    assert_eq!(pixmap.width(), 400);
    assert!(pixmap.height() > 1400);
    stop(client, thread);
}

#[test]
fn a_click_on_a_box_taller_than_its_scrollport_hits_its_visible_part() {
    // The label is partly visible; scroll into view puts its top at the
    // top of the scrollport, and its center stays hidden below.
    let (mut client, thread) = start(Arc::new(NetworkFetcher::new()));
    client
        .navigate(
            "data:text/html,<body style='margin:0'><input type=checkbox id=c>\
             <div id=s style='overflow:auto;height:100px;width:200px'>\
             <div style='height:60px'></div>\
             <label for=c id=l style='display:block;height:300px'>label</label>\
             <div style='height:300px'></div></div><div style='height:2000px'></div>",
        )
        .unwrap();
    assert!(client.wait_for_load(TIMEOUT).unwrap());
    let label = client.query_selector("#l").unwrap().unwrap();
    client.click_node(label).unwrap();
    let checkbox = client.query_selector("#c").unwrap().unwrap();
    let value = client
        .call("dom.value", json!({ "nodeId": checkbox }))
        .unwrap();
    assert_eq!(value["checked"], true);
    stop(client, thread);
}

#[test]
fn scrolling_elements() {
    let (mut client, thread) = start(Arc::new(NetworkFetcher::new()));
    client
        .navigate(
            "data:text/html,<body style='margin:0'><div id=s style='overflow:auto;height:100px;\
             width:200px'><div style='height:300px'></div><input type=checkbox id=c></div>\
             <div style='height:2000px'></div>",
        )
        .unwrap();
    assert!(client.wait_for_load(TIMEOUT).unwrap());
    let s = client.query_selector("#s").unwrap().unwrap();
    let info = client
        .call("dom.scrollInfo", json!({ "nodeId": s }))
        .unwrap();
    assert_eq!(info["scroll"], json!({ "x": 0.0, "y": 0.0 }));
    assert_eq!(info["clientHeight"], 100.0);
    assert_eq!(info["scrollable"], true);
    let max = info["scrollHeight"].as_f64().unwrap() - 100.0;
    assert!(max > 200.0, "{info}");
    // The wheel over the container scrolls it, not the page.
    let wheel = client
        .call(
            "input.wheel",
            json!({ "x": 50, "y": 50, "dx": 0, "dy": 60 }),
        )
        .unwrap();
    assert_eq!(wheel["scrolled"], true);
    let info = client
        .call("dom.scrollInfo", json!({ "nodeId": s }))
        .unwrap();
    assert_eq!(info["scroll"]["y"], 60.0);
    assert_eq!(client.info().unwrap()["scroll"]["y"], 0.0);
    let scrolled = client
        .call("dom.scrollTo", json!({ "nodeId": s, "x": 0, "y": 1e6 }))
        .unwrap();
    assert_eq!(scrolled["scroll"]["y"].as_f64(), Some(max));
    client
        .call("dom.scrollTo", json!({ "nodeId": s, "x": 0, "y": 0 }))
        .unwrap();
    // A click on a node that the container hides scrolls it into view.
    let checkbox = client.query_selector("#c").unwrap().unwrap();
    client.click_node(checkbox).unwrap();
    let value = client
        .call("dom.value", json!({ "nodeId": checkbox }))
        .unwrap();
    assert_eq!(value["checked"], true);
    let info = client
        .call("dom.scrollInfo", json!({ "nodeId": s }))
        .unwrap();
    assert!(info["scroll"]["y"].as_f64().unwrap() > 0.0, "{info}");
    assert_eq!(
        rpc_code(client.call("dom.scrollTo", json!({ "nodeId": s }))),
        RpcError::INVALID_PARAMS
    );
    assert_eq!(
        rpc_code(client.call("dom.scrollInfo", json!({ "nodeId": 999_999 }))),
        RpcError::FAILED
    );
    stop(client, thread);
}

#[test]
fn errors() {
    let (mut client, thread) = start(Arc::new(NetworkFetcher::new()));
    assert_eq!(
        rpc_code(client.call("no.such", Value::Null)),
        RpcError::METHOD_NOT_FOUND
    );
    assert_eq!(
        rpc_code(client.call("page.navigate", json!({ "url": 5 }))),
        RpcError::INVALID_PARAMS
    );
    assert_eq!(
        rpc_code(client.call("page.navigate", json!({ "url": "not a url" }))),
        RpcError::INVALID_PARAMS
    );
    assert_eq!(
        rpc_code(client.call("dom.querySelectorAll", json!({ "selector": "p" }))),
        RpcError::FAILED
    );
    assert_eq!(
        rpc_code(client.call("input.key", json!({ "key": "NoSuchKey" }))),
        RpcError::INVALID_PARAMS
    );
    assert_eq!(
        rpc_code(client.call("page.setViewport", json!({ "width": 0, "height": 10 }))),
        RpcError::INVALID_PARAMS
    );
    client.navigate("data:text/html,<p id=p>x</p>").unwrap();
    assert!(client.wait_for_load(TIMEOUT).unwrap());
    assert_eq!(
        rpc_code(client.call("dom.text", json!({ "nodeId": 999_999 }))),
        RpcError::FAILED
    );
    assert_eq!(
        rpc_code(client.call("dom.querySelectorAll", json!({ "selector": "p[" }))),
        RpcError::INVALID_PARAMS
    );
    assert_eq!(
        rpc_code(client.call("input.click", json!({ "x": 1 }))),
        RpcError::INVALID_PARAMS
    );
    stop(client, thread);
}

#[test]
fn wait_for_load_times_out() {
    // The fetcher does not answer within the test.
    let (mut client, thread) = start(Arc::new(Slow(Duration::from_secs(5))));
    client.navigate("https://slow.test/").unwrap();
    assert!(!client.wait_for_load(Duration::from_millis(100)).unwrap());
    assert_eq!(client.info().unwrap()["loadState"], "loadingDocument");
    stop(client, thread);
}

#[test]
fn malformed_messages_and_web_pages_are_rejected() {
    let (thread, port) = spawn(Arc::new(NetworkFetcher::new()));
    let client = Client::connect(port).unwrap();
    {
        // A second connection on the same server.
        let (mut raw, _) = tungstenite::connect(format!("ws://127.0.0.1:{port}/")).unwrap();
        raw.send(tungstenite::Message::text("{not json")).unwrap();
        let reply: Value = serde_json::from_str(raw.read().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(reply["error"]["code"], RpcError::PARSE_ERROR);
        raw.send(tungstenite::Message::text("{\"id\": 1}")).unwrap();
        let reply: Value = serde_json::from_str(raw.read().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(reply["error"]["code"], RpcError::INVALID_REQUEST);
    }
    // Browsers send an Origin header: a web page cannot connect.
    let mut request = tungstenite::client::IntoClientRequest::into_client_request(format!(
        "ws://127.0.0.1:{port}/"
    ))
    .unwrap();
    request
        .headers_mut()
        .insert("Origin", "https://evil.test".parse().unwrap());
    assert!(tungstenite::connect(request).is_err());
    stop(client, thread);
}

#[test]
fn a_waiting_request_does_not_block_other_connections() {
    // The fetcher answers (with an error) after 3 s.
    let (thread, port) = spawn(Arc::new(Slow(Duration::from_secs(3))));
    let mut waiting = Client::connect(port).unwrap();
    waiting.navigate("https://slow.test/").unwrap();
    let waiter = thread::spawn(move || {
        let loaded = waiting.wait_for_load(TIMEOUT).unwrap();
        (waiting, loaded)
    });
    thread::sleep(Duration::from_millis(100));
    let mut other = Client::connect(port).unwrap();
    let started = std::time::Instant::now();
    assert_eq!(other.info().unwrap()["loadState"], "loadingDocument");
    assert!(started.elapsed() < Duration::from_secs(1));
    let (waiting, loaded) = waiter.join().unwrap();
    assert!(loaded);
    drop(waiting);
    stop(other, thread);
}

#[test]
fn large_requests_close_the_connection() {
    use std::io::Write;
    let (thread, port) = spawn(Arc::new(NetworkFetcher::new()));
    let stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    let (mut raw, _) = tungstenite::client(format!("ws://127.0.0.1:{port}/"), stream).unwrap();
    // The header of a masked text frame of 2 MB; the server closes the
    // connection with the close code 1009 ("message too big") after it.
    let mut header = vec![0x81, 0xff];
    header.extend_from_slice(&2_000_000_u64.to_be_bytes());
    header.extend_from_slice(&[1, 2, 3, 4]);
    raw.get_mut().write_all(&header).unwrap();
    match raw.read() {
        Ok(tungstenite::Message::Close(Some(frame))) => {
            assert_eq!(
                frame.code,
                tungstenite::protocol::frame::coding::CloseCode::Size
            );
        }
        other => panic!("expected a close frame, got {other:?}"),
    }
    // The server still serves other clients.
    let client = Client::connect(port).unwrap();
    stop(client, thread);
}

#[test]
fn connections_are_limited() {
    let (thread, port) = spawn(Arc::new(NetworkFetcher::new()));
    let url = format!("ws://127.0.0.1:{port}/");
    let mut open: Vec<_> = (0..16)
        .map(|_| tungstenite::connect(url.as_str()).unwrap())
        .collect();
    assert!(tungstenite::connect(url.as_str()).is_err());
    // After one connection closes, a new one works.
    open.pop();
    let client = (0..50)
        .find_map(|_| {
            thread::sleep(Duration::from_millis(20));
            Client::connect(port).ok()
        })
        .expect("a connection after one closed");
    drop(open);
    stop(client, thread);
}

#[test]
fn the_server_stops_with_the_browser() {
    let (thread, port) = spawn(Arc::new(NetworkFetcher::new()));
    stop(Client::connect(port).unwrap(), thread);
    let refused = (0..50).any(|_| {
        thread::sleep(Duration::from_millis(20));
        std::net::TcpStream::connect(("127.0.0.1", port)).is_err()
    });
    assert!(refused, "the port still accepts connections");
}

#[test]
fn drag_selection_over_the_protocol() {
    let (mut client, thread) = senko_net();
    let first = client.query_selector("p").unwrap().unwrap();
    let [x, y, ..] = rect(&mut client, first);
    let y = y + 10.0;
    client
        .call("input.mouseDown", json!({ "x": x + 1.0, "y": y }))
        .unwrap();
    client
        .call("input.mouseMove", json!({ "x": x + 300.0, "y": y }))
        .unwrap();
    client
        .call("input.mouseUp", json!({ "x": x + 300.0, "y": y }))
        .unwrap();
    let selection = client.call("selection.get", Value::Null).unwrap();
    assert_eq!(selection["text"], "Hey there!");
    let changed = client.call("selection.selectAll", Value::Null).unwrap();
    assert_eq!(changed["changed"], true);
    stop(client, thread);
}

#[test]
fn cookies_get_and_clear() {
    let fetcher = Arc::new(NetworkFetcher::new());
    let request = swb_net::Request::get(
        swb_net::Url::parse("https://example.com/").unwrap(),
        swb_net::Destination::Document,
    );
    let headers: swb_net::Headers = [("Set-Cookie", "sid=abc; Secure; HttpOnly; Max-Age=60")]
        .into_iter()
        .collect();
    let jar = fetcher.cookie_jar().unwrap();
    jar.store_response_cookies(&request, &headers);
    let (mut client, thread) = start(fetcher);

    let result = client.call("cookies.get", Value::Null).unwrap();
    let cookies = result["cookies"].as_array().unwrap();
    assert_eq!(cookies.len(), 1);
    let cookie = &cookies[0];
    assert_eq!(cookie["name"], "sid");
    assert_eq!(cookie["value"], "abc");
    assert_eq!(cookie["domain"], "example.com");
    assert_eq!(cookie["hostOnly"], true);
    assert_eq!(cookie["path"], "/");
    assert_eq!(cookie["secure"], true);
    assert_eq!(cookie["httpOnly"], true);
    assert_eq!(cookie["sameSite"], "Default");
    assert!(cookie["expires"].is_i64());

    assert_eq!(
        client.call("cookies.clear", Value::Null).unwrap(),
        json!({})
    );
    assert_eq!(
        client.call("cookies.get", Value::Null).unwrap(),
        json!({ "cookies": [] })
    );
    stop(client, thread);
}

#[test]
fn typing_into_a_form_and_submitting_it() {
    let dir = std::env::temp_dir().join(format!("swb-automation-forms-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("form.html"),
        "<!DOCTYPE html><form action=result.html><input id=q name=q>\
         <input id=c type=checkbox name=c><textarea id=t></textarea></form><input id=r>",
    )
    .unwrap();
    std::fs::write(dir.join("result.html"), "<title>result</title>").unwrap();
    let url = swb_net::Url::from_file_path(dir.join("form.html")).unwrap();
    let (mut client, thread) = start(Arc::new(NetworkFetcher::new()));
    client.navigate(url.as_str()).unwrap();
    assert!(client.wait_for_load(TIMEOUT).unwrap());
    // Nothing editable has the focus yet.
    assert_eq!(
        rpc_code(client.call("input.type", json!({ "text": "x" }))),
        RpcError::FAILED
    );
    let q = client.query_selector("#q").unwrap().unwrap();
    let c = client.query_selector("#c").unwrap().unwrap();
    let t = client.query_selector("#t").unwrap().unwrap();
    client.click_node(t).unwrap();
    client.type_text("one\ntwo").unwrap();
    assert_eq!(client.value(t).unwrap().as_deref(), Some("one\ntwo"));
    // A tab presses Tab: typing goes on in the next field.
    client.type_text("\tthree").unwrap();
    assert_eq!(client.value(t).unwrap().as_deref(), Some("one\ntwo"));
    let r = client.query_selector("#r").unwrap().unwrap();
    assert_eq!(client.value(r).unwrap().as_deref(), Some("three"));
    client.click_node(c).unwrap();
    let checkbox = client.call("dom.value", json!({ "nodeId": c })).unwrap();
    assert_eq!(checkbox["checked"], true);
    assert_eq!(checkbox["value"], "on");
    let form = client.query_selector("form").unwrap().unwrap();
    assert_eq!(client.value(form).unwrap(), None);
    client.click_node(q).unwrap();
    client.type_text("hello world").unwrap();
    assert_eq!(client.value(q).unwrap().as_deref(), Some("hello world"));
    // A line break presses Enter: the form is submitted.
    client.type_text("\n").unwrap();
    assert!(client.wait_for_load(TIMEOUT).unwrap());
    let info = client.info().unwrap();
    assert_eq!(info["title"], "result");
    let url = info["url"].as_str().unwrap();
    assert!(url.ends_with("result.html?q=hello+world&c=on"), "{url}");
    stop(client, thread);
    let _ = std::fs::remove_dir_all(&dir);
}
