//! JSON-RPC 2.0 framing: one JSON object per line in both directions.
//! Requests carry `id`; notifications from the client omit it; the daemon
//! sends notifications for subscriptions (§5 "server-initiated
//! notifications for subscriptions").

use serde_json::{json, Value as Json};

/// Standard JSON-RPC error: the line was not JSON.
pub const PARSE_ERROR: i64 = -32700;
/// Standard JSON-RPC error: not a request object.
pub const INVALID_REQUEST: i64 = -32600;
/// Standard JSON-RPC error: no such method.
pub const METHOD_NOT_FOUND: i64 = -32601;

/// A parsed request.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    /// The id, or `None` for a client notification.
    pub id: Option<Json>,
    /// Method name (`world.query`).
    pub method: String,
    /// Params object (an empty object when omitted).
    pub params: Json,
}

/// Parse one line. On failure the error is a ready-to-send response.
pub fn parse_request(line: &str) -> std::result::Result<Request, Json> {
    let v: Json = serde_json::from_str(line).map_err(|e| {
        err_response(
            Json::Null,
            PARSE_ERROR,
            &format!("parse error: {e}"),
            Json::Null,
        )
    })?;
    let obj = v.as_object().ok_or_else(|| {
        err_response(
            Json::Null,
            INVALID_REQUEST,
            "request must be an object",
            Json::Null,
        )
    })?;
    let id = obj.get("id").cloned();
    let method = obj
        .get("method")
        .and_then(Json::as_str)
        .ok_or_else(|| {
            err_response(
                id.clone().unwrap_or(Json::Null),
                INVALID_REQUEST,
                "request needs a method",
                Json::Null,
            )
        })?
        .to_string();
    let params = match obj.get("params") {
        None | Some(Json::Null) => json!({}),
        Some(p @ Json::Object(_)) => p.clone(),
        Some(_) => {
            return Err(err_response(
                id.unwrap_or(Json::Null),
                INVALID_REQUEST,
                "params must be an object",
                Json::Null,
            ))
        }
    };
    Ok(Request { id, method, params })
}

/// A success response.
pub fn ok_response(id: Json, result: Json) -> Json {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// An error response with a JSON-RPC or engine code.
pub fn err_response(id: Json, code: i64, message: &str, data: Json) -> Json {
    let mut e = json!({ "code": code, "message": message });
    if !data.is_null() {
        e["data"] = data;
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": e })
}

/// An error response from an engine error object (`json::error_json`).
pub fn err_response_from(id: Json, error: Json) -> Json {
    json!({ "jsonrpc": "2.0", "id": id, "error": error })
}

/// A server-initiated notification.
pub fn notification(method: &str, params: Json) -> Json {
    json!({ "jsonrpc": "2.0", "method": method, "params": params })
}
