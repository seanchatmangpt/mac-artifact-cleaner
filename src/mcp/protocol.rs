//! JSON-RPC MCP Protocol Implementation
//!
//! Implements the Model Context Protocol (MCP) over JSON-RPC 2.0.
//! Handles message parsing, serialization, and routing.

use std::io::{self, BufRead, Write};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 Request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    pub fn new(id: Value, method: String, params: Option<Value>) -> Self {
        Self { jsonrpc: "2.0".to_string(), id, method, params }
    }
}

/// JSON-RPC 2.0 Response (success)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn success(id: Value, result: Value) -> Self {
        Self { jsonrpc: "2.0".to_string(), id, result: Some(result), error: None }
    }

    pub fn error(id: Value, code: i32, message: String, data: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError { code, message, data }),
        }
    }
}

/// JSON-RPC 2.0 Error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// JSON-RPC 2.0 Notification (no `id` field)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// MCP Protocol message (union of request/response/notification)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    // Request has `id` — must precede Notification (which has no `id`) so the
    // untagged deserializer doesn't eat requests as notifications.
    Request(JsonRpcRequest),
    Response(JsonRpcResponse),
    Notification(JsonRpcNotification),
}

/// MCP Initialization request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeRequest {
    pub protocol_version: String,
    pub capabilities: InitializeCapabilities,
    pub client_info: ClientInfo,
}

/// MCP Server capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental: Option<Value>,
}

/// Client info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

/// MCP Initialization response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResponse {
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    pub server_info: ServerInfo,
}

/// Server capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerCapabilities {
    pub tools: ToolsCapability,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental: Option<Value>,
}

/// Tools capability
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsCapability {
    pub list_changed: bool,
}

/// Server info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

/// Tool definition for MCP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: ToolInputSchema,
}

/// Tool input schema (JSON Schema)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInputSchema {
    pub r#type: String,
    pub properties: Value,
    #[serde(default)]
    pub required: Vec<String>,
}

/// Line-based JSON-RPC message reader
pub struct JsonRpcReader {
    reader: io::BufReader<io::Stdin>,
}

impl JsonRpcReader {
    pub fn new() -> Self {
        Self { reader: io::BufReader::new(io::stdin()) }
    }

    /// Read next JSON-RPC message from stdin (line-delimited)
    pub fn read_message(&mut self) -> io::Result<Option<JsonRpcMessage>> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => Ok(None), // EOF
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    return self.read_message(); // Skip empty lines
                }
                serde_json::from_str::<JsonRpcMessage>(trimmed)
                    .map(Some)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
            }
            Err(e) => Err(e),
        }
    }
}

/// Line-based JSON-RPC message writer
pub struct JsonRpcWriter {
    writer: std::io::Stdout,
}

impl JsonRpcWriter {
    pub fn new() -> Self {
        Self { writer: io::stdout() }
    }

    /// Write JSON-RPC message to stdout (line-delimited)
    pub fn write_message(&mut self, msg: &JsonRpcMessage) -> io::Result<()> {
        let json = serde_json::to_string(msg)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        writeln!(self.writer, "{}", json)?;
        self.writer.flush()?;
        Ok(())
    }

    /// Write JSON-RPC response to stdout
    pub fn write_response(&mut self, response: JsonRpcResponse) -> io::Result<()> {
        self.write_message(&JsonRpcMessage::Response(response))
    }

    /// Write JSON-RPC error to stdout
    pub fn write_error(
        &mut self,
        id: Value,
        code: i32,
        message: String,
        data: Option<Value>,
    ) -> io::Result<()> {
        let response = JsonRpcResponse::error(id, code, message, data);
        self.write_response(response)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn test_jsonrpc_request_serialization() {
        let req = JsonRpcRequest::new(Value::Number(1.into()), "list_tools".to_string(), None);
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"method\":\"list_tools\""));
    }

    #[test]
    fn test_jsonrpc_response_serialization() {
        let resp = JsonRpcResponse::success(Value::Number(1.into()), json!({"tools": []}));
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"result\""));
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn test_jsonrpc_error_serialization() {
        let resp = JsonRpcResponse::error(
            Value::Number(1.into()),
            -32600,
            "Invalid Request".to_string(),
            None,
        );
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"error\""));
        assert!(json.contains("-32600"));
    }
}
