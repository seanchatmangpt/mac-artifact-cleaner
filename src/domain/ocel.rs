//! Object-Centric Event Log (OCEL v2) exporter.

use crate::domain::tool_roots::ToolRootReport;
use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
pub use wasm4pm_compat::ocel::*;

/// Builds an OCEL log structure for the collected tool roots.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::ocel::build_tool_roots_ocel;
///
/// // Positive case: empty input list still creates an audit run log object
/// let log = build_tool_roots_ocel(&[]);
/// assert_eq!(log.objects.len(), 1); // includes disk_audit object
/// assert_eq!(log.objects[0].object_type, "disk_audit");
/// ```
pub fn build_tool_roots_ocel(tool_roots: &[ToolRootReport]) -> OCEL {
    let now = chrono::Utc::now().with_timezone(&chrono::FixedOffset::east_opt(0).unwrap());
    let mut objects = Vec::new();
    let mut events = Vec::new();
    let audit_obj_id = format!("audit-{}", chrono::Utc::now().timestamp());

    objects.push(OCELObject {
        id: audit_obj_id.clone(),
        object_type: "disk_audit".to_string(),
        attributes: vec![timed_attr(
            "created_at",
            &now,
            serde_json::json!(now.to_rfc3339()),
        )],
        relationships: vec![],
    });

    events.push(OCELEvent {
        id: format!("event-audit-started-{}", chrono::Utc::now().timestamp()),
        event_type: "disk_audit_started".to_string(),
        time: now,
        attributes: vec![attr("tool", serde_json::json!("mac-disk-auditor"))],
        relationships: vec![OCELRelationship {
            object_id: audit_obj_id.clone(),
            qualifier: "audit-run".to_string(),
        }],
    });

    for (idx, root) in tool_roots.iter().enumerate() {
        let object_id = stable_object_id("tool-root", &root.path);
        let observed_time = unix_to_datetime(root.newest_descendant_modified_unix);

        objects.push(OCELObject {
            id: object_id.clone(),
            object_type: "tool_root".to_string(),
            attributes: vec![
                timed_attr("path", &observed_time, serde_json::json!(root.path)),
                timed_attr("category", &observed_time, serde_json::json!(root.category)),
                timed_attr("bytes", &observed_time, serde_json::json!(root.bytes)),
                timed_attr("human", &observed_time, serde_json::json!(root.human)),
                timed_attr("files", &observed_time, serde_json::json!(root.files)),
                timed_attr("dirs", &observed_time, serde_json::json!(root.dirs)),
                timed_attr(
                    "last_accessed_unix",
                    &observed_time,
                    serde_json::json!(root.last_accessed_unix),
                ),
                timed_attr(
                    "last_modified_unix",
                    &observed_time,
                    serde_json::json!(root.last_modified_unix),
                ),
                timed_attr(
                    "metadata_changed_unix",
                    &observed_time,
                    serde_json::json!(root.metadata_changed_unix),
                ),
                timed_attr(
                    "newest_descendant_modified_unix",
                    &observed_time,
                    serde_json::json!(root.newest_descendant_modified_unix),
                ),
                timed_attr(
                    "newest_descendant_path",
                    &observed_time,
                    serde_json::json!(root.newest_descendant_path),
                ),
                timed_attr(
                    "recommendation",
                    &observed_time,
                    serde_json::json!(root.recommendation),
                ),
                timed_attr(
                    "rationale",
                    &observed_time,
                    serde_json::json!(root.rationale),
                ),
            ],
            relationships: vec![OCELRelationship {
                object_id: audit_obj_id.clone(),
                qualifier: "observed-in".to_string(),
            }],
        });

        events.push(OCELEvent {
            id: format!("event-tool-root-observed-{:06}", idx),
            event_type: "tool_root_observed".to_string(),
            time: observed_time,
            attributes: vec![
                attr("path", serde_json::json!(root.path)),
                attr("category", serde_json::json!(root.category)),
                attr("bytes", serde_json::json!(root.bytes)),
                attr("files", serde_json::json!(root.files)),
                attr("dirs", serde_json::json!(root.dirs)),
                attr("recommendation", serde_json::json!(root.recommendation)),
                attr("rationale", serde_json::json!(root.rationale)),
            ],
            relationships: vec![
                OCELRelationship {
                    object_id: audit_obj_id.clone(),
                    qualifier: "audit-run".to_string(),
                },
                OCELRelationship {
                    object_id: object_id.clone(),
                    qualifier: "observed-tool-root".to_string(),
                },
            ],
        });

        if root.recommendation.contains("cleanup")
            || root.recommendation.contains("review")
            || root.recommendation.contains("delete")
        {
            events.push(OCELEvent {
                id: format!("event-tool-root-review-proposed-{:06}", idx),
                event_type: "tool_root_review_proposed".to_string(),
                time: now,
                attributes: vec![
                    attr("path", serde_json::json!(root.path)),
                    attr("recommendation", serde_json::json!(root.recommendation)),
                    attr("rationale", serde_json::json!(root.rationale)),
                ],
                relationships: vec![
                    OCELRelationship {
                        object_id: audit_obj_id.clone(),
                        qualifier: "audit-run".to_string(),
                    },
                    OCELRelationship {
                        object_id,
                        qualifier: "review-target".to_string(),
                    },
                ],
            });
        }
    }

    OCEL {
        event_types: vec![
            OCELType {
                name: "disk_audit_started".to_string(),
                attributes: vec![attr_def("tool", "string")],
            },
            OCELType {
                name: "tool_root_observed".to_string(),
                attributes: vec![
                    attr_def("path", "string"),
                    attr_def("category", "string"),
                    attr_def("bytes", "integer"),
                    attr_def("files", "integer"),
                    attr_def("dirs", "integer"),
                    attr_def("recommendation", "string"),
                    attr_def("rationale", "string"),
                ],
            },
            OCELType {
                name: "tool_root_review_proposed".to_string(),
                attributes: vec![
                    attr_def("path", "string"),
                    attr_def("recommendation", "string"),
                    attr_def("rationale", "string"),
                ],
            },
        ],
        object_types: vec![
            OCELType {
                name: "disk_audit".to_string(),
                attributes: vec![attr_def("created_at", "string")],
            },
            OCELType {
                name: "tool_root".to_string(),
                attributes: vec![
                    attr_def("path", "string"),
                    attr_def("category", "string"),
                    attr_def("bytes", "integer"),
                    attr_def("human", "string"),
                    attr_def("files", "integer"),
                    attr_def("dirs", "integer"),
                    attr_def("last_accessed_unix", "integer"),
                    attr_def("last_modified_unix", "integer"),
                    attr_def("metadata_changed_unix", "integer"),
                    attr_def("newest_descendant_modified_unix", "integer"),
                    attr_def("newest_descendant_path", "string"),
                    attr_def("recommendation", "string"),
                    attr_def("rationale", "string"),
                ],
            },
        ],
        events,
        objects,
    }
}

fn stable_object_id(prefix: &str, path: &str) -> String {
    let safe = path.replace(['/', ' ', ':', '.'], "_");

    format!("{}-{}", prefix, safe)
}

fn unix_to_datetime(t: Option<i64>) -> DateTime<FixedOffset> {
    let ts = t.unwrap_or_else(|| chrono::Utc::now().timestamp());
    chrono::DateTime::<chrono::Utc>::from_timestamp(ts, 0)
        .unwrap_or_else(chrono::Utc::now)
        .with_timezone(&chrono::FixedOffset::east_opt(0).unwrap())
}

fn value_to_ocel(val: serde_json::Value) -> OCELAttributeValue {
    match val {
        serde_json::Value::String(s) => OCELAttributeValue::String(s),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                OCELAttributeValue::Integer(i)
            } else if let Some(f) = n.as_f64() {
                OCELAttributeValue::Float(f)
            } else {
                OCELAttributeValue::Null
            }
        }
        serde_json::Value::Bool(b) => OCELAttributeValue::Boolean(b),
        serde_json::Value::Array(a) => {
            OCELAttributeValue::String(serde_json::to_string(&a).unwrap_or_default())
        }
        serde_json::Value::Object(o) => {
            OCELAttributeValue::String(serde_json::to_string(&o).unwrap_or_default())
        }
        _ => OCELAttributeValue::Null,
    }
}

fn attr(name: &str, value: serde_json::Value) -> OCELEventAttribute {
    OCELEventAttribute {
        name: name.to_string(),
        value: value_to_ocel(value),
    }
}

fn timed_attr(
    name: &str,
    time: &DateTime<FixedOffset>,
    value: serde_json::Value,
) -> OCELObjectAttribute {
    OCELObjectAttribute {
        name: name.to_string(),
        time: *time,
        value: value_to_ocel(value),
    }
}

fn attr_def(name: &str, value_type: &str) -> OCELTypeAttribute {
    OCELTypeAttribute {
        name: name.to_string(),
        value_type: value_type.to_string(),
    }
}

/// Builds an OCEL log structure for a full disk audit run.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use osx_clnr::domain::artifact::Candidate;
/// use osx_clnr::domain::tool_roots::ToolRootReport;
/// use osx_clnr::domain::audit::Stats;
/// use osx_clnr::domain::ocel::build_disk_audit_ocel;
///
/// // Positive case: build audit log with empty candidates and tool roots
/// let roots = vec![PathBuf::from("/Users/test")];
/// let candidates = vec![];
/// let tool_roots = vec![];
/// let stats = Stats::default();
/// let log = build_disk_audit_ocel(&roots, &candidates, &tool_roots, &stats);
///
/// assert_eq!(log.object_types.len(), 5);
/// assert!(log.events.iter().any(|e| e.event_type == "disk_audit_started"));
///
/// // Refusal case: no roots provided, no scan_root_started events are emitted
/// let roots: Vec<PathBuf> = vec![];
/// let log_no_roots = build_disk_audit_ocel(&roots, &candidates, &tool_roots, &stats);
/// assert!(!log_no_roots.events.iter().any(|e| e.event_type == "scan_root_started"));
/// ```
pub fn build_disk_audit_ocel(
    roots: &[std::path::PathBuf],
    candidates: &[crate::domain::artifact::Candidate],
    tool_roots: &[crate::domain::tool_roots::ToolRootReport],
    stats: &crate::domain::audit::Stats,
) -> OCEL {
    let now = chrono::Utc::now().with_timezone(&chrono::FixedOffset::east_opt(0).unwrap());
    let audit_obj_id = format!("audit-{}", chrono::Utc::now().timestamp());

    let mut objects = Vec::new();
    let mut events = Vec::new();

    let files_seen = stats.files_seen.load(std::sync::atomic::Ordering::Relaxed) as i64;
    let dirs_seen = stats.dirs_seen.load(std::sync::atomic::Ordering::Relaxed) as i64;
    let bytes_seen = stats.bytes_seen.load(std::sync::atomic::Ordering::Relaxed) as i64;
    let projects_seen = stats
        .projects_seen
        .load(std::sync::atomic::Ordering::Relaxed) as i64;
    let candidates_seen = stats
        .candidates_seen
        .load(std::sync::atomic::Ordering::Relaxed) as i64;
    let pruned_dirs = stats.pruned_dirs.load(std::sync::atomic::Ordering::Relaxed) as i64;
    let errors = stats.errors.load(std::sync::atomic::Ordering::Relaxed) as i64;

    // 1. Audit Object
    objects.push(OCELObject {
        id: audit_obj_id.clone(),
        object_type: "disk_audit".to_string(),
        attributes: vec![
            timed_attr("created_at", &now, serde_json::json!(now)),
            timed_attr("files_seen", &now, serde_json::json!(files_seen)),
            timed_attr("dirs_seen", &now, serde_json::json!(dirs_seen)),
            timed_attr("bytes_seen", &now, serde_json::json!(bytes_seen)),
            timed_attr("projects_seen", &now, serde_json::json!(projects_seen)),
            timed_attr("candidates_seen", &now, serde_json::json!(candidates_seen)),
            timed_attr("pruned_dirs", &now, serde_json::json!(pruned_dirs)),
            timed_attr("errors", &now, serde_json::json!(errors)),
        ],
        relationships: vec![],
    });

    // 2. Audit Started Event
    events.push(OCELEvent {
        id: format!("event-audit-started-{}", chrono::Utc::now().timestamp()),
        event_type: "disk_audit_started".to_string(),
        time: now,
        attributes: vec![attr("tool", serde_json::json!("osx-clnr"))],
        relationships: vec![OCELRelationship {
            object_id: audit_obj_id.clone(),
            qualifier: "audit-run".to_string(),
        }],
    });

    // 3. Scan Root Objects & Started Events
    for (idx, r) in roots.iter().enumerate() {
        let r_str = r.display().to_string();
        let root_obj_id = stable_object_id("scan-root", &r_str);

        objects.push(OCELObject {
            id: root_obj_id.clone(),
            object_type: "scan_root".to_string(),
            attributes: vec![timed_attr("path", &now, serde_json::json!(r_str))],
            relationships: vec![OCELRelationship {
                object_id: audit_obj_id.clone(),
                qualifier: "root-of-audit".to_string(),
            }],
        });

        events.push(OCELEvent {
            id: format!("event-scan-root-started-{}", idx),
            event_type: "scan_root_started".to_string(),
            time: now,
            attributes: vec![attr("path", serde_json::json!(r_str))],
            relationships: vec![
                OCELRelationship {
                    object_id: audit_obj_id.clone(),
                    qualifier: "audit-run".to_string(),
                },
                OCELRelationship {
                    object_id: root_obj_id.clone(),
                    qualifier: "started-root".to_string(),
                },
            ],
        });
    }

    // 4. Artifact Candidates & Filesystem Objects
    for (idx, c) in candidates.iter().enumerate() {
        let path_str = c.path.display().to_string();
        let fs_obj_id = stable_object_id("fs-obj", &path_str);
        let cand_obj_id = stable_object_id("candidate", &path_str);
        let kind = if c.path.is_file() {
            "file"
        } else {
            "directory"
        };

        objects.push(OCELObject {
            id: fs_obj_id.clone(),
            object_type: "filesystem_object".to_string(),
            attributes: vec![
                timed_attr("path", &now, serde_json::json!(path_str)),
                timed_attr("kind", &now, serde_json::json!(kind)),
            ],
            relationships: vec![OCELRelationship {
                object_id: audit_obj_id.clone(),
                qualifier: "observed-in-audit".to_string(),
            }],
        });

        objects.push(OCELObject {
            id: cand_obj_id.clone(),
            object_type: "artifact_candidate".to_string(),
            attributes: vec![
                timed_attr("path", &now, serde_json::json!(path_str)),
                timed_attr("reason", &now, serde_json::json!(c.reason)),
            ],
            relationships: vec![OCELRelationship {
                object_id: fs_obj_id.clone(),
                qualifier: "corresponds-to-fs-obj".to_string(),
            }],
        });

        events.push(OCELEvent {
            id: format!("event-fs-observed-{:06}", idx),
            event_type: "filesystem_object_observed".to_string(),
            time: now,
            attributes: vec![
                attr("path", serde_json::json!(path_str)),
                attr("kind", serde_json::json!(kind)),
            ],
            relationships: vec![
                OCELRelationship {
                    object_id: audit_obj_id.clone(),
                    qualifier: "audit-run".to_string(),
                },
                OCELRelationship {
                    object_id: fs_obj_id.clone(),
                    qualifier: "observed-object".to_string(),
                },
            ],
        });

        events.push(OCELEvent {
            id: format!("event-candidate-proposed-{:06}", idx),
            event_type: "artifact_candidate_proposed".to_string(),
            time: now,
            attributes: vec![
                attr("path", serde_json::json!(path_str)),
                attr("reason", serde_json::json!(c.reason)),
            ],
            relationships: vec![
                OCELRelationship {
                    object_id: audit_obj_id.clone(),
                    qualifier: "audit-run".to_string(),
                },
                OCELRelationship {
                    object_id: cand_obj_id.clone(),
                    qualifier: "proposed-candidate".to_string(),
                },
                OCELRelationship {
                    object_id: fs_obj_id.clone(),
                    qualifier: "targets-fs-object".to_string(),
                },
            ],
        });
    }

    // 5. Tool Roots
    for (idx, tr) in tool_roots.iter().enumerate() {
        let tr_obj_id = stable_object_id("tool-root", &tr.path);

        objects.push(OCELObject {
            id: tr_obj_id.clone(),
            object_type: "tool_root".to_string(),
            attributes: vec![
                timed_attr("path", &now, serde_json::json!(tr.path)),
                timed_attr("category", &now, serde_json::json!(tr.category)),
                timed_attr("bytes", &now, serde_json::json!(tr.bytes)),
                timed_attr("human", &now, serde_json::json!(tr.human)),
                timed_attr("files", &now, serde_json::json!(tr.files)),
                timed_attr("dirs", &now, serde_json::json!(tr.dirs)),
            ],
            relationships: vec![OCELRelationship {
                object_id: audit_obj_id.clone(),
                qualifier: "tool-root-of-audit".to_string(),
            }],
        });

        events.push(OCELEvent {
            id: format!("event-tool-root-observed-{:06}", idx),
            event_type: "tool_root_observed".to_string(),
            time: now,
            attributes: vec![
                attr("path", serde_json::json!(tr.path)),
                attr("category", serde_json::json!(tr.category)),
                attr("bytes", serde_json::json!(tr.bytes)),
            ],
            relationships: vec![
                OCELRelationship {
                    object_id: audit_obj_id.clone(),
                    qualifier: "audit-run".to_string(),
                },
                OCELRelationship {
                    object_id: tr_obj_id.clone(),
                    qualifier: "observed-tool-root".to_string(),
                },
            ],
        });
    }

    OCEL {
        event_types: vec![
            OCELType {
                name: "disk_audit_started".to_string(),
                attributes: vec![attr_def("tool", "string")],
            },
            OCELType {
                name: "scan_root_started".to_string(),
                attributes: vec![attr_def("path", "string")],
            },
            OCELType {
                name: "filesystem_object_observed".to_string(),
                attributes: vec![attr_def("path", "string"), attr_def("kind", "string")],
            },
            OCELType {
                name: "artifact_candidate_proposed".to_string(),
                attributes: vec![attr_def("path", "string"), attr_def("reason", "string")],
            },
            OCELType {
                name: "tool_root_observed".to_string(),
                attributes: vec![
                    attr_def("path", "string"),
                    attr_def("category", "string"),
                    attr_def("bytes", "integer"),
                ],
            },
        ],
        object_types: vec![
            OCELType {
                name: "disk_audit".to_string(),
                attributes: vec![
                    attr_def("created_at", "string"),
                    attr_def("files_seen", "integer"),
                    attr_def("dirs_seen", "integer"),
                    attr_def("bytes_seen", "integer"),
                    attr_def("projects_seen", "integer"),
                    attr_def("candidates_seen", "integer"),
                    attr_def("pruned_dirs", "integer"),
                    attr_def("errors", "integer"),
                ],
            },
            OCELType {
                name: "scan_root".to_string(),
                attributes: vec![attr_def("path", "string")],
            },
            OCELType {
                name: "filesystem_object".to_string(),
                attributes: vec![attr_def("path", "string"), attr_def("kind", "string")],
            },
            OCELType {
                name: "artifact_candidate".to_string(),
                attributes: vec![attr_def("path", "string"), attr_def("reason", "string")],
            },
            OCELType {
                name: "tool_root".to_string(),
                attributes: vec![
                    attr_def("path", "string"),
                    attr_def("category", "string"),
                    attr_def("bytes", "integer"),
                    attr_def("human", "string"),
                    attr_def("files", "integer"),
                    attr_def("dirs", "integer"),
                ],
            },
        ],
        events,
        objects,
    }
}

/// Builds an OCEL log structure for a snapshot audit.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::ocel::build_snapshot_audit_ocel;
///
/// let log = build_snapshot_audit_ocel("/", &["snap1".to_string(), "snap2".to_string()]);
/// assert_eq!(log.objects.len(), 1);
/// assert_eq!(log.objects[0].object_type, "snapshot_state");
/// assert_eq!(log.events[0].event_type, "snapshot_state_observed");
/// ```
pub fn build_snapshot_audit_ocel(volume: &str, snapshots: &[String]) -> OCEL {
    let now = chrono::Utc::now().with_timezone(&chrono::FixedOffset::east_opt(0).unwrap());
    let state_obj_id = format!("snapshot-state-{}", chrono::Utc::now().timestamp());

    let mut objects = Vec::new();
    let mut events = Vec::new();

    objects.push(OCELObject {
        id: state_obj_id.clone(),
        object_type: "snapshot_state".to_string(),
        attributes: vec![
            timed_attr("volume", &now, serde_json::json!(volume)),
            timed_attr(
                "snapshot_count",
                &now,
                serde_json::json!(snapshots.len() as i64),
            ),
            timed_attr("snapshots", &now, serde_json::json!(snapshots)),
        ],
        relationships: vec![],
    });

    events.push(OCELEvent {
        id: format!("event-snapshot-observed-{}", chrono::Utc::now().timestamp()),
        event_type: "snapshot_state_observed".to_string(),
        time: now,
        attributes: vec![
            attr("volume", serde_json::json!(volume)),
            attr("snapshot_count", serde_json::json!(snapshots.len() as i64)),
        ],
        relationships: vec![OCELRelationship {
            object_id: state_obj_id,
            qualifier: "observed-state".to_string(),
        }],
    });

    OCEL {
        event_types: vec![OCELType {
            name: "snapshot_state_observed".to_string(),
            attributes: vec![
                attr_def("volume", "string"),
                attr_def("snapshot_count", "integer"),
            ],
        }],
        object_types: vec![OCELType {
            name: "snapshot_state".to_string(),
            attributes: vec![
                attr_def("volume", "string"),
                attr_def("snapshot_count", "integer"),
                attr_def("snapshots", "array"),
            ],
        }],
        events,
        objects,
    }
}

/// Builds an OCEL log structure for a snapshot thinning.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::ocel::build_snapshot_thin_ocel;
///
/// let log = build_snapshot_thin_ocel("/", 1000, &["snap1".to_string()], &[], &["snap1".to_string()]);
/// assert_eq!(log.objects.len(), 1);
/// assert_eq!(log.objects[0].object_type, "snapshot_state");
/// assert_eq!(log.events[0].event_type, "snapshot_thin_requested");
/// ```
pub fn build_snapshot_thin_ocel(
    volume: &str,
    requested_bytes: u64,
    before: &[String],
    after: &[String],
    thinned: &[String],
) -> OCEL {
    let now = chrono::Utc::now().with_timezone(&chrono::FixedOffset::east_opt(0).unwrap());
    let state_obj_id = format!("snapshot-state-{}", chrono::Utc::now().timestamp());

    let mut objects = Vec::new();
    let mut events = Vec::new();

    objects.push(OCELObject {
        id: state_obj_id.clone(),
        object_type: "snapshot_state".to_string(),
        attributes: vec![
            timed_attr("volume", &now, serde_json::json!(volume)),
            timed_attr(
                "snapshot_count",
                &now,
                serde_json::json!(after.len() as i64),
            ),
            timed_attr("snapshots", &now, serde_json::json!(after)),
        ],
        relationships: vec![],
    });

    events.push(OCELEvent {
        id: format!("event-snapshot-thin-{}", chrono::Utc::now().timestamp()),
        event_type: "snapshot_thin_requested".to_string(),
        time: now,
        attributes: vec![
            attr("volume", serde_json::json!(volume)),
            attr("requested_bytes", serde_json::json!(requested_bytes as i64)),
            attr(
                "snapshots_before_count",
                serde_json::json!(before.len() as i64),
            ),
            attr(
                "snapshots_after_count",
                serde_json::json!(after.len() as i64),
            ),
            attr("thinned_count", serde_json::json!(thinned.len() as i64)),
        ],
        relationships: vec![OCELRelationship {
            object_id: state_obj_id,
            qualifier: "resulting-state".to_string(),
        }],
    });

    OCEL {
        event_types: vec![OCELType {
            name: "snapshot_thin_requested".to_string(),
            attributes: vec![
                attr_def("volume", "string"),
                attr_def("requested_bytes", "integer"),
                attr_def("snapshots_before_count", "integer"),
                attr_def("snapshots_after_count", "integer"),
                attr_def("thinned_count", "integer"),
            ],
        }],
        object_types: vec![OCELType {
            name: "snapshot_state".to_string(),
            attributes: vec![
                attr_def("volume", "string"),
                attr_def("snapshot_count", "integer"),
                attr_def("snapshots", "array"),
            ],
        }],
        events,
        objects,
    }
}

/// Builds the OCEL log for a *selective* snapshot deletion (`snapshot delete`).
///
/// Distinct from [`build_snapshot_thin_ocel`] on purpose: thinning is byte-driven
/// (macOS chooses what to drop to hit a target); selective deletion names exactly
/// which snapshots to remove. Emitting the same `snapshot_thin_requested` event
/// for both would make the event log lie about which operation actually ran — a
/// model-vs-log mismatch. This emits `snapshot_delete_requested` so process
/// mining can tell the two apart.
pub fn build_snapshot_delete_ocel(
    volume: &str,
    before: &[String],
    after: &[String],
    deleted: &[String],
) -> OCEL {
    let now = chrono::Utc::now().with_timezone(&chrono::FixedOffset::east_opt(0).unwrap());
    let state_obj_id = format!("snapshot-state-{}", chrono::Utc::now().timestamp());

    let objects = vec![OCELObject {
        id: state_obj_id.clone(),
        object_type: "snapshot_state".to_string(),
        attributes: vec![
            timed_attr("volume", &now, serde_json::json!(volume)),
            timed_attr(
                "snapshot_count",
                &now,
                serde_json::json!(after.len() as i64),
            ),
            timed_attr("snapshots", &now, serde_json::json!(after)),
        ],
        relationships: vec![],
    }];

    let events = vec![OCELEvent {
        id: format!("event-snapshot-delete-{}", chrono::Utc::now().timestamp()),
        event_type: "snapshot_delete_requested".to_string(),
        time: now,
        attributes: vec![
            attr("volume", serde_json::json!(volume)),
            attr(
                "snapshots_before_count",
                serde_json::json!(before.len() as i64),
            ),
            attr(
                "snapshots_after_count",
                serde_json::json!(after.len() as i64),
            ),
            attr("deleted_count", serde_json::json!(deleted.len() as i64)),
        ],
        relationships: vec![OCELRelationship {
            object_id: state_obj_id,
            qualifier: "resulting-state".to_string(),
        }],
    }];

    OCEL {
        event_types: vec![OCELType {
            name: "snapshot_delete_requested".to_string(),
            attributes: vec![
                attr_def("volume", "string"),
                attr_def("snapshots_before_count", "integer"),
                attr_def("snapshots_after_count", "integer"),
                attr_def("deleted_count", "integer"),
            ],
        }],
        object_types: vec![OCELType {
            name: "snapshot_state".to_string(),
            attributes: vec![
                attr_def("volume", "string"),
                attr_def("snapshot_count", "integer"),
                attr_def("snapshots", "array"),
            ],
        }],
        events,
        objects,
    }
}

/// Builds an OCEL log structure for a Time Machine exclusion plan.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::ocel::build_exclusion_plan_ocel;
///
/// let log = build_exclusion_plan_ocel("/path/to/script.sh", 5);
/// assert_eq!(log.objects.len(), 1);
/// assert_eq!(log.objects[0].object_type, "tm_exclusion_plan");
/// assert_eq!(log.events[0].event_type, "tm_exclusion_plan_written");
/// ```
pub fn build_exclusion_plan_ocel(script_path: &str, candidate_count: usize) -> OCEL {
    let now = chrono::Utc::now().with_timezone(&chrono::FixedOffset::east_opt(0).unwrap());
    let plan_obj_id = format!("tm-exclusion-plan-{}", chrono::Utc::now().timestamp());

    let mut objects = Vec::new();
    let mut events = Vec::new();

    objects.push(OCELObject {
        id: plan_obj_id.clone(),
        object_type: "tm_exclusion_plan".to_string(),
        attributes: vec![
            timed_attr("script_path", &now, serde_json::json!(script_path)),
            timed_attr(
                "candidate_count",
                &now,
                serde_json::json!(candidate_count as i64),
            ),
        ],
        relationships: vec![],
    });

    events.push(OCELEvent {
        id: format!("event-exclusion-written-{}", chrono::Utc::now().timestamp()),
        event_type: "tm_exclusion_plan_written".to_string(),
        time: now,
        attributes: vec![
            attr("script_path", serde_json::json!(script_path)),
            attr("candidate_count", serde_json::json!(candidate_count as i64)),
        ],
        relationships: vec![OCELRelationship {
            object_id: plan_obj_id,
            qualifier: "written-plan".to_string(),
        }],
    });

    OCEL {
        event_types: vec![OCELType {
            name: "tm_exclusion_plan_written".to_string(),
            attributes: vec![
                attr_def("script_path", "string"),
                attr_def("candidate_count", "integer"),
            ],
        }],
        object_types: vec![OCELType {
            name: "tm_exclusion_plan".to_string(),
            attributes: vec![
                attr_def("script_path", "string"),
                attr_def("candidate_count", "integer"),
            ],
        }],
        events,
        objects,
    }
}

// OcelValidationReport replaced by wasm4pm_compat::admission::Admit trait.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditSummaryStats {
    pub created_at: String,
    pub files_seen: i64,
    pub dirs_seen: i64,
    pub bytes_seen: i64,
    pub projects_seen: i64,
    pub candidates_seen: i64,
    pub pruned_dirs: i64,
    pub errors: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcelSummary {
    pub total_events: usize,
    pub total_objects: usize,
    pub event_counts: std::collections::HashMap<String, usize>,
    pub object_counts: std::collections::HashMap<String, usize>,
    pub audit_stats: Vec<AuditSummaryStats>,
}

fn val_matches_type(val: &OCELAttributeValue, type_str: &str) -> bool {
    match type_str {
        "string" => matches!(val, OCELAttributeValue::String(_)),
        "integer" => matches!(val, OCELAttributeValue::Integer(_)),
        "float" | "number" => matches!(
            val,
            OCELAttributeValue::Float(_) | OCELAttributeValue::Integer(_)
        ),
        "boolean" => matches!(val, OCELAttributeValue::Boolean(_)),
        "array" => matches!(val, OCELAttributeValue::String(_)),
        _ => true,
    }
}

/// Validates the structure and referential integrity of an OCEL log.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::ocel::{build_tool_roots_ocel, OcelLogAdjudicator};
/// use wasm4pm_compat::admission::Admit;
/// use wasm4pm_compat::evidence::Evidence;
///
/// // Positive case: validation succeeds for standard empty audit log
/// let log = build_tool_roots_ocel(&[]);
/// let report = OcelLogAdjudicator::admit(Evidence::raw(log));
/// assert!(report.is_ok());
///
/// // Refusal/Negative case: validation fails if an event type is undefined
/// let mut bad_log = build_tool_roots_ocel(&[]);
/// bad_log.events[0].event_type = "invalid_event_type".to_string();
/// let report = OcelLogAdjudicator::admit(Evidence::raw(bad_log));
/// assert!(report.is_err());
/// ```
use wasm4pm_compat::admission::{Admission, Admit, Refusal};
use wasm4pm_compat::evidence::Evidence;
use wasm4pm_compat::state::Raw;
use wasm4pm_compat::witness::Ocel20;

pub struct OcelLogAdjudicator;

impl Admit for OcelLogAdjudicator {
    type Raw = OCEL;
    type Admitted = OCEL;
    type Reason = String;
    type Witness = Ocel20;

    fn admit(
        raw: Evidence<Self::Raw, Raw, Self::Witness>,
    ) -> Result<Admission<Self::Admitted, Self::Witness>, Refusal<Self::Reason, Self::Witness>>
    {
        let log = &raw.value;
        let mut errors = Vec::new();

        let event_schema: std::collections::HashMap<&str, &OCELType> = log
            .event_types
            .iter()
            .map(|t| (t.name.as_str(), t))
            .collect();
        let object_schema: std::collections::HashMap<&str, &OCELType> = log
            .object_types
            .iter()
            .map(|t| (t.name.as_str(), t))
            .collect();

        let mut object_map = std::collections::HashMap::new();
        for obj in &log.objects {
            object_map.insert(obj.id.as_str(), obj.object_type.as_str());
        }

        // Validate events
        for event in &log.events {
            let schema = match event_schema.get(event.event_type.as_str()) {
                Some(s) => s,
                None => {
                    errors.push(format!(
                        "Event '{}' has type '{}' which is not defined in eventTypes schema",
                        event.id, event.event_type
                    ));
                    continue;
                }
            };

            for attr in &event.attributes {
                let attr_def = schema.attributes.iter().find(|a| a.name == attr.name);
                match attr_def {
                    None => {
                        errors.push(format!(
                        "Event '{}' has attribute '{}' not defined in schema for event type '{}'",
                        event.id, attr.name, event.event_type
                    ));
                    }
                    Some(def) => {
                        if !val_matches_type(&attr.value, &def.value_type) {
                            errors.push(format!(
                            "Event '{}' attribute '{}' has value '{:?}' which does not match defined type '{}'",
                            event.id, attr.name, attr.value, def.value_type
                        ));
                        }
                    }
                }
            }

            for rel in &event.relationships {
                if !object_map.contains_key(rel.object_id.as_str()) {
                    errors.push(format!(
                        "Event '{}' has relationship pointing to non-existent object '{}'",
                        event.id, rel.object_id
                    ));
                }
            }

            // Delete event relationship checks
            let is_delete_event = matches!(
                event.event_type.as_str(),
                "artifact_deleted"
                    | "artifact_delete_skipped"
                    | "artifact_delete_refused"
                    | "artifact_delete_failed"
            );
            if is_delete_event {
                let mut has_receipt = false;
                let mut has_plan = false;
                let mut has_candidate = false;
                let mut has_fs_obj = false;

                for rel in &event.relationships {
                    if let Some(&obj_type) = object_map.get(rel.object_id.as_str()) {
                        match obj_type {
                            "delete_receipt" => has_receipt = true,
                            "deletion_plan" => has_plan = true,
                            "artifact_candidate" => has_candidate = true,
                            "filesystem_object" => has_fs_obj = true,
                            _ => {}
                        }
                    }
                }

                if !has_receipt {
                    errors.push(format!(
                        "Delete event '{}' lacks relationship to 'delete_receipt' object",
                        event.id
                    ));
                }
                if !has_plan {
                    errors.push(format!(
                        "Delete event '{}' lacks relationship to 'deletion_plan' object",
                        event.id
                    ));
                }
                if !has_candidate {
                    errors.push(format!(
                        "Delete event '{}' lacks relationship to 'artifact_candidate' object",
                        event.id
                    ));
                }
                if !has_fs_obj {
                    errors.push(format!(
                        "Delete event '{}' lacks relationship to 'filesystem_object' object",
                        event.id
                    ));
                }
            }

            // Candidate event checks
            if event.event_type == "artifact_candidate_proposed" {
                let mut has_audit = false;
                let mut has_root = false;
                let mut has_fs_obj = false;

                for rel in &event.relationships {
                    if let Some(&obj_type) = object_map.get(rel.object_id.as_str()) {
                        match obj_type {
                            "disk_audit" => has_audit = true,
                            "scan_root" => has_root = true,
                            "filesystem_object" => has_fs_obj = true,
                            _ => {}
                        }
                    }
                }

                if !has_audit {
                    errors.push(format!(
                        "Candidate proposed event '{}' lacks relationship to 'disk_audit' object",
                        event.id
                    ));
                }
                if !has_root {
                    errors.push(format!(
                        "Candidate proposed event '{}' lacks relationship to 'scan_root' object",
                        event.id
                    ));
                }
                if !has_fs_obj {
                    errors.push(format!(
                    "Candidate proposed event '{}' lacks relationship to 'filesystem_object' object",
                    event.id
                ));
                }
            }

            // Tool root review event checks
            if event.event_type == "tool_root_review_proposed" {
                let mut has_audit = false;
                let mut has_tool_root = false;

                for rel in &event.relationships {
                    if let Some(&obj_type) = object_map.get(rel.object_id.as_str()) {
                        match obj_type {
                            "disk_audit" => has_audit = true,
                            "tool_root" => has_tool_root = true,
                            _ => {}
                        }
                    }
                }

                if !has_audit {
                    errors.push(format!(
                    "Tool root review proposed event '{}' lacks relationship to 'disk_audit' object",
                    event.id
                ));
                }
                if !has_tool_root {
                    errors.push(format!(
                    "Tool root review proposed event '{}' lacks relationship to 'tool_root' object",
                    event.id
                ));
                }
            }
        }

        // Validate objects
        for obj in &log.objects {
            let schema = match object_schema.get(obj.object_type.as_str()) {
                Some(s) => s,
                None => {
                    errors.push(format!(
                        "Object '{}' has type '{}' which is not defined in objectTypes schema",
                        obj.id, obj.object_type
                    ));
                    continue;
                }
            };

            for attr in &obj.attributes {
                let attr_def = schema.attributes.iter().find(|a| a.name == attr.name);
                match attr_def {
                    None => {
                        errors.push(format!(
                        "Object '{}' has attribute '{}' not defined in schema for object type '{}'",
                        obj.id, attr.name, obj.object_type
                    ));
                    }
                    Some(def) => {
                        if !val_matches_type(&attr.value, &def.value_type) {
                            errors.push(format!(
                            "Object '{}' attribute '{}' has value '{:?}' which does not match defined type '{}'",
                            obj.id, attr.name, attr.value, def.value_type
                        ));
                        }
                    }
                }
            }

            for rel in &obj.relationships {
                if !object_map.contains_key(rel.object_id.as_str()) {
                    errors.push(format!(
                        "Object '{}' has relationship pointing to non-existent object '{}'",
                        obj.id, rel.object_id
                    ));
                }
            }
        }

        if errors.is_empty() {
            Ok(Admission::new(raw.value))
        } else {
            Err(Refusal::new(errors.join(", ")))
        }
    }
}

/// Summarizes events and objects in the OCEL log, extracting statistics if available.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::ocel::{build_tool_roots_ocel, summarize_ocel_log};
///
/// // Positive case: summary extracts correct counts from standard empty log
/// let log = build_tool_roots_ocel(&[]);
/// let summary = summarize_ocel_log(&log);
/// assert_eq!(summary.total_events, 1);
/// assert_eq!(summary.total_objects, 1);
/// assert_eq!(summary.event_counts.get("disk_audit_started"), Some(&1));
/// assert_eq!(summary.object_counts.get("disk_audit"), Some(&1));
/// ```
pub fn summarize_ocel_log(log: &OCEL) -> OcelSummary {
    let mut event_counts = std::collections::HashMap::new();
    for e in &log.events {
        *event_counts.entry(e.event_type.clone()).or_insert(0) += 1;
    }

    let mut object_counts = std::collections::HashMap::new();
    for o in &log.objects {
        *object_counts.entry(o.object_type.clone()).or_insert(0) += 1;
    }

    let mut audit_stats = Vec::new();
    for o in &log.objects {
        if o.object_type == "disk_audit" {
            let mut created_at = String::new();
            let mut files_seen = 0;
            let mut dirs_seen = 0;
            let mut bytes_seen = 0;
            let mut projects_seen = 0;
            let mut candidates_seen = 0;
            let mut pruned_dirs = 0;
            let mut errors = 0;

            for attr in &o.attributes {
                match attr.name.as_str() {
                    "created_at" => {
                        if let OCELAttributeValue::String(s) = &attr.value {
                            created_at = s.to_string();
                        }
                    }
                    "files_seen" => {
                        files_seen = match attr.value {
                            OCELAttributeValue::Integer(i) => i,
                            _ => 0,
                        };
                    }
                    "dirs_seen" => {
                        dirs_seen = match attr.value {
                            OCELAttributeValue::Integer(i) => i,
                            _ => 0,
                        };
                    }
                    "bytes_seen" => {
                        bytes_seen = match attr.value {
                            OCELAttributeValue::Integer(i) => i,
                            _ => 0,
                        };
                    }
                    "projects_seen" => {
                        projects_seen = match attr.value {
                            OCELAttributeValue::Integer(i) => i,
                            _ => 0,
                        };
                    }
                    "candidates_seen" => {
                        candidates_seen = match attr.value {
                            OCELAttributeValue::Integer(i) => i,
                            _ => 0,
                        };
                    }
                    "pruned_dirs" => {
                        pruned_dirs = match attr.value {
                            OCELAttributeValue::Integer(i) => i,
                            _ => 0,
                        };
                    }
                    "errors" => {
                        errors = match attr.value {
                            OCELAttributeValue::Integer(i) => i,
                            _ => 0,
                        };
                    }
                    _ => {}
                }
            }

            audit_stats.push(AuditSummaryStats {
                created_at,
                files_seen,
                dirs_seen,
                bytes_seen,
                projects_seen,
                candidates_seen,
                pruned_dirs,
                errors,
            });
        }
    }

    OcelSummary {
        total_events: log.events.len(),
        total_objects: log.objects.len(),
        event_counts,
        object_counts,
        audit_stats,
    }
}
