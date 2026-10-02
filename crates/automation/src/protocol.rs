//! Protocol messages: requests, responses and errors (JSON-RPC 2.0 style,
//! without the `jsonrpc` member). See `docs/automation.md`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A request from a client.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Request {
    /// The request ID, a number or a string; the response repeats it.
    pub id: Value,
    /// The method name, `domain.verb`.
    pub method: String,
    /// The parameters, an object (or null).
    #[serde(default)]
    pub params: Value,
}

/// A response to a request: a result or an error.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Response {
    /// The ID of the request (null if the request could not be parsed).
    pub id: Value,
    /// The result, if the request succeeded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// The error, if the request failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Response {
    /// A response for `id` from a method's outcome.
    pub fn new(id: Value, outcome: Result<Value, RpcError>) -> Self {
        match outcome {
            Ok(result) => Response {
                id,
                result: Some(result),
                error: None,
            },
            Err(error) => Response {
                id,
                result: None,
                error: Some(error),
            },
        }
    }
}

/// An error of a request.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, thiserror::Error)]
#[error("{message} (error {code})")]
pub struct RpcError {
    /// The error code (the JSON-RPC codes, and [`RpcError::FAILED`]).
    pub code: i64,
    /// A description.
    pub message: String,
}

impl RpcError {
    /// The message is not valid JSON.
    pub const PARSE_ERROR: i64 = -32700;
    /// The message is not a valid request.
    pub const INVALID_REQUEST: i64 = -32600;
    /// The method does not exist.
    pub const METHOD_NOT_FOUND: i64 = -32601;
    /// The parameters are not valid.
    pub const INVALID_PARAMS: i64 = -32602;
    /// The request is valid but failed (for example, no document is
    /// loaded, or the node does not exist).
    pub const FAILED: i64 = -32000;

    /// An error with a code and a message.
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        RpcError {
            code,
            message: message.into(),
        }
    }

    /// Invalid parameters.
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(Self::INVALID_PARAMS, message)
    }

    /// A request that failed.
    pub fn failed(message: impl Into<String>) -> Self {
        Self::new(Self::FAILED, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_format() {
        let request: Request =
            serde_json::from_str(r#"{"id": 1, "method": "page.info"}"#).expect("valid");
        assert_eq!(request.params, Value::Null);
        let ok = Response::new(Value::from(1), Ok(serde_json::json!({"a": 1})));
        assert_eq!(
            serde_json::to_string(&ok).expect("serializable"),
            r#"{"id":1,"result":{"a":1}}"#
        );
        let err = Response::new(Value::from("x"), Err(RpcError::failed("no")));
        assert_eq!(
            serde_json::to_string(&err).expect("serializable"),
            r#"{"id":"x","error":{"code":-32000,"message":"no"}}"#
        );
    }
}
