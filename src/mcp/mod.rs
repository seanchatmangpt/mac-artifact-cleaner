#![allow(
    clippy::result_large_err,
    clippy::new_without_default,
    clippy::useless_format,
    clippy::ptr_arg,
    clippy::len_zero,
    clippy::unnecessary_lazy_evaluations
)]
//! osx-clnr MCP Server
//!
//! Model Context Protocol (MCP) server exposing osx-clnr capabilities.
//! Implements a stdio-based MCP server that orchestrates disk auditing,
//! plan generation, deletion, and receipt verification.
//!
//! # Features
//!
//! - Spawns oclnr as subprocess
//! - Implements 19 MCP tools for complete cleanup workflow
//! - State machine enforcement (UNSTARTED → AUDIT_COMPLETE → PLAN_READY → etc.)
//! - Plan approval gates with HMAC-SHA256 signature verification
//! - Immutable evidence trails (audit, plan, receipt files)
//! - Comprehensive error handling with recovery suggestions
//! - JSON/JSONOCEL parsing and validation
//!
//! # Architecture
//!
//! ```text
//! Claude/LLM Client
//!        |
//!        v
//!   MCP Protocol (stdio)
//!        |
//!        v
//! MCP Server (this module)
//!        |
//!        v
//! Tool Dispatcher
//!        |
//!        v
//! Subprocess Runner (oclnr audit, plan, delete, receipt)
//!        |
//!        v
//! Filesystem & APFS snapshots
//! ```
//!
//! # Usage
//!
//! Run as MCP server:
//! ```bash
//! cargo run --bin oclnr-mcp
//! ```
//!
//! Or embed in a Claude Code integration.

pub mod error;
pub mod protocol;
pub mod server;
pub mod state;
pub mod subprocess;
pub mod tools;

pub use error::{ErrorCode, ErrorResponse};
pub use protocol::{JsonRpcMessage, JsonRpcRequest, JsonRpcResponse};
pub use server::OsxClnrMcpServer;
pub use state::{WorkflowContext, WorkflowState};
// Re-export key types
pub use tools::{
    ApprovalMetadata, ArtifactKind, AuditScanInput, AuditScanOutput, AuditSummary, Candidate,
    DeletionResult, DeletionStatus, PlanBuildInput, PlanBuildOutput, PlanSummary, ProjectType,
    SafetyChecks,
};

/// MCP Protocol version this server implements
pub const MCP_VERSION: &str = "2025-03-26";

/// osx-clnr MCP server version
pub const SERVER_VERSION: &str = "1.0.0";
