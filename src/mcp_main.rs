//! oclnr-mcp binary — stdio JSON-RPC MCP server for osx-clnr.

use osx_clnr::mcp::{
    protocol::{JsonRpcMessage, JsonRpcReader, JsonRpcResponse, JsonRpcWriter},
    server::OsxClnrMcpServer,
    ErrorCode, ErrorResponse,
};
use serde_json::Value;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let workspace = std::env::var("OCLNR_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")));

    let mut server = OsxClnrMcpServer::new(workspace)
        .map_err(|e| anyhow::anyhow!("Failed to start MCP server: {}", e.message))?;

    let mut reader = JsonRpcReader::new();
    let mut writer = JsonRpcWriter::new();

    loop {
        let msg = match reader.read_message() {
            Ok(Some(m)) => m,
            Ok(None) => break,
            Err(e) => {
                eprintln!("Read error: {e}");
                break;
            }
        };

        if let Some(resp) = handle_message(&mut server, msg) {
            if let Err(e) = writer.write_message(&resp) {
                eprintln!("Write error: {e}");
                break;
            }
        }
    }

    Ok(())
}

fn handle_message(server: &mut OsxClnrMcpServer, msg: JsonRpcMessage) -> Option<JsonRpcMessage> {
    let JsonRpcMessage::Request(req) = msg else {
        return None; // notifications and responses need no reply
    };

    let id = req.id.clone();

    let result: Result<Value, ErrorResponse> = match req.method.as_str() {
        "initialize" => server.initialize(req.params.unwrap_or(Value::Null)),
        "tools/list" => server.list_tools(),
        "tools/call" => {
            let params = req.params.unwrap_or(Value::Null);
            let name = params["name"].as_str().unwrap_or("").to_string();
            let tool_params = params.get("arguments").cloned();
            server.call_tool(&name, tool_params)
        }
        other => Err(ErrorResponse::new(
            ErrorCode::MethodNotFound,
            format!("Unknown method: {other}"),
        )),
    };

    let resp = match result {
        Ok(v) => JsonRpcResponse::success(id, v),
        Err(e) => JsonRpcResponse::error(
            id,
            ErrorCode::InternalError as i32,
            e.message,
            Some(serde_json::to_value(&e.code).unwrap_or(Value::Null)),
        ),
    };

    Some(JsonRpcMessage::Response(resp))
}
