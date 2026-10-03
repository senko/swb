//! A blocking client for the automation protocol.

use std::net::TcpStream;
use std::time::Duration;

use serde_json::{Value, json};
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Message, WebSocket};

use crate::protocol::{Request, Response, RpcError};

/// An error of a client call.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// The connection failed.
    #[error("connection error: {0}")]
    Io(#[from] std::io::Error),
    /// The WebSocket failed.
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tungstenite::Error),
    /// A message was not valid JSON.
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// The server answered with an error.
    #[error("{0}")]
    Rpc(#[from] RpcError),
    /// The server answered something unexpected.
    #[error("protocol error: {0}")]
    Protocol(String),
}

/// A connection to an swb automation server.
pub struct Client {
    socket: WebSocket<TcpStream>,
    next_id: u64,
}

impl Client {
    /// Connects to the server on 127.0.0.1:`port`.
    pub fn connect(port: u16) -> Result<Client, ClientError> {
        let stream = TcpStream::connect(("127.0.0.1", port))?;
        let url = format!("ws://127.0.0.1:{port}/");
        // Responses can be large: a full-page screenshot is up to 128 Mpx.
        let config = WebSocketConfig::default()
            .max_message_size(None)
            .max_frame_size(None);
        let (socket, _) = tungstenite::client::client_with_config(url, stream, Some(config))
            .map_err(|e| match e {
                tungstenite::HandshakeError::Failure(e) => ClientError::WebSocket(e),
                tungstenite::HandshakeError::Interrupted(_) => {
                    ClientError::Protocol("the handshake was interrupted".to_owned())
                }
            })?;
        Ok(Client { socket, next_id: 1 })
    }

    /// Calls a method and returns its result.
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, ClientError> {
        let id = self.next_id;
        self.next_id += 1;
        let request = Request {
            id: Value::from(id),
            method: method.to_owned(),
            params,
        };
        self.socket
            .send(Message::text(serde_json::to_string(&request)?))?;
        loop {
            let text = match self.socket.read()? {
                Message::Text(text) => text,
                Message::Close(_) => {
                    return Err(ClientError::Protocol(
                        "the server closed the connection".into(),
                    ));
                }
                _ => continue,
            };
            let response: Response = serde_json::from_str(&text)?;
            if response.id != id {
                return Err(ClientError::Protocol(format!(
                    "expected the response to request {id}, got {}",
                    response.id
                )));
            }
            if let Some(error) = response.error {
                return Err(error.into());
            }
            return Ok(response.result.unwrap_or(Value::Null));
        }
    }

    /// Starts loading `url`.
    pub fn navigate(&mut self, url: &str) -> Result<(), ClientError> {
        self.call("page.navigate", json!({ "url": url }))?;
        Ok(())
    }

    /// Waits until the page is loaded. Returns false on timeout.
    pub fn wait_for_load(&mut self, timeout: Duration) -> Result<bool, ClientError> {
        let ms = u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX);
        let result = self.call("page.waitForLoad", json!({ "timeoutMs": ms }))?;
        Ok(result["loaded"].as_bool().unwrap_or(false))
    }

    /// The node IDs of the elements that match a selector.
    pub fn query_selector_all(&mut self, selector: &str) -> Result<Vec<u64>, ClientError> {
        let result = self.call("dom.querySelectorAll", json!({ "selector": selector }))?;
        let ids = result["nodeIds"]
            .as_array()
            .ok_or_else(|| ClientError::Protocol("nodeIds is missing".into()))?;
        Ok(ids.iter().filter_map(Value::as_u64).collect())
    }

    /// The node ID of the first element that matches a selector.
    pub fn query_selector(&mut self, selector: &str) -> Result<Option<u64>, ClientError> {
        let result = self.call("dom.querySelector", json!({ "selector": selector }))?;
        Ok(result["nodeId"].as_u64())
    }

    /// The rendered text of a node.
    pub fn text(&mut self, node: u64) -> Result<String, ClientError> {
        let result = self.call("dom.text", json!({ "nodeId": node }))?;
        Ok(result["text"].as_str().unwrap_or("").to_owned())
    }

    /// Clicks at a point in viewport coordinates.
    pub fn click(&mut self, x: f32, y: f32) -> Result<(), ClientError> {
        self.call("input.click", json!({ "x": x, "y": y }))?;
        Ok(())
    }

    /// Clicks the center of a node's box.
    pub fn click_node(&mut self, node: u64) -> Result<(), ClientError> {
        self.call("input.click", json!({ "nodeId": node }))?;
        Ok(())
    }

    /// Moves the mouse pointer to a point in viewport coordinates.
    pub fn mouse_move(&mut self, x: f32, y: f32) -> Result<(), ClientError> {
        self.call("input.mouseMove", json!({ "x": x, "y": y }))?;
        Ok(())
    }

    /// Presses a key (a DOM key value) with modifiers (`Shift`,
    /// `Control`, `Alt`, `Meta`). Returns true if the page handled it.
    pub fn key(&mut self, key: &str, modifiers: &[&str]) -> Result<bool, ClientError> {
        let result = self.call("input.key", json!({ "key": key, "modifiers": modifiers }))?;
        Ok(result["handled"].as_bool().unwrap_or(false))
    }

    /// Types text into the focused text field or text area
    /// (`input.type`).
    pub fn type_text(&mut self, text: &str) -> Result<(), ClientError> {
        self.call("input.type", json!({ "text": text }))?;
        Ok(())
    }

    /// The value of a form control, or `None` for other nodes
    /// (`dom.value`).
    pub fn value(&mut self, node: u64) -> Result<Option<String>, ClientError> {
        let result = self.call("dom.value", json!({ "nodeId": node }))?;
        Ok(result["value"].as_str().map(str::to_owned))
    }

    /// The page state (`page.info`).
    pub fn info(&mut self) -> Result<Value, ClientError> {
        self.call("page.info", Value::Null)
    }

    /// A PNG screenshot of the viewport (or of the whole page).
    pub fn screenshot(&mut self, full_page: bool) -> Result<Vec<u8>, ClientError> {
        let result = self.call("page.screenshot", json!({ "fullPage": full_page }))?;
        let png = result["png"]
            .as_str()
            .ok_or_else(|| ClientError::Protocol("png is missing".into()))?;
        data_encoding::BASE64
            .decode(png.as_bytes())
            .map_err(|e| ClientError::Protocol(format!("invalid base64: {e}")))
    }

    /// Asks the browser to close.
    pub fn close_browser(mut self) -> Result<(), ClientError> {
        self.call("browser.close", Value::Null)?;
        Ok(())
    }
}
