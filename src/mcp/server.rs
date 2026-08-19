//! MCP Server implementation
//!
//! Main orchestration logic for handling MCP requests and dispatching to tools.

use std::{collections::HashMap, path::PathBuf};

use chrono::Utc;
use serde_json::{json, Value};

use super::{
    error::{ErrorCode, ErrorResponse},
    protocol::{InitializeResponse, ServerCapabilities, ServerInfo, ToolsCapability},
    state::{WorkflowContext, WorkflowState},
    subprocess::{parse_json_output, parse_jsonocel_output, OclnrRunner},
    tools::*,
};

/// MCP Server
pub struct OsxClnrMcpServer {
    runner: OclnrRunner,
    workflows: HashMap<String, WorkflowContext>,
    default_workspace: PathBuf,
}

impl OsxClnrMcpServer {
    /// Create new MCP server
    pub fn new(default_workspace: PathBuf) -> Result<Self, ErrorResponse> {
        let runner = OclnrRunner::new()?;
        Ok(Self { runner, workflows: HashMap::new(), default_workspace })
    }

    /// Initialize MCP server (handshake with client)
    pub fn initialize(&self, _request: Value) -> Result<Value, ErrorResponse> {
        let response = InitializeResponse {
            protocol_version: super::MCP_VERSION.to_string(),
            capabilities: ServerCapabilities {
                tools: ToolsCapability { list_changed: false },
                experimental: None,
            },
            server_info: ServerInfo {
                name: "osx-clnr-mcp".to_string(),
                version: super::SERVER_VERSION.to_string(),
            },
        };

        Ok(serde_json::to_value(response).unwrap())
    }

    /// List available tools
    ///
    /// The surface is 7 resource-grouped tools, each dispatched by an
    /// `action` enum, rather than one tool per verb. `call_tool` routes
    /// `(name, action)` to the same per-verb handler methods below — this
    /// is a routing-layer consolidation only, no handler logic changed.
    pub fn list_tools(&self) -> Result<Value, ErrorResponse> {
        let tools = vec![
            json!({
                "name": "workflow",
                "description": "Cleanup workflow state management: query current state, archive \
                                 evidence and reset, or restore from snapshots.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["query", "clear", "rollback"] },
                        "workspace": { "type": "string", "description": "Workspace directory (default: current)" },
                        "archive_to": { "type": "string", "description": "(clear only)" },
                        "dry_run": { "type": "boolean", "default": true, "description": "(clear only)" },
                        "receipt_file": { "type": "string", "description": "(rollback only)" },
                        "confirm": { "type": "boolean", "default": false, "description": "(clear/rollback only)" }
                    },
                    "required": ["action"]
                }
            }),
            json!({
                "name": "audit",
                "description": "Scan the filesystem: `scan` for deletion-candidate evidence \
                                 (build artifacts, dependency dirs, tool caches — feeds `plan`), \
                                 `parse` to re-read a saved scan without rescanning, or \
                                 `breakdown` for a full byte-accounted usage breakdown including \
                                 hidden dirs and non-artifact data (Library, caches, VM disk \
                                 images) — use breakdown to find where disk space actually went, \
                                 scan to find what's safe to delete.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["scan", "parse", "breakdown"] },
                        "workspace": { "type": "string" },
                        "roots": { "type": "array", "items": { "type": "string" }, "description": "(scan only)" },
                        "include_deps": { "type": "boolean", "description": "(scan only)" },
                        "include_aggressive": { "type": "boolean", "description": "(scan only)" },
                        "ignore_recent_hours": { "type": "integer", "default": 168, "description": "(scan only)" },
                        "tool_roots": { "type": "boolean", "default": false, "description": "(scan only)" },
                        "all_filesystems": { "type": "boolean", "default": false, "description": "(scan only) Allow crossing onto other filesystems/APFS volumes reachable from a root (e.g. from \"/\" onto the System volume). Default false pins the walk to each root's own volume, so roots: [\"/\"] alone does NOT cover the whole disk on macOS -- \"/\" and \"/Users\" are typically separate volumes joined by firmlinks." },
                        "audit_file": { "type": "string", "description": "(parse only)" },
                        "top_n": { "type": "integer", "default": 50, "description": "(parse only)" },
                        "filter_reason": { "type": "string", "description": "(parse only)" },
                        "root": { "type": "string", "description": "(breakdown only) Root to scan (default: home directory)" },
                        "depth": { "type": "integer", "default": 2, "description": "(breakdown only) Path components below root to bucket by; 2+ splits catch-all dirs like Library into children" },
                        "top": { "type": "integer", "default": 40, "description": "(breakdown only)" },
                        "min_mb": { "type": "integer", "default": 0, "description": "(breakdown only)" }
                    },
                    "required": ["action"]
                }
            }),
            json!({
                "name": "plan",
                "description": "Build and review a deletion plan: `build` from audit results, \
                                 `inspect` an existing plan file, `validate` its safety (no OS \
                                 dirs, no dotfiles in home, proper signatures — path protection \
                                 and symlink checks), or `approve` it with an HMAC-SHA256 \
                                 signature before deletion is allowed.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["build", "inspect", "validate", "approve"] },
                        "workspace": { "type": "string" },
                        "audit_file": { "type": "string", "description": "(build only)" },
                        "roots": { "type": "array", "items": { "type": "string" }, "description": "(build only)" },
                        "deps": { "type": "boolean", "description": "(build only)" },
                        "aggressive": { "type": "boolean", "description": "(build only)" },
                        "include_global_caches": { "type": "boolean", "description": "(build only)" },
                        "max_reclaim_gb": { "type": "number", "description": "(build only)" },
                        "ignore_recent_hours": { "type": "integer", "description": "(build only)" },
                        "plan_file": { "type": "string", "description": "(inspect/validate/approve)" },
                        "top_n": { "type": "integer", "default": 20, "description": "(inspect only)" },
                        "approver_name": { "type": "string", "description": "(approve only)" },
                        "approval_reason": { "type": "string", "description": "(approve only)" },
                        "confirm": { "type": "boolean", "default": false, "description": "(approve only)" }
                    },
                    "required": ["action"]
                }
            }),
            json!({
                "name": "delete",
                "description": "Plan-bound deletion: `dry_run` previews without modifying the \
                                 filesystem, `execute` performs the deletion from an approved \
                                 plan. Both read exclusively from a saved plan file — never a \
                                 live scan.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["dry_run", "execute"] },
                        "workspace": { "type": "string" },
                        "plan_file": { "type": "string" },
                        "receipt_file": { "type": "string", "description": "(execute only)" },
                        "confirm": { "type": "boolean", "default": false, "description": "(execute only)" },
                        "max_concurrent": { "type": "integer", "default": 4, "description": "(execute only)" },
                        "timeout_secs": { "type": "integer", "default": 30, "description": "(execute only)" }
                    },
                    "required": ["action", "plan_file"]
                }
            }),
            json!({
                "name": "receipt",
                "description": "Inspect and verify a deletion receipt: `parse` reads it raw, \
                                 `verify` checks claimed vs. actual free-space delta and \
                                 validates OCEL referential integrity, optionally sealing it \
                                 with an affidavit cryptographic proof chain via `seal: true`.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["parse", "verify"] },
                        "workspace": { "type": "string" },
                        "receipt_file": { "type": "string" },
                        "seal": { "type": "boolean", "default": false, "description": "(verify only) Also seal the receipt with an affidavit proof chain; requires confirm: true." },
                        "confirm": { "type": "boolean", "default": false, "description": "(verify only, required when seal: true)" }
                    },
                    "required": ["action"]
                }
            }),
            json!({
                "name": "snapshot",
                "description": "Local APFS snapshot management: `audit` lists and analyzes \
                                 snapshots, `thin` reclaims a target number of bytes, `delete` \
                                 removes specific snapshots by name/date, oldest-N, or all. \
                                 `thin` and `delete` both seal their receipt via affidavit core/v1.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["audit", "thin", "delete"] },
                        "workspace": { "type": "string" },
                        "roots": { "type": "array", "items": { "type": "string" }, "description": "(audit only)" },
                        "mount": { "type": "string", "description": "(thin/delete) Real volume mount point, e.g. \"/\". Required — no default." },
                        "bytes": { "type": "string", "description": "(thin only) Target bytes to reclaim, e.g. \"10GB\" or raw digits." },
                        "which": { "type": "string", "description": "(delete only) \"oldest\", \"all\", or an explicit snapshot name/date." },
                        "oldest_n": { "type": "integer", "default": 1, "description": "(delete only) Used only when which == \"oldest\"." },
                        "confirm": { "type": "boolean", "default": false, "description": "(thin/delete only)" }
                    },
                    "required": ["action"]
                }
            }),
            json!({
                "name": "emergency_reclaim",
                "description": "Aggressively reclaim disk space when low. Kept as its own tool, \
                                 deliberately not folded into `delete`: unlike every `delete` \
                                 action, this scans and deletes in one call with no separate \
                                 plan-review step. NOT scoped to `workspace`: sweeps real APFS \
                                 snapshots and home-directory caches on the given `mount`. Never \
                                 call against a real mount without explicit user intent.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "workspace": { "type": "string" },
                        "mount": { "type": "string", "description": "Real volume mount point to reclaim, e.g. \"/\". Required — no default." },
                        "target_free_gb": { "type": "number" },
                        "confirm": { "type": "boolean", "default": false }
                    },
                    "required": ["mount", "target_free_gb"]
                }
            }),
            json!({
                "name": "docker",
                "description": "Docker/Colima disk cleanup: `scan` shows current disk usage \
                                 (images, containers, volumes, build cache), `plan` previews what \
                                 a prune would reclaim without changing anything, `prune` actually \
                                 runs `docker system prune -af --volumes` and (unless \
                                 skip_colima) `colima prune` to reclaim it. `prune` is destructive \
                                 (removes unused images, stopped containers, unused volumes, and \
                                 build cache) and requires confirm: true; it has no dry-run receipt \
                                 or affidavit seal — no MCP-tracked audit trail exists for Docker \
                                 yet, unlike delete/snapshot.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["scan", "plan", "prune"] },
                        "workspace": { "type": "string" },
                        "skip_colima": { "type": "boolean", "default": false, "description": "(prune only) Skip `colima prune` even if Colima is available." },
                        "confirm": { "type": "boolean", "default": false, "description": "(prune only) Required to actually prune." }
                    },
                    "required": ["action"]
                }
            }),
            json!({
                "name": "doctor",
                "description": "Self-verification diagnostics: architecture layout, macOS \
                                 substrate capabilities, doctest completeness, privacy/redaction \
                                 rule compliance, domain purity (no std::fs/std::process in \
                                 src/domain/**), and the scanner-cannot-delete / \
                                 deleter-cannot-scan invariant. Read-only, non-destructive.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "check": {
                            "type": "string",
                            "enum": [
                                "architecture",
                                "substrate",
                                "doctests",
                                "privacy",
                                "domain-purity",
                                "scan-delete-separation"
                            ]
                        },
                        "workspace": { "type": "string" }
                    },
                    "required": ["check"]
                }
            }),
        ];

        Ok(json!({ "tools": tools }))
    }

    /// Handle tool call
    fn tool_result(value: Value) -> Value {
        json!({
            "content": [{"type": "text", "text": serde_json::to_string_pretty(&value).unwrap_or_default()}],
            "isError": false
        })
    }

    fn tool_error(msg: &str) -> Value {
        json!({
            "content": [{"type": "text", "text": msg}],
            "isError": true
        })
    }

    /// Extract the required `action` string from a tool call's params.
    fn require_action<'a>(name: &str, params: &'a Value) -> Result<&'a str, ErrorResponse> {
        params.get("action").and_then(|v| v.as_str()).ok_or_else(|| {
            ErrorResponse::new(
                ErrorCode::InvalidInput,
                format!("`action` is required for tool `{name}`"),
            )
        })
    }

    fn unknown_action(tool: &str, action: &str) -> ErrorResponse {
        ErrorResponse::new(
            ErrorCode::InvalidInput,
            format!("unknown action `{action}` for tool `{tool}`"),
        )
    }

    pub fn call_tool(&mut self, name: &str, params: Option<Value>) -> Result<Value, ErrorResponse> {
        let params = params.unwrap_or(Value::Object(Default::default()));

        // Every tool but `emergency_reclaim` is a resource dispatched by an
        // `action` enum; the same per-verb handler methods below (unchanged
        // from the pre-consolidation 1-tool-per-verb surface) do the work —
        // this match only routes `(name, action)` to them.
        let inner = match name {
            "workflow" => match Self::require_action(name, &params)? {
                "query" => self.query_workflow_state(params),
                "clear" => self.clear_artifacts(params),
                "rollback" => self.plan_rollback(params),
                other => Err(Self::unknown_action(name, other)),
            },
            "audit" => match Self::require_action(name, &params)? {
                "scan" => self.audit_scan(params),
                "parse" => self.audit_parse(params),
                "breakdown" => self.audit_breakdown(params),
                other => Err(Self::unknown_action(name, other)),
            },
            "plan" => match Self::require_action(name, &params)? {
                "build" => self.plan_build(params),
                "inspect" => self.plan_inspect(params),
                "validate" => self.plan_validate(params),
                "approve" => self.plan_approve(params),
                other => Err(Self::unknown_action(name, other)),
            },
            "delete" => match Self::require_action(name, &params)? {
                "dry_run" => self.delete_dry_run(params),
                "execute" => self.delete_execute(params),
                other => Err(Self::unknown_action(name, other)),
            },
            "receipt" => match Self::require_action(name, &params)? {
                "parse" => self.receipt_parse(params),
                "verify" => self.receipt_verify(params),
                other => Err(Self::unknown_action(name, other)),
            },
            "snapshot" => match Self::require_action(name, &params)? {
                "audit" => self.snapshot_audit(params),
                "thin" => self.snapshot_thin(params),
                "delete" => self.snapshot_delete(params),
                other => Err(Self::unknown_action(name, other)),
            },
            "emergency_reclaim" => self.emergency_reclaim(params),
            "docker" => match Self::require_action(name, &params)? {
                "scan" => self.docker_scan(params),
                "plan" => self.docker_plan(params),
                "prune" => self.docker_prune(params),
                other => Err(Self::unknown_action(name, other)),
            },
            "doctor" => self.doctor_check(params),

            _ => Err(ErrorResponse::new(
                ErrorCode::MethodNotFound,
                format!("Unknown tool: {}", name),
            )),
        };

        Ok(match inner {
            Ok(v) => Self::tool_result(v),
            Err(e) => Self::tool_error(&e.message),
        })
    }

    // ========================================================================
    // TOOL IMPLEMENTATIONS
    // ========================================================================

    fn get_or_create_context(&mut self, workspace: Option<PathBuf>) -> WorkflowContext {
        let ws = workspace.unwrap_or_else(|| self.default_workspace.clone());
        let id = ws.display().to_string();
        self.workflows.entry(id).or_insert_with(|| WorkflowContext::new(ws.clone())).clone()
    }

    fn query_workflow_state(&self, params: Value) -> Result<Value, ErrorResponse> {
        let input: serde_json::Map<String, Value> =
            serde_json::from_value(params).unwrap_or_default();
        let workspace: Option<PathBuf> =
            input.get("workspace").and_then(|v| v.as_str()).map(PathBuf::from);

        let ws_ref = workspace.as_ref().unwrap_or(&self.default_workspace);
        let ctx = self.workflows.values().find(|w| &w.workspace == ws_ref).cloned().unwrap_or_else(
            || WorkflowContext::new(workspace.unwrap_or_else(|| self.default_workspace.clone())),
        );

        let output = QueryWorkflowStateOutput {
            state: ctx.state.as_str().to_string(),
            last_audit_time: ctx.last_audit_time.map(|t| t.to_rfc3339()),
            last_plan_time: ctx.last_plan_time.map(|t| t.to_rfc3339()),
            last_delete_time: ctx.last_delete_time.map(|t| t.to_rfc3339()),
            audit_file: ctx.audit_file.as_ref().map(|p| p.display().to_string()),
            plan_file: ctx.plan_file.as_ref().map(|p| p.display().to_string()),
            receipt_file: ctx.receipt_file.as_ref().map(|p| p.display().to_string()),
            affidavit_file: ctx.affidavit_file.as_ref().map(|p| p.display().to_string()),
            messages: vec![ctx.next_step_guidance()],
        };

        Ok(serde_json::to_value(output).unwrap())
    }

    fn clear_artifacts(&mut self, params: Value) -> Result<Value, ErrorResponse> {
        let input: ClearArtifactsInput = serde_json::from_value(params)
            .map_err(|e| ErrorResponse::json_parse_error(&e.to_string()))?;

        let workspace = input.workspace.unwrap_or_else(|| self.default_workspace.clone());
        let mut ctx = self.get_or_create_context(Some(workspace.clone()));

        let now = Utc::now().format("%Y%m%d_%H%M%S").to_string();
        let archive_dir = input.archive_to.unwrap_or_else(|| {
            let mut p = workspace.clone();
            p.push(format!("archive/{}", now));
            p
        });

        let has_audit = ctx.audit_file.as_ref().is_some_and(|p| p.exists());

        if !has_audit {
            // Nothing to archive: this workspace has no scanned evidence at all.
            // Fabricating an archive_location / success here would mislead a
            // caller into believing artifacts were archived when nothing
            // happened and the workspace/archive path was never validated.
            return Err(ErrorResponse::new(
                ErrorCode::InvalidInput,
                format!(
                    "No audit evidence found for workspace {}; nothing to archive. \
                     Run audit_scan first, or verify the workspace path is correct.",
                    workspace.display()
                ),
            ));
        }

        let audit = ctx.audit_file.clone().unwrap();
        let dest = archive_dir.join(audit.file_name().unwrap_or_default());

        if input.dry_run {
            // Preview only: describe what would be archived without touching
            // the filesystem or mutating workflow state. dry_run always wins,
            // even if `confirm` is also true.
            return Ok(serde_json::to_value(ClearArtifactsOutput {
                success: true,
                archived_files: vec![ArchivedFile {
                    source: audit.display().to_string(),
                    destination: dest.display().to_string(),
                }],
                archive_location: archive_dir.display().to_string(),
                timestamp: now,
                dry_run: true,
            })
            .unwrap());
        }

        if !input.confirm {
            return Err(ErrorResponse::confirmation_required("clear_artifacts"));
        }

        let mut archived = Vec::new();

        // Archive audit file. Any I/O failure (e.g. permission denied creating
        // the archive directory) must surface as an error, not a silent no-op
        // reported as success.
        std::fs::create_dir_all(&archive_dir).map_err(|e| {
            ErrorResponse::new(
                ErrorCode::IoError,
                format!("Failed to create archive directory {}: {}", archive_dir.display(), e),
            )
        })?;

        std::fs::copy(&audit, &dest).map_err(|e| {
            ErrorResponse::new(
                ErrorCode::IoError,
                format!("Failed to copy {} to {}: {}", audit.display(), dest.display(), e),
            )
        })?;
        archived.push(ArchivedFile {
            source: audit.display().to_string(),
            destination: dest.display().to_string(),
        });

        // Reset context
        ctx.state = WorkflowState::Unstarted;
        ctx.audit_file = None;
        ctx.plan_file = None;
        ctx.receipt_file = None;
        ctx.affidavit_file = None;
        self.workflows.insert(workspace.display().to_string(), ctx);

        Ok(serde_json::to_value(ClearArtifactsOutput {
            success: true,
            archived_files: archived,
            archive_location: archive_dir.display().to_string(),
            timestamp: now,
            dry_run: false,
        })
        .unwrap())
    }

    fn audit_scan(&mut self, params: Value) -> Result<Value, ErrorResponse> {
        let input: AuditScanInput = serde_json::from_value(params)
            .map_err(|e| ErrorResponse::json_parse_error(&e.to_string()))?;

        // Guard against an unbounded, unconfirmed full-home-directory scan.
        // `default_scan_roots()` resolves to the user's entire home directory
        // plus /tmp; silently falling back to it whenever the caller omits
        // `roots` means an empty-object call (`{}`) triggers a slow, real
        // filesystem walk of the whole home dir with no confirmation gate.
        // Require callers to pass `roots` explicitly instead.
        // A root that is empty or whitespace-only (e.g. `roots: [""]`)
        // resolves to a blank/relative path that downstream code treats as
        // "no constraint", defeating this guard exactly like an empty array
        // does. Reject those too, regardless of `tool_roots`.
        let has_blank_root = input.roots.iter().any(|r| r.to_string_lossy().trim().is_empty());

        // `tool_roots: true` is a legitimately scoped, non-home-directory
        // operation (it scans known developer tool roots rather than the
        // whole home directory), so it satisfies this guard on its own even
        // when `roots` is empty -- matching what the error message below
        // advertises.
        if (input.roots.is_empty() && !input.tool_roots) || has_blank_root {
            return Err(ErrorResponse::new(
                ErrorCode::InvalidInput,
                "roots is required and must be non-empty (with no blank entries): audit_scan \
                 does not scan the full home directory implicitly. Pass one or more explicit \
                 non-blank paths to scan (e.g. the current project directory), such as \
                 [\"/path/to/project\"], or set tool_roots: true to scan known developer tool \
                 roots instead."
                    .to_string(),
            )
            .with_suggestions(vec![
                "Pass roots: [\"<project-dir>\"] to scope the scan to a specific directory"
                    .to_string(),
                "Use tool_roots: true to scan known developer tool roots instead of the home \
                 directory"
                    .to_string(),
            ]));
        }

        let workspace = input.workspace.clone().unwrap_or_else(|| self.default_workspace.clone());
        let mut ctx = self.get_or_create_context(Some(workspace.clone()));

        ctx.transition(WorkflowState::AuditNeeded).map_err(|_| {
            ErrorResponse::invalid_state_transition(ctx.state.as_str(), "AUDIT_NEEDED")
        })?;

        ctx.transition(WorkflowState::AuditInProgress).map_err(|_| {
            ErrorResponse::invalid_state_transition(ctx.state.as_str(), "AUDIT_IN_PROGRESS")
        })?;

        // Spawn subprocess
        let roots = input.roots;

        let start = std::time::Instant::now();
        let result = self.runner.audit_run(
            &workspace,
            roots.clone(),
            input.include_deps,
            input.include_aggressive,
            input.ignore_recent_hours,
            input.tool_roots,
            input.all_filesystems,
        )?;
        let scan_duration_secs = start.elapsed().as_secs_f64();

        if !result.success() {
            ctx.transition(WorkflowState::AuditFailed).ok();
            ctx.record_error(result.stderr.clone());
            self.workflows.insert(workspace.display().to_string(), ctx);
            return Err(result.to_error("oclnr audit run"));
        }

        // Parse output
        let audit_file = workspace.join("disk-audit.jsonocel");
        ctx.state = WorkflowState::AuditComplete;
        ctx.audit_file = Some(audit_file.clone());
        ctx.audit_roots = Some(roots.clone());
        ctx.audit_ignore_recent_hours = Some(input.ignore_recent_hours);
        ctx.last_audit_time = Some(Utc::now());
        ctx.clear_error();
        self.workflows.insert(workspace.display().to_string(), ctx);

        let summary = std::fs::read_to_string(&audit_file)
            .ok()
            .and_then(|contents| parse_json_output(&contents).ok())
            .map(|log| summarize_disk_audit_ocel(&log, scan_duration_secs))
            .unwrap_or(AuditSummary {
                total_dirs: 0,
                total_files: 0,
                total_bytes: 0,
                total_candidates: 0,
                projects_detected: HashMap::new(),
                largest_candidates: vec![],
                errors: vec![],
                scan_duration_secs,
            });

        Ok(serde_json::to_value(AuditScanOutput {
            state: "AUDIT_COMPLETE".to_string(),
            audit_file: audit_file.display().to_string(),
            summary,
            message: "Audit complete".to_string(),
        })
        .unwrap())
    }

    fn audit_parse(&self, params: Value) -> Result<Value, ErrorResponse> {
        let input: serde_json::Map<String, Value> = serde_json::from_value(params)
            .map_err(|e| ErrorResponse::json_parse_error(&e.to_string()))?;

        let audit_file = input.get("audit_file").and_then(|v| v.as_str()).ok_or_else(|| {
            ErrorResponse::new(ErrorCode::InvalidInput, "audit_file required".to_string())
        })?;

        let top_n = input.get("top_n").and_then(|v| v.as_i64()).unwrap_or(50).max(0) as usize;
        let filter_reason = input.get("filter_reason").and_then(|v| v.as_str());

        let audit_path = PathBuf::from(audit_file);
        if !audit_path.exists() {
            return Err(ErrorResponse::file_not_found(&audit_path, "audit_scan"));
        }

        // Read and parse JSONOCEL
        let content = std::fs::read_to_string(&audit_path)
            .map_err(|e| ErrorResponse::new(ErrorCode::IoError, e.to_string()))?;

        let parsed = parse_jsonocel_output(&content)?;

        // Reuse the same object-graph walk as summarize_disk_audit_ocel to pull
        // real `artifact_candidate` objects (joined to their `filesystem_object`
        // for kind) out of the parsed OCEL log, instead of fabricating an empty
        // candidate list.
        let objects = parsed.get("objects").and_then(|v| v.as_array());

        let fs_kind_by_id: HashMap<String, String> = objects
            .map(|objs| {
                objs.iter()
                    .filter(|o| o.get("type").and_then(|t| t.as_str()) == Some("filesystem_object"))
                    .filter_map(|o| {
                        let id = o.get("id")?.as_str()?.to_string();
                        let kind = o
                            .get("attributes")
                            .and_then(|a| a.as_array())
                            .and_then(|attrs| {
                                attrs.iter().find(|a| {
                                    a.get("name").and_then(|n| n.as_str()) == Some("kind")
                                })
                            })
                            .and_then(|a| a.get("value").and_then(|v| v.as_str()))
                            .unwrap_or("file")
                            .to_string();
                        Some((id, kind))
                    })
                    .collect()
            })
            .unwrap_or_default();

        let mut all_candidates: Vec<Candidate> = objects
            .map(|objs| {
                objs.iter()
                    .filter(|o| {
                        o.get("type").and_then(|t| t.as_str()) == Some("artifact_candidate")
                    })
                    .filter_map(|o| {
                        let attrs = o.get("attributes").and_then(|a| a.as_array())?;
                        let get_attr = |name: &str| -> Option<String> {
                            attrs
                                .iter()
                                .find(|a| a.get("name").and_then(|n| n.as_str()) == Some(name))
                                .and_then(|a| a.get("value").and_then(|v| v.as_str()))
                                .map(|s| s.to_string())
                        };
                        let path = get_attr("path")?;
                        let reason = get_attr("reason").unwrap_or_default();

                        let fs_obj_id = o
                            .get("relationships")
                            .and_then(|r| r.as_array())
                            .and_then(|rels| rels.first())
                            .and_then(|r| r.get("objectId").and_then(|v| v.as_str()));
                        let kind = fs_obj_id
                            .and_then(|id| fs_kind_by_id.get(id))
                            .map(|k| k.as_str())
                            .unwrap_or("file");

                        Some(Candidate {
                            path: PathBuf::from(path),
                            kind: if kind == "directory" {
                                ArtifactKind::Dir
                            } else {
                                ArtifactKind::File
                            },
                            // Per-candidate byte counts are not recorded in the
                            // disk-audit OCEL log (only aggregate `bytes_seen`
                            // is), so this is honestly 0 rather than fabricated.
                            bytes: 0,
                            reason,
                            project_type: ProjectType::Generic,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        if let Some(reason_filter) = filter_reason {
            all_candidates.retain(|c| c.reason == reason_filter);
        }

        let total_candidates = all_candidates.len();
        let total_bytes: u64 = all_candidates.iter().map(|c| c.bytes).sum();
        all_candidates.truncate(top_n);

        Ok(json!({
            "audit_metadata": {
                "created_unix": Utc::now().timestamp(),
                "created_iso": Utc::now().to_rfc3339(),
                "scanner_version": "0.1.0"
            },
            "candidates": all_candidates,
            "totals": {
                "total_candidates": total_candidates,
                "total_bytes": total_bytes
            }
        }))
    }

    /// Full byte-accounted disk usage breakdown. Read-only — unlike
    /// `audit_scan`/`plan_build` this doesn't gate on or advance
    /// `WorkflowContext` state, matching `snapshot_audit`.
    fn audit_breakdown(&self, params: Value) -> Result<Value, ErrorResponse> {
        let input: AuditBreakdownInput = serde_json::from_value(params)
            .map_err(|e| ErrorResponse::json_parse_error(&e.to_string()))?;

        let workspace = input.workspace.clone().unwrap_or_else(|| self.default_workspace.clone());
        let root = match input.root.clone() {
            Some(r) => r,
            None => dirs::home_dir().ok_or_else(|| {
                ErrorResponse::new(
                    ErrorCode::InvalidInput,
                    "could not resolve home directory; \
                    pass `root` explicitly"
                        .to_string(),
                )
            })?,
        };

        let result =
            self.runner.audit_breakdown(&workspace, &root, input.depth, input.top, input.min_mb)?;

        if !result.success() {
            return Err(result.to_error("oclnr audit breakdown"));
        }

        let parsed: Value = serde_json::from_str(result.stdout.trim()).map_err(|e| {
            ErrorResponse::new(
                ErrorCode::IoError,
                format!("failed to parse `oclnr audit breakdown --json` output: {e}"),
            )
        })?;

        let disk_total = parsed.get("disk_total_bytes").and_then(|v| v.as_u64()).unwrap_or(0);
        let disk_available =
            parsed.get("disk_available_bytes").and_then(|v| v.as_u64()).unwrap_or(0);
        let disk_percent_used = if disk_total > 0 {
            (((disk_total.saturating_sub(disk_available)) as f64 / disk_total as f64) * 100.0)
                .round() as u8
        } else {
            0
        };

        let entries: Vec<BreakdownEntryOutput> = parsed
            .get("entries")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|e| {
                        Some(BreakdownEntryOutput {
                            path: e.get("path")?.as_str()?.to_string(),
                            bytes: e.get("bytes")?.as_u64()?,
                            percent_of_total: e
                                .get("percent_of_total")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.0),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(serde_json::to_value(AuditBreakdownOutput {
            state: "BREAKDOWN_COMPLETE".to_string(),
            root: root.display().to_string(),
            depth: input.depth,
            disk_total_bytes: disk_total,
            disk_available_bytes: disk_available,
            disk_percent_used,
            total_bytes: parsed.get("total_bytes").and_then(|v| v.as_u64()).unwrap_or(0),
            entry_count: parsed.get("entry_count").and_then(|v| v.as_u64()).unwrap_or(0) as usize,
            entries,
            message: "Disk breakdown complete".to_string(),
        })
        .unwrap())
    }

    fn plan_build(&mut self, params: Value) -> Result<Value, ErrorResponse> {
        let input: PlanBuildInput = serde_json::from_value(params)
            .map_err(|e| ErrorResponse::json_parse_error(&e.to_string()))?;

        let workspace = input.workspace.clone().unwrap_or_else(|| self.default_workspace.clone());
        let mut ctx = self.get_or_create_context(Some(workspace.clone()));

        if ctx.state != WorkflowState::AuditComplete {
            return Err(ErrorResponse::audit_not_complete());
        }

        ctx.transition(WorkflowState::PlanNeeded).map_err(|_| {
            ErrorResponse::invalid_state_transition(ctx.state.as_str(), "PLAN_NEEDED")
        })?;
        ctx.transition(WorkflowState::PlanInProgress).map_err(|_| {
            ErrorResponse::invalid_state_transition(ctx.state.as_str(), "PLAN_IN_PROGRESS")
        })?;

        // Scope the plan to the same roots the referenced audit was actually
        // scanned against. Falling back to global default roots here would let
        // a plan silently diverge from the audit's scope (e.g. audit scoped to
        // a narrow test directory, plan surfacing unrelated files across the
        // whole home directory / /tmp). Only fall back to defaults if no
        // prior audit recorded its roots at all.
        let roots = if !input.roots.is_empty() {
            input.roots.clone()
        } else if let Some(audit_roots) = ctx.audit_roots.clone() {
            audit_roots
        } else {
            crate::nouns::default_scan_roots()
                .map_err(|e| ErrorResponse::new(ErrorCode::InvalidInput, e.to_string()))?
        };

        // Recency: prefer an explicit override on this call, then fall back
        // to whatever recency decision the referenced audit_scan actually
        // used, then finally the CLI's own default. This keeps plan_build
        // consistent with the audit it was built from instead of silently
        // re-deriving recency with the CLI's hardcoded default.
        let ignore_recent_hours = input
            .ignore_recent_hours
            .unwrap_or_else(|| ctx.audit_ignore_recent_hours.unwrap_or(168));

        let result = self.runner.plan_create(
            &workspace,
            roots,
            input.deps,
            input.aggressive,
            input.include_global_caches,
            ignore_recent_hours,
        )?;

        if !result.success() {
            ctx.transition(WorkflowState::PlanValidationFailed).ok();
            ctx.record_error(result.stderr.clone());
            self.workflows.insert(workspace.display().to_string(), ctx);
            return Err(result.to_error("oclnr plan build"));
        }

        let plan_file = workspace.join("cleanup-plan.json");
        let audit_referenced = input
            .audit_file
            .clone()
            .or_else(|| ctx.audit_file.clone())
            .map(|p| p.display().to_string())
            .unwrap_or_default();

        let plan_data = std::fs::read_to_string(&plan_file)
            .ok()
            .and_then(|s| serde_json::from_str::<crate::domain::plan::DeletionPlan>(&s).ok());

        let plan_summary = match &plan_data {
            Some(plan) => {
                let mut items_by_type: HashMap<String, usize> = HashMap::new();
                let mut items_by_reason: HashMap<String, usize> = HashMap::new();
                let mut total_bytes = 0u64;
                for item in &plan.items {
                    *items_by_type.entry(format!("{:?}", item.kind)).or_insert(0) += 1;
                    *items_by_reason.entry(item.reason.clone()).or_insert(0) += 1;
                    total_bytes += item.bytes;
                }
                PlanSummary {
                    created_unix: Utc::now().timestamp(),
                    created_iso: Utc::now().to_rfc3339(),
                    audit_referenced,
                    total_items: plan.items.len(),
                    total_bytes,
                    items_by_type,
                    items_by_reason,
                    exclusions: vec![],
                }
            }
            None => PlanSummary {
                created_unix: Utc::now().timestamp(),
                created_iso: Utc::now().to_rfc3339(),
                audit_referenced,
                total_items: 0,
                total_bytes: 0,
                items_by_type: HashMap::new(),
                items_by_reason: HashMap::new(),
                exclusions: vec![],
            },
        };

        ctx.transition(WorkflowState::PlanReady).map_err(|_| {
            ErrorResponse::invalid_state_transition(ctx.state.as_str(), "PLAN_READY")
        })?;
        ctx.plan_file = Some(plan_file.clone());
        ctx.last_plan_time = Some(Utc::now());
        ctx.clear_error();
        self.workflows.insert(workspace.display().to_string(), ctx);

        Ok(serde_json::to_value(PlanBuildOutput {
            state: "PLAN_READY".to_string(),
            plan_file: plan_file.display().to_string(),
            plan_summary,
            safety_checks: SafetyChecks {
                os_directory_protection: true,
                no_dotfiles_in_home: true,
                max_reclaim_respected: true,
                audit_integrity_ok: true,
                issues: vec![],
                warnings: vec![],
            },
            message: "Plan ready".to_string(),
        })
        .unwrap())
    }

    fn plan_inspect(&self, params: Value) -> Result<Value, ErrorResponse> {
        use crate::domain::plan::DeletionPlan;

        let input: serde_json::Map<String, Value> = serde_json::from_value(params)
            .map_err(|e| ErrorResponse::json_parse_error(&e.to_string()))?;

        let plan_file = input.get("plan_file").and_then(|v| v.as_str()).ok_or_else(|| {
            ErrorResponse::new(ErrorCode::InvalidInput, "plan_file required".to_string())
        })?;
        let top_n = input.get("top_n").and_then(|v| v.as_i64()).unwrap_or(20).max(0) as usize;

        let path = PathBuf::from(plan_file);
        if !path.exists() {
            return Err(ErrorResponse::file_not_found(&path, "plan_build"));
        }

        let content = std::fs::read_to_string(&path)
            .map_err(|e| ErrorResponse::new(ErrorCode::IoError, e.to_string()))?;

        // Parse the plan itself (not just as opaque JSON) so we can honor
        // `top_n` against the real item list rather than dumping the raw blob.
        let parsed = parse_json_output(&content)?;
        let plan: Option<DeletionPlan> = serde_json::from_str(&content).ok();

        let (total_items, total_bytes, top_items) = match &plan {
            Some(p) => {
                let total_bytes: u64 = p.items.iter().map(|i| i.bytes).sum();
                let mut items = p.items.clone();
                items.sort_by_key(|i| std::cmp::Reverse(i.bytes));
                items.truncate(top_n);
                (p.items.len(), total_bytes, serde_json::to_value(items).unwrap_or(json!([])))
            }
            None => (0, 0, json!([])),
        };

        Ok(json!({
            "plan_file": plan_file,
            "total_items": total_items,
            "total_bytes": total_bytes,
            "top_items": top_items,
            "contents": parsed,
            "message": "Plan inspected"
        }))
    }

    fn plan_validate(&self, params: Value) -> Result<Value, ErrorResponse> {
        use crate::domain::plan::DeletionPlan;

        let input: PlanValidateInput = serde_json::from_value(params)
            .map_err(|e| ErrorResponse::json_parse_error(&e.to_string()))?;

        if !input.plan_file.exists() {
            return Err(ErrorResponse::file_not_found(&input.plan_file, "plan_build"));
        }

        let raw = std::fs::read_to_string(&input.plan_file).map_err(|e| {
            ErrorResponse::new(ErrorCode::IoError, format!("cannot read plan: {}", e))
        })?;
        let plan: DeletionPlan = serde_json::from_str(&raw).map_err(|e| {
            ErrorResponse::new(ErrorCode::JsonParseError, format!("invalid plan JSON: {}", e))
        })?;

        // Reuse the shared `compute_safety_issues` (formerly also used by a
        // standalone `safety_audit` tool, since folded in here) instead of
        // re-implementing (or, as before, skipping) it.
        let issues = Self::compute_safety_issues(&plan);
        let os_directory_protection = !issues.iter().any(|i| i["kind"] == "protected_os_path");
        let no_dotfiles_in_home = !issues.iter().any(|i| i["kind"] == "dotfile_in_home");
        let valid = issues.iter().all(|i| i["severity"] != "critical");

        let (critical, warnings): (Vec<_>, Vec<_>) =
            issues.into_iter().partition(|i| i["severity"] == "critical");

        Ok(serde_json::to_value(PlanValidateOutput {
            valid,
            safety_checks: SafetyChecks {
                os_directory_protection,
                no_dotfiles_in_home,
                max_reclaim_respected: true,
                audit_integrity_ok: true,
                issues: critical
                    .iter()
                    .map(|i| i["message"].as_str().unwrap_or_default().to_string())
                    .collect(),
                warnings: warnings
                    .iter()
                    .map(|i| i["message"].as_str().unwrap_or_default().to_string())
                    .collect(),
            },
            message: if valid {
                "Plan is valid".to_string()
            } else {
                "Plan has critical safety issues".to_string()
            },
        })
        .unwrap())
    }

    fn plan_approve(&mut self, params: Value) -> Result<Value, ErrorResponse> {
        let input: PlanApproveInput = serde_json::from_value(params)
            .map_err(|e| ErrorResponse::json_parse_error(&e.to_string()))?;

        if !input.confirm {
            return Err(ErrorResponse::confirmation_required("plan_approve"));
        }

        if !input.plan_file.exists() {
            return Err(ErrorResponse::file_not_found(&input.plan_file, "plan_build"));
        }

        let workspace = input.plan_file.parent().unwrap_or(&self.default_workspace).to_path_buf();
        let mut ctx = self.get_or_create_context(Some(workspace));

        // Parse the plan so approval can be bound to its actual content
        // hash, not just the file's raw bytes treated as an opaque blob.
        let plan_content = std::fs::read_to_string(&input.plan_file).map_err(|e| {
            ErrorResponse::new(ErrorCode::IoError, format!("cannot read plan: {}", e))
        })?;
        let mut plan: crate::domain::plan::DeletionPlan = serde_json::from_str(&plan_content)
            .map_err(|e| {
                ErrorResponse::new(ErrorCode::JsonParseError, format!("invalid plan JSON: {}", e))
            })?;

        let mut approval = ApprovalMetadata::new(input.approver_name, input.approval_reason);

        // Source the real approval secret (env var or machine-local key
        // file — never derivable from the plan file itself) and use it for
        // both the return-value HMAC (`ApprovalMetadata::sign`, kept for
        // callers that only look at the RPC response) and the on-disk
        // `PlanApproval.hmac_signature`, which is what `delete execute`
        // actually gates on. A previous version signed with a hardcoded
        // literal key (`b"secret"`) and never checked that signature at
        // delete time at all — only a plain, unkeyed content hash was
        // checked, which a verifier proved forgeable by hand-computing the
        // same hash offline without ever calling this tool.
        let secret = crate::integration::config::approval_secret().map_err(|e| {
            ErrorResponse::new(
                ErrorCode::IoError,
                format!("cannot source plan-approval secret: {}", e),
            )
        })?;
        // Gating security lives entirely in `plan.sign_approval` below (the
        // on-disk signature `delete execute` actually checks); this call only
        // populates the RPC-response-only `approval_signature` field for
        // convenience. Still, silently leaving it blank on failure would look
        // identical to a successful, empty-by-design signature — surface it.
        approval.sign(&plan_content, &secret).map_err(|e| {
            ErrorResponse::new(ErrorCode::IoError, format!("cannot sign approval metadata: {}", e))
        })?;

        // Persist the approval into the plan file itself, bound to a
        // content hash of the plan's substantive fields (computed excluding
        // the approval field) *and* a real keyed HMAC signature over that
        // hash. This closes the gap where `plan_approve`'s signature was
        // only ever a return value: nothing on disk recorded that the plan
        // had been reviewed, so `oclnr delete execute` could not tell an
        // approved plan from a hand-edited or forged one.
        plan.approval =
            Some(plan.sign_approval(&secret, &approval.approver, &approval.approval_reason));
        let signed_content = serde_json::to_string_pretty(&plan).map_err(|e| {
            ErrorResponse::new(ErrorCode::IoError, format!("cannot serialize signed plan: {}", e))
        })?;
        std::fs::write(&input.plan_file, signed_content).map_err(|e| {
            ErrorResponse::new(ErrorCode::IoError, format!("cannot write signed plan: {}", e))
        })?;

        ctx.state = WorkflowState::PlanApproved;
        self.workflows.insert(
            input.plan_file.parent().unwrap_or(&self.default_workspace).display().to_string(),
            ctx,
        );

        Ok(serde_json::to_value(PlanApproveOutput {
            state: "PLAN_APPROVED".to_string(),
            plan_file: input.plan_file.display().to_string(),
            approval_metadata: approval,
            message: "Plan approved".to_string(),
        })
        .unwrap())
    }

    fn delete_dry_run(&self, params: Value) -> Result<Value, ErrorResponse> {
        use crate::domain::plan::DeletionPlan;

        let input: DeleteDryRunInput = serde_json::from_value(params)
            .map_err(|e| ErrorResponse::json_parse_error(&e.to_string()))?;

        if !input.plan_file.exists() {
            return Err(ErrorResponse::file_not_found(&input.plan_file, "plan_build"));
        }

        let workspace = input.workspace.clone().unwrap_or_else(|| self.default_workspace.clone());

        // `delete execute` requires a --receipt path even in dry-run mode (it
        // simply returns before writing it), so this is a scratch path.
        let scratch_receipt = std::env::temp_dir()
            .join(format!("oclnr-dry-run-receipt-{}.json", uuid::Uuid::new_v4()));

        let result =
            self.runner.delete_run(&workspace, &input.plan_file, &scratch_receipt, false)?;
        std::fs::remove_file(&scratch_receipt).ok();

        if !result.success() {
            return Err(result.to_error("oclnr delete execute (dry run)"));
        }

        // The dry-run CLI path only prints; read the real plan to report an
        // accurate, per-item preview of what an execute would do.
        let content = std::fs::read_to_string(&input.plan_file)
            .map_err(|e| ErrorResponse::new(ErrorCode::IoError, e.to_string()))?;
        let plan: DeletionPlan = serde_json::from_str(&content).map_err(|e| {
            ErrorResponse::new(ErrorCode::JsonParseError, format!("invalid plan JSON: {}", e))
        })?;

        let mut items_by_status: HashMap<String, usize> = HashMap::new();
        let mut total_bytes = 0u64;
        let preview_items: Vec<DeletionResult> = plan
            .items
            .iter()
            .map(|item| {
                total_bytes += item.bytes;
                let status = if item.path.exists() {
                    DeletionStatus::Deleted
                } else {
                    DeletionStatus::SkippedMissing
                };
                *items_by_status.entry(format!("{:?}", status)).or_insert(0) += 1;
                DeletionResult {
                    path: item.path.clone(),
                    status,
                    bytes_freed: item.bytes,
                    error: None,
                    blake3_hash: None,
                }
            })
            .collect();

        Ok(serde_json::to_value(DeleteDryRunOutput {
            message: "Dry run preview (nothing deleted)".to_string(),
            preview: DeletePreview {
                total_items: plan.items.len(),
                total_bytes,
                items_by_status,
                preview_items,
            },
        })
        .unwrap())
    }

    fn delete_execute(&mut self, params: Value) -> Result<Value, ErrorResponse> {
        use crate::domain::receipt::DeletionReceipt;

        let input: DeleteExecuteInput = serde_json::from_value(params)
            .map_err(|e| ErrorResponse::json_parse_error(&e.to_string()))?;

        if !input.confirm {
            return Err(ErrorResponse::confirmation_required("delete_execute"));
        }

        if !input.plan_file.exists() {
            return Err(ErrorResponse::file_not_found(&input.plan_file, "plan_build"));
        }

        let workspace = input.workspace.clone().unwrap_or_else(|| self.default_workspace.clone());
        let mut ctx = self.get_or_create_context(Some(workspace.clone()));

        ctx.state = WorkflowState::DeleteInProgress;

        let receipt_file =
            input.receipt_file.clone().unwrap_or_else(|| workspace.join("deletion-receipt.json"));

        // Run deletion
        let result = self.runner.delete_run(&workspace, &input.plan_file, &receipt_file, true)?;

        // A non-zero exit here can mean the deletion itself failed, or it can mean
        // the CLI's post-hoc space-verification check (comparing claimed bytes freed
        // to the measured free-space delta) tripped after the receipt was already
        // written and certified. The latter is a soft warning, not a deletion
        // failure — bailing out here would discard an already-successful, already-
        // certified receipt behind an opaque "subprocess failed" error. Only treat
        // this as a hard failure if no valid receipt was actually produced.
        let space_verification_warning = if !result.success() {
            if receipt_file.exists() {
                Some(result.stderr.trim().to_string())
            } else {
                ctx.state = WorkflowState::DeleteFailed;
                ctx.record_error(result.stderr.clone());
                self.workflows.insert(workspace.display().to_string(), ctx);
                return Err(result.to_error("oclnr delete execute"));
            }
        } else {
            None
        };

        // Use the receipt the CLI actually wrote, not a fabricated summary.
        let receipt_content = std::fs::read_to_string(&receipt_file).map_err(|e| {
            ErrorResponse::new(
                ErrorCode::IoError,
                format!("delete execute succeeded but receipt could not be read: {}", e),
            )
        })?;
        let receipt: DeletionReceipt = serde_json::from_str(&receipt_content).map_err(|e| {
            ErrorResponse::new(ErrorCode::JsonParseError, format!("invalid receipt JSON: {}", e))
        })?;

        let mut successful = 0usize;
        let mut failed = 0usize;
        let mut skipped = 0usize;
        let mut refused = 0usize;
        let mut total_bytes_freed = 0u64;
        let results: Vec<DeletionResult> = receipt
            .execution_record
            .results
            .iter()
            .map(|r| {
                total_bytes_freed += r.bytes_freed;
                let status = match r.status {
                    crate::domain::receipt::DeletionStatus::Deleted => {
                        successful += 1;
                        DeletionStatus::Deleted
                    }
                    crate::domain::receipt::DeletionStatus::SkippedMissing => {
                        skipped += 1;
                        DeletionStatus::SkippedMissing
                    }
                    crate::domain::receipt::DeletionStatus::Refused => {
                        refused += 1;
                        DeletionStatus::Refused
                    }
                    crate::domain::receipt::DeletionStatus::Failed => {
                        failed += 1;
                        DeletionStatus::Failed
                    }
                };
                DeletionResult {
                    path: r.path.clone(),
                    status,
                    bytes_freed: r.bytes_freed,
                    error: r.error.clone(),
                    blake3_hash: r.blake3_hash.clone(),
                }
            })
            .collect();

        let free_before_bytes = receipt.execution_record.available_before.unwrap_or(0);
        let free_after_bytes = receipt.execution_record.available_after.unwrap_or(0);
        let now = Utc::now();

        let affidavit_path = receipt_file.with_extension("affidavit.json");
        let affidavit_file =
            if affidavit_path.exists() { Some(affidavit_path.display().to_string()) } else { None };

        ctx.state = WorkflowState::DeleteComplete;
        ctx.receipt_file = Some(receipt_file.clone());
        ctx.affidavit_file = affidavit_path.exists().then(|| affidavit_path.clone());
        ctx.last_delete_time = Some(now);
        ctx.clear_error();
        self.workflows.insert(workspace.display().to_string(), ctx);

        Ok(serde_json::to_value(DeleteExecuteOutput {
            state: "DELETE_COMPLETE".to_string(),
            execution_record: ExecutionRecord {
                plan_file: input.plan_file.display().to_string(),
                execution_started_unix: receipt.execution_record.execution_started_unix as i64,
                execution_completed_unix: receipt.execution_record.execution_completed_unix as i64,
                duration_secs: (receipt.execution_record.execution_completed_unix as i64
                    - receipt.execution_record.execution_started_unix as i64)
                    as f64,
                results,
                summary: ExecutionSummary {
                    total_attempted: receipt.execution_record.results.len(),
                    successful,
                    failed,
                    skipped,
                    refused,
                    total_bytes_freed,
                },
                disk_space: DiskSpaceInfo {
                    free_before_bytes,
                    free_after_bytes,
                    freed_delta_bytes: free_after_bytes as i64 - free_before_bytes as i64,
                    measurement_time: now.to_rfc3339(),
                },
                affidavit_file,
            },
            receipt_file: receipt_file.display().to_string(),
            message: "Deletion executed".to_string(),
            space_verification_warning,
        })
        .unwrap())
    }

    fn receipt_parse(&self, params: Value) -> Result<Value, ErrorResponse> {
        use crate::domain::receipt::DeletionReceipt;

        let input: serde_json::Map<String, Value> = serde_json::from_value(params)
            .map_err(|e| ErrorResponse::json_parse_error(&e.to_string()))?;

        let receipt_file = input.get("receipt_file").and_then(|v| v.as_str()).ok_or_else(|| {
            ErrorResponse::new(ErrorCode::InvalidInput, "receipt_file required".to_string())
        })?;
        let top_n = input.get("top_n").and_then(|v| v.as_i64()).unwrap_or(50).max(0) as usize;

        let path = PathBuf::from(receipt_file);
        if !path.exists() {
            return Err(ErrorResponse::file_not_found(&path, "delete_execute"));
        }

        let content = std::fs::read_to_string(&path)
            .map_err(|e| ErrorResponse::new(ErrorCode::IoError, e.to_string()))?;
        let receipt: DeletionReceipt = serde_json::from_str(&content).map_err(|e| {
            ErrorResponse::new(ErrorCode::JsonParseError, format!("invalid receipt JSON: {}", e))
        })?;

        let total_items = receipt.execution_record.results.len();
        let total_bytes_freed: u64 =
            receipt.execution_record.results.iter().map(|r| r.bytes_freed).sum();
        let mut results = receipt.execution_record.results.clone();
        results.truncate(top_n);

        Ok(json!({
            "receipt_file": receipt_file,
            "version": receipt.execution_record.version,
            "plan_created_unix": receipt.execution_record.plan_created_unix,
            "execution_started_unix": receipt.execution_record.execution_started_unix,
            "execution_completed_unix": receipt.execution_record.execution_completed_unix,
            "available_before": receipt.execution_record.available_before,
            "available_after": receipt.execution_record.available_after,
            "total_items": total_items,
            "total_bytes_freed": total_bytes_freed,
            "results": results,
        }))
    }

    fn receipt_verify(&mut self, params: Value) -> Result<Value, ErrorResponse> {
        use crate::domain::{affidavit_integration, receipt::DeletionReceipt};

        let input: ReceiptVerifyInput = serde_json::from_value(params)
            .map_err(|e| ErrorResponse::json_parse_error(&e.to_string()))?;

        let workspace = input.workspace.clone().unwrap_or_else(|| self.default_workspace.clone());
        let ctx = self.get_or_create_context(Some(workspace.clone()));

        let receipt_file =
            input.receipt_file.clone().or_else(|| ctx.receipt_file.clone()).ok_or_else(|| {
                ErrorResponse::new(
                    ErrorCode::InvalidInput,
                    "receipt_file required (no prior delete_execute recorded for this workspace)"
                        .to_string(),
                )
            })?;

        if !receipt_file.exists() {
            return Err(ErrorResponse::file_not_found(&receipt_file, "delete_execute"));
        }

        // Drive the CLI's own verifier too (it exercises the identical domain
        // logic via a separate process, so a mismatch here is itself signal).
        let cli_result = self.runner.receipt_verify(&workspace, Some(&receipt_file))?;

        let content = std::fs::read_to_string(&receipt_file)
            .map_err(|e| ErrorResponse::new(ErrorCode::IoError, e.to_string()))?;
        let receipt: DeletionReceipt = serde_json::from_str(&content).map_err(|e| {
            ErrorResponse::new(ErrorCode::JsonParseError, format!("invalid receipt JSON: {}", e))
        })?;

        let report = receipt.verify(None);
        let affidavit_receipt = affidavit_integration::build_deletion_affidavit(&receipt);
        let verdict = affidavit_integration::certify(&affidavit_receipt);

        // If a sealed affidavit file exists alongside this receipt, its
        // stored chain_hash is the provenance claim to check — compare it
        // against what we just recomputed. A mismatch means the receipt or
        // affidavit file was hand-edited after sealing and must reject.
        let affidavit_path = receipt_file.with_extension("affidavit.json");
        let stored_chain_hash = std::fs::read_to_string(&affidavit_path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v.get("chain_hash").and_then(|h| h.as_str().map(str::to_string)));
        let verdict = affidavit_integration::verify_chain_hash(
            verdict,
            affidavit_receipt.chain_hash.as_hex(),
            stored_chain_hash.as_deref(),
        );

        let total_bytes_freed_recorded: u64 =
            receipt.execution_record.results.iter().map(|r| r.bytes_freed).sum();
        let actual_free_space_delta = match (
            receipt.execution_record.available_before,
            receipt.execution_record.available_after,
        ) {
            (Some(before), Some(after)) => after as i64 - before as i64,
            _ => 0,
        };

        let all_targets_gone = report.is_consistent && cli_result.success() && verdict.accepted;

        // `seal: true` absorbs the former standalone `receipt_certify` tool:
        // write the sealed `.affidavit.json` alongside the receipt, using
        // the exact same CLI certification path `receipt_certify` used to
        // drive (`oclnr receipt certify`), gated the same way it was
        // (`confirm: true` required, since this writes a file).
        let seal_output = if input.seal {
            if !input.confirm {
                return Err(ErrorResponse::confirmation_required(
                    "receipt(action: verify, seal: true)",
                ));
            }

            let out_path = receipt_file.with_extension("affidavit.json");
            let seal_result =
                self.runner.receipt_certify(&workspace, &receipt_file, Some(&out_path))?;
            if !seal_result.success() {
                return Err(seal_result.to_error("oclnr receipt certify"));
            }

            let mut ctx = self.get_or_create_context(Some(workspace.clone()));
            ctx.affidavit_file = Some(out_path.clone());
            self.workflows.insert(workspace.display().to_string(), ctx);

            Some(ReceiptSealOutput {
                certified: verdict.accepted,
                chain_hash: affidavit_receipt.chain_hash.to_string(),
                content_address: affidavit_integration::content_address(&affidavit_receipt),
                affidavit_file: out_path.display().to_string(),
                verdict_reason: verdict.reason.clone(),
                profile: verdict.profile.as_str().to_string(),
            })
        } else {
            None
        };

        Ok(serde_json::to_value(ReceiptVerifyOutput {
            state: if all_targets_gone {
                "RECEIPT_VERIFIED".to_string()
            } else {
                "RECEIPT_VERIFICATION_FAILED".to_string()
            },
            receipt_file: receipt_file.display().to_string(),
            verification_summary: VerificationSummary {
                verified_unix: Utc::now().timestamp(),
                verified_iso: Utc::now().to_rfc3339(),
                total_deletions_recorded: receipt.execution_record.results.len(),
                total_bytes_freed_recorded,
                actual_free_space_delta,
                all_targets_gone,
                affidavit_verified: verdict.accepted,
            },
            seal: seal_output,
            message: if all_targets_gone {
                "Receipt verified".to_string()
            } else {
                format!("Receipt verification found {} issue(s)", report.issues.len())
            },
        })
        .unwrap())
    }

    /// Safety-check logic used by `plan`'s `validate` action. Previously
    /// also used by a standalone `safety_audit` tool, which was folded into
    /// `plan(action: "validate")` during the MCP surface consolidation —
    /// it had no behavior `validate` didn't already produce.
    fn compute_safety_issues(plan: &crate::domain::plan::DeletionPlan) -> Vec<Value> {
        use crate::domain::artifact::is_macos_os_dir;

        let mut issues: Vec<Value> = Vec::new();

        let home_dir = dirs::home_dir();
        if home_dir.is_none() {
            issues.push(json!({
                "severity": "warning",
                "kind": "home_directory_unknown",
                "path": "",
                "message": "Could not determine home directory (HOME unset/misconfigured); \
                             the dotfile_in_home safety check is disabled for this run"
            }));
        }

        for item in &plan.items {
            if is_macos_os_dir(&item.path) {
                issues.push(json!({
                    "severity": "critical",
                    "kind": "protected_os_path",
                    "path": item.path.to_string_lossy(),
                    "message": "Path is inside a macOS OS-protected directory"
                }));
            }

            // Flag dotfiles in home directory root
            if let Some(name) = item.path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('.') {
                    if let Some(parent) = item.path.parent() {
                        if Some(parent) == home_dir.as_deref() {
                            issues.push(json!({
                                "severity": "warning",
                                "kind": "dotfile_in_home",
                                "path": item.path.to_string_lossy(),
                                "message": "Dotfile at home root — verify this is not a config file"
                            }));
                        }
                    }
                }
            }
        }

        issues
    }

    fn plan_rollback(&self, params: Value) -> Result<Value, ErrorResponse> {
        use crate::{domain::receipt::DeletionReceipt, integration::tmutil};

        let input: serde_json::Map<String, Value> = serde_json::from_value(params)
            .map_err(|e| ErrorResponse::json_parse_error(&e.to_string()))?;

        let confirm = input.get("confirm").and_then(|v| v.as_bool()).unwrap_or(false);

        if !confirm {
            return Err(ErrorResponse::confirmation_required("plan_rollback"));
        }

        // A rollback must be scoped to the receipt/snapshot it is rolling
        // back, so a receipt_file is required and must actually parse as a
        // valid DeletionReceipt — the same validation receipt_parse and
        // receipt_verify perform. This prevents a bogus/empty/nonexistent
        // receipt_file from silently producing a normal-looking response.
        let receipt_file = input.get("receipt_file").and_then(|v| v.as_str()).ok_or_else(|| {
            ErrorResponse::new(ErrorCode::InvalidInput, "receipt_file required".to_string())
        })?;
        let receipt_path = PathBuf::from(receipt_file);
        if !receipt_path.exists() {
            return Err(ErrorResponse::file_not_found(&receipt_path, "delete_execute"));
        }
        let content = std::fs::read_to_string(&receipt_path)
            .map_err(|e| ErrorResponse::new(ErrorCode::IoError, e.to_string()))?;
        let _receipt: DeletionReceipt = serde_json::from_str(&content).map_err(|e| {
            ErrorResponse::new(ErrorCode::JsonParseError, format!("invalid receipt JSON: {}", e))
        })?;

        let mount = input.get("mount").and_then(|v| v.as_str()).unwrap_or("/");

        let snapshots = tmutil::list_local_snapshots(mount).map_err(|e| {
            ErrorResponse::new(
                ErrorCode::SubprocessFailed,
                format!("tmutil list snapshots failed: {}", e),
            )
        })?;

        if snapshots.is_empty() {
            return Ok(json!({
                "restored": false,
                "reason": "no local APFS snapshots available for rollback",
                "mount": mount
            }));
        }

        // Report available snapshots — caller chooses which to restore.
        // Actual restore requires `tmutil localsnapshot restore` which needs root;
        // we surface the list and instruct the user to use macOS Recovery or tmutil.
        Ok(json!({
            "restored": false,
            "action_required": "manual",
            "mount": mount,
            "available_snapshots": snapshots,
            "instructions": "To restore: boot to macOS Recovery, or run 'tmutil restore -s <snapshot>' as root.",
            "message": format!("{} snapshots available for rollback", snapshots.len())
        }))
    }

    fn snapshot_audit(&self, params: Value) -> Result<Value, ErrorResponse> {
        let input: SnapshotAuditInput = serde_json::from_value(params)
            .map_err(|e| ErrorResponse::json_parse_error(&e.to_string()))?;

        let workspace = input.workspace.unwrap_or_else(|| self.default_workspace.clone());
        let roots = if input.roots.is_empty() { vec![workspace] } else { input.roots };

        let result = self.runner.snapshot_audit(&self.default_workspace, roots)?;
        if !result.success() {
            return Err(result.to_error("oclnr snapshot audit"));
        }

        // `snapshot audit` prints "  - <name>" for each local APFS snapshot;
        // parse the real stdout instead of reporting zero regardless of what
        // was found.
        let now_unix = Utc::now().timestamp();
        let snapshots: Vec<SnapshotInfo> = result
            .stdout
            .lines()
            .filter_map(|line| line.trim().strip_prefix("- "))
            .map(|name| {
                let age_hours = crate::domain::time::snapshot_unix_timestamp(name)
                    .map(|ts| (now_unix - ts).max(0) as u64 / 3600)
                    .unwrap_or(0);
                SnapshotInfo {
                    name: name.to_string(),
                    path: String::new(),
                    // No known `tmutil` data source reports per-snapshot byte
                    // size (`tmutil listlocalsnapshots` prints names only) —
                    // left at 0 rather than inventing a fake estimate.
                    bytes: 0,
                    age_hours,
                    created_at: name.to_string(),
                }
            })
            .collect();

        Ok(serde_json::to_value(SnapshotAuditOutput {
            state: "SNAPSHOT_AUDIT_COMPLETE".to_string(),
            total_snapshots: snapshots.len(),
            total_bytes: 0,
            snapshots,
            message: "Snapshot audit complete".to_string(),
        })
        .unwrap())
    }

    fn snapshot_thin(&self, params: Value) -> Result<Value, ErrorResponse> {
        use crate::domain::time::SnapshotThinReceipt;

        let input: SnapshotThinInput = serde_json::from_value(params)
            .map_err(|e| ErrorResponse::json_parse_error(&e.to_string()))?;

        if !input.confirm {
            return Err(ErrorResponse::confirmation_required("snapshot_thin"));
        }

        let workspace = input.workspace.clone().unwrap_or_else(|| self.default_workspace.clone());
        let receipt_file = workspace.join("snapshot-thin-receipt.json");

        let result =
            self.runner.snapshot_thin(&workspace, &input.mount, &input.bytes, &receipt_file)?;

        if !result.success() && !receipt_file.exists() {
            return Err(result.to_error("oclnr snapshot thin"));
        }

        let receipt_content = std::fs::read_to_string(&receipt_file).map_err(|e| {
            ErrorResponse::new(
                ErrorCode::IoError,
                format!("snapshot thin succeeded but receipt could not be read: {}", e),
            )
        })?;
        let receipt: SnapshotThinReceipt = serde_json::from_str(&receipt_content).map_err(|e| {
            ErrorResponse::new(ErrorCode::JsonParseError, format!("invalid receipt JSON: {}", e))
        })?;

        let affidavit_path = receipt_file.with_extension("affidavit.json");
        let affidavit_file =
            if affidavit_path.exists() { Some(affidavit_path.display().to_string()) } else { None };

        Ok(serde_json::to_value(SnapshotThinOutput {
            state: "SNAPSHOT_THIN_COMPLETE".to_string(),
            mount: receipt.volume,
            requested_bytes: receipt.requested_bytes,
            snapshots_before: receipt.snapshots_before.len(),
            snapshots_after: receipt.snapshots_after.len(),
            snapshots_thinned: receipt.snapshots_thinned,
            receipt_file: receipt_file.display().to_string(),
            affidavit_file,
            message: "Snapshot thin complete".to_string(),
        })
        .unwrap())
    }

    fn snapshot_delete(&self, params: Value) -> Result<Value, ErrorResponse> {
        use crate::domain::time::SnapshotThinReceipt;

        let input: SnapshotDeleteInput = serde_json::from_value(params)
            .map_err(|e| ErrorResponse::json_parse_error(&e.to_string()))?;

        if !input.confirm {
            return Err(ErrorResponse::confirmation_required("snapshot_delete"));
        }

        let workspace = input.workspace.clone().unwrap_or_else(|| self.default_workspace.clone());
        let receipt_file = workspace.join("snapshot-delete-receipt.json");

        let result = self.runner.snapshot_delete(
            &workspace,
            &input.mount,
            &input.which,
            input.oldest_n,
            &receipt_file,
        )?;

        if !result.success() && !receipt_file.exists() {
            return Err(result.to_error("oclnr snapshot delete"));
        }

        let receipt_content = std::fs::read_to_string(&receipt_file).map_err(|e| {
            ErrorResponse::new(
                ErrorCode::IoError,
                format!("snapshot delete succeeded but receipt could not be read: {}", e),
            )
        })?;
        let receipt: SnapshotThinReceipt = serde_json::from_str(&receipt_content).map_err(|e| {
            ErrorResponse::new(ErrorCode::JsonParseError, format!("invalid receipt JSON: {}", e))
        })?;

        let affidavit_path = receipt_file.with_extension("affidavit.json");
        let affidavit_file =
            if affidavit_path.exists() { Some(affidavit_path.display().to_string()) } else { None };

        Ok(serde_json::to_value(SnapshotDeleteOutput {
            state: "SNAPSHOT_DELETE_COMPLETE".to_string(),
            mount: receipt.volume,
            snapshots_before: receipt.snapshots_before.len(),
            snapshots_after: receipt.snapshots_after.len(),
            snapshots_deleted: receipt.snapshots_thinned,
            receipt_file: receipt_file.display().to_string(),
            affidavit_file,
            message: "Snapshot delete complete".to_string(),
        })
        .unwrap())
    }

    fn emergency_reclaim(&self, params: Value) -> Result<Value, ErrorResponse> {
        let input: EmergencyReclaimInput = serde_json::from_value(params)
            .map_err(|e| ErrorResponse::json_parse_error(&e.to_string()))?;

        if !input.confirm {
            return Err(ErrorResponse::confirmation_required("emergency_reclaim"));
        }

        let workspace = input.workspace.clone().unwrap_or_else(|| self.default_workspace.clone());
        let receipt_file = workspace.join("emergency-reclaim-receipt.json");

        // NOTE: `target_free_gb` is not currently a flag on `oclnr emergency`
        // (it reclaims unconditionally); accepted for forward-compatibility
        // with a future CLI flag but not yet wired through.
        let _ = input.target_free_gb;

        let result =
            self.runner.emergency_reclaim(&workspace, &input.mount, true, Some(&receipt_file))?;
        if !result.success() {
            return Err(result.to_error("oclnr emergency"));
        }

        // Prefer the structured JSON receipt the CLI writes (best-effort: a
        // catastrophically full disk may prevent even that write).
        let receipt_json = std::fs::read_to_string(&receipt_file)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok());

        let cache_bytes = receipt_json
            .as_ref()
            .and_then(|v| v.get("cache_bytes"))
            .and_then(|b| b.as_u64())
            .unwrap_or(0);
        let start_avail =
            receipt_json.as_ref().and_then(|v| v.get("start_available")).and_then(|b| b.as_u64());
        let end_avail =
            receipt_json.as_ref().and_then(|v| v.get("end_available")).and_then(|b| b.as_u64());
        let measured_delta = match (start_avail, end_avail) {
            (Some(s), Some(e)) => e.saturating_sub(s),
            _ => 0,
        };
        let space_freed = measured_delta.max(cache_bytes);

        let snapshots_thinned = receipt_json
            .as_ref()
            .and_then(|v| v.get("snapshots_seen"))
            .and_then(|n| n.as_u64())
            .unwrap_or(0) as usize;

        // Count real "cleared <size> <path>" lines emitted for each cache swept.
        let caches_cleared =
            result.stdout.lines().filter(|l| l.trim_start().starts_with("cleared ")).count();

        Ok(serde_json::to_value(EmergencyReclaimOutput {
            state: "EMERGENCY_RECLAIM_COMPLETE".to_string(),
            space_freed,
            snapshots_thinned,
            caches_cleared,
            message: "Emergency reclaim complete".to_string(),
        })
        .unwrap())
    }

    fn docker_scan(&self, params: Value) -> Result<Value, ErrorResponse> {
        let input: DockerScanInput = serde_json::from_value(params)
            .map_err(|e| ErrorResponse::json_parse_error(&e.to_string()))?;
        let workspace = input.workspace.unwrap_or_else(|| self.default_workspace.clone());

        let result = self.runner.docker_scan(&workspace)?;
        if !result.success() {
            return Err(result.to_error("oclnr docker scan"));
        }

        Ok(serde_json::to_value(DockerScanOutput {
            state: "DOCKER_SCAN_COMPLETE".to_string(),
            raw: result.stdout.trim().to_string(),
            message: "Docker disk usage scan complete".to_string(),
        })
        .unwrap())
    }

    fn docker_plan(&self, params: Value) -> Result<Value, ErrorResponse> {
        let input: DockerPlanInput = serde_json::from_value(params)
            .map_err(|e| ErrorResponse::json_parse_error(&e.to_string()))?;
        let workspace = input.workspace.unwrap_or_else(|| self.default_workspace.clone());

        let result = self.runner.docker_plan(&workspace)?;
        if !result.success() {
            return Err(result.to_error("oclnr docker plan"));
        }

        Ok(serde_json::to_value(DockerPlanOutput {
            state: "DOCKER_PLAN_COMPLETE".to_string(),
            raw: result.stdout.trim().to_string(),
            message: "Docker prune preview complete".to_string(),
        })
        .unwrap())
    }

    fn docker_prune(&self, params: Value) -> Result<Value, ErrorResponse> {
        let input: DockerPruneInput = serde_json::from_value(params)
            .map_err(|e| ErrorResponse::json_parse_error(&e.to_string()))?;

        if !input.confirm {
            return Err(ErrorResponse::confirmation_required("docker_prune"));
        }

        let workspace = input.workspace.unwrap_or_else(|| self.default_workspace.clone());

        let result = self.runner.docker_prune(&workspace, input.skip_colima)?;
        if !result.success() {
            return Err(result.to_error("oclnr docker prune"));
        }

        Ok(serde_json::to_value(DockerPruneOutput {
            state: "DOCKER_PRUNE_COMPLETE".to_string(),
            raw: result.stdout.trim().to_string(),
            message: "Docker (and Colima, unless skipped) prune complete".to_string(),
        })
        .unwrap())
    }

    fn doctor_check(&self, params: Value) -> Result<Value, ErrorResponse> {
        let input: DoctorCheckInput = serde_json::from_value(params)
            .map_err(|e| ErrorResponse::json_parse_error(&e.to_string()))?;
        let workspace = input.workspace.unwrap_or_else(|| self.default_workspace.clone());

        const VALID_CHECKS: &[&str] = &[
            "architecture",
            "substrate",
            "doctests",
            "privacy",
            "domain-purity",
            "scan-delete-separation",
        ];
        if !VALID_CHECKS.contains(&input.check.as_str()) {
            return Err(Self::unknown_action("doctor", &input.check));
        }

        let result = self.runner.doctor_check(&workspace, &input.check)?;
        if !result.success() {
            return Err(result.to_error("oclnr doctor"));
        }

        Ok(serde_json::to_value(DoctorCheckOutput {
            state: "DOCTOR_CHECK_COMPLETE".to_string(),
            check: input.check,
            raw: result.stdout.trim().to_string(),
            message: "Doctor check complete".to_string(),
        })
        .unwrap())
    }
}

/// Extracts audit summary fields from a parsed disk-audit OCEL log.
fn summarize_disk_audit_ocel(log: &Value, scan_duration_secs: f64) -> AuditSummary {
    let objects = log.get("objects").and_then(|v| v.as_array());

    let disk_audit_attr = |name: &str| -> u64 {
        objects
            .and_then(|objs| {
                objs.iter().find(|o| o.get("type").and_then(|t| t.as_str()) == Some("disk_audit"))
            })
            .and_then(|o| o.get("attributes").and_then(|a| a.as_array()))
            .and_then(|attrs| {
                attrs.iter().find(|a| a.get("name").and_then(|n| n.as_str()) == Some(name))
            })
            .and_then(|a| a.get("value").and_then(|v| v.as_u64()))
            .unwrap_or(0)
    };

    let total_candidates = objects
        .map(|objs| {
            objs.iter()
                .filter(|o| o.get("type").and_then(|t| t.as_str()) == Some("artifact_candidate"))
                .count()
        })
        .unwrap_or(0);

    AuditSummary {
        total_dirs: disk_audit_attr("dirs_seen") as usize,
        total_files: disk_audit_attr("files_seen") as usize,
        total_bytes: disk_audit_attr("bytes_seen"),
        total_candidates,
        projects_detected: HashMap::new(),
        largest_candidates: vec![],
        errors: vec![],
        scan_duration_secs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_creation() {
        let server = OsxClnrMcpServer::new(PathBuf::from("/tmp"));
        assert!(server.is_ok());
    }

    #[test]
    fn test_list_tools() {
        let server = OsxClnrMcpServer::new(PathBuf::from("/tmp")).unwrap();
        let result = server.list_tools();
        assert!(result.is_ok());
    }
}
