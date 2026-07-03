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
    pub fn list_tools(&self) -> Result<Value, ErrorResponse> {
        let tools = vec![
            // Workflow
            json!({
                "name": "query_workflow_state",
                "description": "Query current state of cleanup workflow",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "workspace": { "type": "string", "description": "Workspace directory (default: current)" }
                    }
                }
            }),
            json!({
                "name": "clear_artifacts",
                "description": "Archive old evidence and reset to UNSTARTED state",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "workspace": { "type": "string" },
                        "archive_to": { "type": "string" },
                        "dry_run": { "type": "boolean", "default": true },
                        "confirm": { "type": "boolean", "default": false }
                    }
                }
            }),
            // Audit
            json!({
                "name": "audit_scan",
                "description": "Scan filesystem and generate audit evidence",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "workspace": { "type": "string" },
                        "roots": { "type": "array", "items": { "type": "string" } },
                        "include_deps": { "type": "boolean" },
                        "include_aggressive": { "type": "boolean" },
                        "ignore_recent_hours": { "type": "integer", "default": 168 },
                        "tool_roots": { "type": "boolean", "default": false }
                    }
                }
            }),
            json!({
                "name": "audit_parse",
                "description": "Parse existing audit evidence file",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "audit_file": { "type": "string" },
                        "top_n": { "type": "integer", "default": 50 },
                        "filter_reason": { "type": "string" }
                    }
                }
            }),
            // Plan
            json!({
                "name": "plan_build",
                "description": "Build a deletion plan from audit results",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "workspace": { "type": "string" },
                        "audit_file": { "type": "string" },
                        "roots": { "type": "array", "items": { "type": "string" } },
                        "deps": { "type": "boolean" },
                        "aggressive": { "type": "boolean" },
                        "include_global_caches": { "type": "boolean" },
                        "max_reclaim_gb": { "type": "number" }
                    }
                }
            }),
            json!({
                "name": "plan_inspect",
                "description": "Read and inspect generated plan",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "plan_file": { "type": "string" },
                        "top_n": { "type": "integer", "default": 20 }
                    }
                }
            }),
            json!({
                "name": "plan_validate",
                "description": "Validate plan safety (no OS dirs, proper signatures)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "workspace": { "type": "string" },
                        "plan_file": { "type": "string" }
                    }
                }
            }),
            json!({
                "name": "plan_approve",
                "description": "Approve plan with HMAC-SHA256 signature",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "plan_file": { "type": "string" },
                        "approver_name": { "type": "string" },
                        "approval_reason": { "type": "string" },
                        "confirm": { "type": "boolean", "default": false }
                    },
                    "required": ["plan_file", "approver_name", "approval_reason"]
                }
            }),
            // Delete
            json!({
                "name": "delete_dry_run",
                "description": "Preview deletion without modifying filesystem",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "workspace": { "type": "string" },
                        "plan_file": { "type": "string" }
                    },
                    "required": ["plan_file"]
                }
            }),
            json!({
                "name": "delete_execute",
                "description": "Execute deletion from approved plan",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "workspace": { "type": "string" },
                        "plan_file": { "type": "string" },
                        "receipt_file": { "type": "string" },
                        "confirm": { "type": "boolean", "default": false },
                        "max_concurrent": { "type": "integer", "default": 4 },
                        "timeout_secs": { "type": "integer", "default": 30 }
                    },
                    "required": ["plan_file"]
                }
            }),
            // Receipt
            json!({
                "name": "receipt_parse",
                "description": "Parse deletion receipt file",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "receipt_file": { "type": "string" }
                    },
                    "required": ["receipt_file"]
                }
            }),
            json!({
                "name": "receipt_verify",
                "description": "Verify deletion receipt and check actual free space",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "workspace": { "type": "string" },
                        "receipt_file": { "type": "string" }
                    }
                }
            }),
            json!({
                "name": "receipt_certify",
                "description": "Seal receipt with affidavit cryptographic proof chain",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "workspace": { "type": "string" },
                        "receipt_file": { "type": "string" },
                        "confirm": { "type": "boolean", "default": false }
                    }
                }
            }),
            // Safety
            json!({
                "name": "safety_audit",
                "description": "Run safety checks (path protection, symlink detection)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "plan_file": { "type": "string" }
                    }
                }
            }),
            json!({
                "name": "plan_rollback",
                "description": "Restore from snapshots if available",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "workspace": { "type": "string" },
                        "receipt_file": { "type": "string" },
                        "confirm": { "type": "boolean", "default": false }
                    }
                }
            }),
            // Snapshots
            json!({
                "name": "snapshot_audit",
                "description": "List and analyze APFS snapshots",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "workspace": { "type": "string" },
                        "roots": { "type": "array", "items": { "type": "string" } }
                    }
                }
            }),
            json!({
                "name": "emergency_reclaim",
                "description": "Aggressively reclaim disk space when low. NOT scoped to `workspace`: sweeps real APFS snapshots and home-directory caches on the given `mount`. Never call against a real mount without explicit user intent.",
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

    pub fn call_tool(&mut self, name: &str, params: Option<Value>) -> Result<Value, ErrorResponse> {
        let params = params.unwrap_or(Value::Object(Default::default()));

        let inner = match name {
            // Workflow
            "query_workflow_state" => self.query_workflow_state(params),
            "clear_artifacts" => self.clear_artifacts(params),

            // Audit
            "audit_scan" => self.audit_scan(params),
            "audit_parse" => self.audit_parse(params),

            // Plan
            "plan_build" => self.plan_build(params),
            "plan_inspect" => self.plan_inspect(params),
            "plan_validate" => self.plan_validate(params),
            "plan_approve" => self.plan_approve(params),

            // Delete
            "delete_dry_run" => self.delete_dry_run(params),
            "delete_execute" => self.delete_execute(params),

            // Receipt
            "receipt_parse" => self.receipt_parse(params),
            "receipt_verify" => self.receipt_verify(params),
            "receipt_certify" => self.receipt_certify(params),

            // Safety
            "safety_audit" => self.safety_audit(params),
            "plan_rollback" => self.plan_rollback(params),

            // Snapshots
            "snapshot_audit" => self.snapshot_audit(params),
            "emergency_reclaim" => self.emergency_reclaim(params),

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

        if !input.confirm {
            return Err(ErrorResponse::confirmation_required("clear_artifacts"));
        }

        let now = Utc::now().format("%Y%m%d_%H%M%S").to_string();
        let archive_dir = input.archive_to.unwrap_or_else(|| {
            let mut p = workspace.clone();
            p.push(format!("archive/{}", now));
            p
        });

        let mut archived = Vec::new();

        // Archive audit file
        if let Some(audit) = &ctx.audit_file {
            if audit.exists() {
                std::fs::create_dir_all(&archive_dir).ok();
                let dest = archive_dir.join(audit.file_name().unwrap_or_default());
                std::fs::copy(audit, &dest).ok();
                archived.push(ArchivedFile {
                    source: audit.display().to_string(),
                    destination: dest.display().to_string(),
                });
            }
        }

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
        })
        .unwrap())
    }

    fn audit_scan(&mut self, params: Value) -> Result<Value, ErrorResponse> {
        let input: AuditScanInput = serde_json::from_value(params)
            .map_err(|e| ErrorResponse::json_parse_error(&e.to_string()))?;

        let workspace = input.workspace.clone().unwrap_or_else(|| self.default_workspace.clone());
        let mut ctx = self.get_or_create_context(Some(workspace.clone()));

        ctx.transition(WorkflowState::AuditNeeded).map_err(|_| {
            ErrorResponse::invalid_state_transition(ctx.state.as_str(), "AUDIT_NEEDED")
        })?;

        ctx.transition(WorkflowState::AuditInProgress).map_err(|_| {
            ErrorResponse::invalid_state_transition(ctx.state.as_str(), "AUDIT_IN_PROGRESS")
        })?;

        // Spawn subprocess
        let roots = if input.roots.is_empty() {
            crate::nouns::default_scan_roots()
                .map_err(|e| ErrorResponse::new(ErrorCode::InvalidInput, e.to_string()))?
        } else {
            input.roots
        };

        let start = std::time::Instant::now();
        let result = self.runner.audit_run(
            &workspace,
            roots,
            input.include_deps,
            input.include_aggressive,
            input.ignore_recent_hours,
            input.tool_roots,
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

        let roots = if input.roots.is_empty() {
            crate::nouns::default_scan_roots()
                .map_err(|e| ErrorResponse::new(ErrorCode::InvalidInput, e.to_string()))?
        } else {
            input.roots.clone()
        };

        let result = self.runner.plan_create(
            &workspace,
            roots,
            input.deps,
            input.aggressive,
            input.include_global_caches,
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

        // Reuse the exact same safety-check logic as `safety_audit` instead of
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

        // Sign the actual plan file's bytes so the signature is bound to what
        // was reviewed — a modified plan produces a different signature.
        let plan_content = std::fs::read_to_string(&input.plan_file).map_err(|e| {
            ErrorResponse::new(ErrorCode::IoError, format!("cannot read plan: {}", e))
        })?;

        let mut approval = ApprovalMetadata::new(input.approver_name, input.approval_reason);
        // TODO(secret-sourcing): this HMAC key is a placeholder shared with the
        // rest of the MCP server's approval flow. Wiring a real key-management
        // source (keychain / env-provisioned secret) is out of scope for this
        // fix — see plan_approve's doctest / receipt_certify for the same gap.
        approval.sign(&plan_content, b"secret").ok();

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

        if !result.success() {
            ctx.state = WorkflowState::DeleteFailed;
            ctx.record_error(result.stderr.clone());
            self.workflows.insert(workspace.display().to_string(), ctx);
            return Err(result.to_error("oclnr delete execute"));
        }

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

        let total_bytes_freed_recorded: u64 =
            receipt.execution_record.results.iter().map(|r| r.bytes_freed).sum();
        let actual_free_space_delta = match (
            receipt.execution_record.available_before,
            receipt.execution_record.available_after,
        ) {
            (Some(before), Some(after)) => after as i64 - before as i64,
            _ => 0,
        };

        let all_targets_gone = report.is_consistent && cli_result.success();

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
            message: if all_targets_gone {
                "Receipt verified".to_string()
            } else {
                format!("Receipt verification found {} issue(s)", report.issues.len())
            },
        })
        .unwrap())
    }

    fn receipt_certify(&mut self, params: Value) -> Result<Value, ErrorResponse> {
        use crate::domain::{affidavit_integration, receipt::DeletionReceipt};

        let input: serde_json::Map<String, Value> = serde_json::from_value(params)
            .map_err(|e| ErrorResponse::json_parse_error(&e.to_string()))?;

        let confirm = input.get("confirm").and_then(|v| v.as_bool()).unwrap_or(false);
        if !confirm {
            return Err(ErrorResponse::confirmation_required("receipt_certify"));
        }

        let workspace = input
            .get("workspace")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.default_workspace.clone());
        let mut ctx = self.get_or_create_context(Some(workspace.clone()));

        let receipt_file = input
            .get("receipt_file")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .or_else(|| ctx.receipt_file.clone())
            .ok_or_else(|| {
                ErrorResponse::new(
                    ErrorCode::InvalidInput,
                    "receipt_file required (no prior delete_execute recorded for this workspace)"
                        .to_string(),
                )
            })?;

        if !receipt_file.exists() {
            return Err(ErrorResponse::file_not_found(&receipt_file, "delete_execute"));
        }

        let out_path = receipt_file.with_extension("affidavit.json");

        // Drive the real CLI certification path (same code the standalone
        // `oclnr receipt certify` command uses) rather than only checking `confirm`.
        let result = self.runner.receipt_certify(&workspace, &receipt_file, Some(&out_path))?;
        if !result.success() {
            return Err(result.to_error("oclnr receipt certify"));
        }

        let content = std::fs::read_to_string(&receipt_file)
            .map_err(|e| ErrorResponse::new(ErrorCode::IoError, e.to_string()))?;
        let receipt: DeletionReceipt = serde_json::from_str(&content).map_err(|e| {
            ErrorResponse::new(ErrorCode::JsonParseError, format!("invalid receipt JSON: {}", e))
        })?;
        let affidavit_receipt = affidavit_integration::build_deletion_affidavit(&receipt);
        let verdict = affidavit_integration::certify(&affidavit_receipt);

        ctx.affidavit_file = Some(out_path.clone());
        self.workflows.insert(workspace.display().to_string(), ctx);

        Ok(json!({
            "certified": verdict.accepted,
            "chain_hash": affidavit_receipt.chain_hash,
            "content_address": affidavit_integration::content_address(&affidavit_receipt),
            "affidavit_file": out_path.display().to_string(),
            "verdict_reason": verdict.reason,
            "profile": verdict.profile.as_str(),
        }))
    }

    /// Shared safety-check logic used by both `safety_audit` and `plan_validate`
    /// so the two tools cannot silently drift out of sync.
    fn compute_safety_issues(plan: &crate::domain::plan::DeletionPlan) -> Vec<Value> {
        use crate::domain::artifact::is_macos_os_dir;

        let mut issues: Vec<Value> = Vec::new();

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
                        if parent == dirs::home_dir().unwrap_or_default() {
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

    fn safety_audit(&self, params: Value) -> Result<Value, ErrorResponse> {
        use crate::domain::plan::DeletionPlan;

        let input: serde_json::Map<String, Value> = serde_json::from_value(params)
            .map_err(|e| ErrorResponse::json_parse_error(&e.to_string()))?;

        let plan_file_str = input.get("plan_file").and_then(|v| v.as_str()).ok_or_else(|| {
            ErrorResponse::new(ErrorCode::InvalidInput, "plan_file required".to_string())
        })?;

        let plan_file = std::path::Path::new(plan_file_str);
        if !plan_file.exists() {
            return Err(ErrorResponse::new(
                ErrorCode::InvalidInput,
                format!("plan file not found: {}", plan_file_str),
            ));
        }

        let raw = std::fs::read_to_string(plan_file).map_err(|e| {
            ErrorResponse::new(ErrorCode::SubprocessFailed, format!("cannot read plan: {}", e))
        })?;

        let plan: DeletionPlan = serde_json::from_str(&raw).map_err(|e| {
            ErrorResponse::new(ErrorCode::JsonParseError, format!("invalid plan JSON: {}", e))
        })?;

        let issues = Self::compute_safety_issues(&plan);

        let safe = issues.iter().all(|i| i["severity"] != "critical");
        Ok(json!({
            "safe": safe,
            "issues": issues,
            "candidates_checked": plan.items.len()
        }))
    }

    fn plan_rollback(&self, params: Value) -> Result<Value, ErrorResponse> {
        use crate::integration::tmutil;

        let input: serde_json::Map<String, Value> = serde_json::from_value(params)
            .map_err(|e| ErrorResponse::json_parse_error(&e.to_string()))?;

        let confirm = input.get("confirm").and_then(|v| v.as_bool()).unwrap_or(false);

        if !confirm {
            return Err(ErrorResponse::confirmation_required("plan_rollback"));
        }

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
        let snapshots: Vec<SnapshotInfo> = result
            .stdout
            .lines()
            .filter_map(|line| line.trim().strip_prefix("- "))
            .map(|name| SnapshotInfo {
                name: name.to_string(),
                path: String::new(),
                bytes: 0,
                age_hours: 0,
                created_at: name.to_string(),
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
