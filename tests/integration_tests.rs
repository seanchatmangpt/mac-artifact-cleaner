use dashmap::DashMap;
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use mac_artifact_cleaner::domain::artifact::{
    artifact_candidates_from_snapshot, detect_project_from_snapshot, ArgsSnapshot, Candidate,
};
use mac_artifact_cleaner::domain::audit::Stats;
use mac_artifact_cleaner::domain::delete::{validate_plan, validate_plan_item};
use mac_artifact_cleaner::domain::plan::{DeletionPlan, PlanItem, PlanItemKind};
use mac_artifact_cleaner::domain::receipt::{DeletionReceipt, DeletionStatus};
use mac_artifact_cleaner::integration::fs::scan_root;
use mac_artifact_cleaner::integration::fs::{delete_dir_all, delete_file, read_dir_snapshot};

#[test]
fn test_project_detection_and_candidates() {
    let tmp = tempfile::Builder::new().tempdir_in(".").unwrap();
    let project_dir = tmp.path().join("my-rust-project");
    fs::create_dir(&project_dir).unwrap();

    // Create rust project marker
    File::create(project_dir.join("Cargo.toml")).unwrap();
    // Create rebuildable build artifact folder
    let target_dir = project_dir.join("target");
    fs::create_dir(&target_dir).unwrap();

    // Integration layer builds the inert snapshot; domain receives only the DTO.
    let snap = read_dir_snapshot(&project_dir);
    let kind = detect_project_from_snapshot(&snap);
    assert!(kind.is_some());
    assert_eq!(kind.unwrap().names[0], "rust");

    let args = ArgsSnapshot {
        deps: true,
        aggressive: true,
        verbose: false,
        tool_roots: false,
    };
    let snap2 = read_dir_snapshot(&project_dir);
    let project = detect_project_from_snapshot(&snap2).unwrap();
    let candidates = artifact_candidates_from_snapshot(&project_dir, &project, &args, &snap2);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].path, target_dir);
}

#[test]
fn test_plan_bound_deletion_validation() {
    let plan = DeletionPlan::new(
        vec![PathBuf::from("/Users/user")],
        false,
        true,
        vec![PlanItem {
            path: PathBuf::from("/Users/user/dev/project/target"),
            kind: PlanItemKind::Dir,
            reason: "rust target".to_string(),
        }],
        vec![],
    );

    // Should validate the target path successfully
    assert!(validate_plan_item(
        Path::new("/Users/user/dev/project/target"),
        &plan
    ));

    // Should reject random files not present in the plan
    assert!(!validate_plan_item(
        Path::new("/Users/user/dev/project/src/main.rs"),
        &plan
    ));

    // Should refuse system folders even if manually injected into the plan
    let mut bad_plan = plan.clone();
    bad_plan.items.push(PlanItem {
        path: PathBuf::from("/System"),
        kind: PlanItemKind::Dir,
        reason: "fake target".to_string(),
    });
    assert!(validate_plan(&bad_plan).is_err());
}

#[test]
fn test_end_to_end_artifact_scan_build_delete() {
    let tmp = tempfile::Builder::new().tempdir_in(".").unwrap();
    let root = tmp.path();

    // 1. Set up mock project structure
    // Mock rust project
    let rust_proj = root.join("rust-proj");
    fs::create_dir(&rust_proj).unwrap();
    File::create(rust_proj.join("Cargo.toml")).unwrap();
    let rust_target = rust_proj.join("target");
    fs::create_dir(&rust_target).unwrap();
    File::create(rust_target.join("output.bin")).unwrap();

    // Mock python project
    let py_proj = root.join("py-proj");
    fs::create_dir(&py_proj).unwrap();
    File::create(py_proj.join("requirements.txt")).unwrap();
    let py_venv = py_proj.join("venv");
    fs::create_dir(&py_venv).unwrap();
    File::create(py_venv.join("pip")).unwrap();

    // 2. Perform file traversal scanning
    let args = ArgsSnapshot {
        deps: true,
        aggressive: true,
        verbose: true,
        tool_roots: false,
    };
    let candidates = Arc::new(Mutex::new(BTreeSet::<Candidate>::new()));
    let stats = Arc::new(Stats::default());
    let tool_accs = Arc::new(DashMap::new());

    scan_root(
        root,
        &args,
        candidates.clone(),
        stats.clone(),
        &[],
        tool_accs.clone(),
    )
    .unwrap();

    let cand_list: Vec<Candidate> = candidates.lock().unwrap().iter().cloned().collect();
    assert_eq!(cand_list.len(), 2);

    let path_list: Vec<PathBuf> = cand_list.iter().map(|c| c.path.clone()).collect();
    assert!(path_list.contains(&rust_target));
    assert!(path_list.contains(&py_venv));

    // 3. Build deletion plan
    let mut plan_items = Vec::new();
    for c in &cand_list {
        let kind = if c.path.is_file() {
            PlanItemKind::File
        } else {
            PlanItemKind::Dir
        };
        plan_items.push(PlanItem {
            path: c.path.clone(),
            kind,
            reason: c.reason.clone(),
        });
    }

    let plan = DeletionPlan::new(vec![root.to_path_buf()], true, true, plan_items, vec![]);
    assert!(validate_plan(&plan).is_ok());

    // 4. Execute deletion strictly matching domain deletion rules
    let start_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut results = Vec::new();
    for item in &plan.items {
        assert!(validate_plan_item(&item.path, &plan));

        if !item.path.exists() {
            results.push(mac_artifact_cleaner::domain::receipt::DeletionResult {
                path: item.path.clone(),
                status: DeletionStatus::SkippedMissing,
                error: None,
            });
            continue;
        }

        // Delegate all filesystem mutations to the integration layer.
        let res = match item.kind {
            PlanItemKind::File => match delete_file(&item.path) {
                Ok(()) => mac_artifact_cleaner::domain::receipt::DeletionResult {
                    path: item.path.clone(),
                    status: DeletionStatus::Deleted,
                    error: None,
                },
                Err(e) => mac_artifact_cleaner::domain::receipt::DeletionResult {
                    path: item.path.clone(),
                    status: DeletionStatus::Failed,
                    error: Some(e.to_string()),
                },
            },
            PlanItemKind::Dir => match delete_dir_all(&item.path) {
                Ok(()) => mac_artifact_cleaner::domain::receipt::DeletionResult {
                    path: item.path.clone(),
                    status: DeletionStatus::Deleted,
                    error: None,
                },
                Err(e) => mac_artifact_cleaner::domain::receipt::DeletionResult {
                    path: item.path.clone(),
                    status: DeletionStatus::Failed,
                    error: Some(e.to_string()),
                },
            },
        };
        results.push(res);
    }

    let end_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let receipt = DeletionReceipt::new(plan.created_unix, start_time, end_time, results);
    assert_eq!(receipt.results.len(), 2);
    assert!(receipt
        .results
        .iter()
        .all(|r| r.status == DeletionStatus::Deleted));

    // Verify files were actually deleted from filesystem
    assert!(!rust_target.exists());
    assert!(!py_venv.exists());
}

#[test]
fn test_snapshot_query_thin_parsing() {
    use mac_artifact_cleaner::domain::time::{
        identify_thinned_snapshots, parse_snapshot_date, SnapshotThinReceipt,
    };

    // Test parse_snapshot_date
    assert_eq!(
        parse_snapshot_date("com.apple.TimeMachine.2026-05-26-135630.local"),
        Some("2026-05-26-135630".to_string())
    );
    assert_eq!(parse_snapshot_date("invalid-snapshot-name"), None);

    // Test identify_thinned_snapshots
    let before = vec!["snap1".to_string(), "snap2".to_string()];
    let after = vec!["snap2".to_string()];
    let thinned = identify_thinned_snapshots(&before, &after);
    assert_eq!(thinned, vec!["snap1".to_string()]);

    let thinned_none = identify_thinned_snapshots(&before, &before);
    assert!(thinned_none.is_empty());

    // Test SnapshotThinReceipt
    let receipt = SnapshotThinReceipt::new("/".to_string(), 1000000, 1716768000, before, after);
    assert_eq!(receipt.snapshots_thinned, vec!["snap1".to_string()]);
}

#[test]
fn test_size_parsing() {
    use mac_artifact_cleaner::domain::time::parse_size_in_bytes;

    assert_eq!(parse_size_in_bytes("10GB").unwrap(), 10_000_000_000);
    assert_eq!(parse_size_in_bytes("500mb").unwrap(), 500_000_000);
    assert_eq!(parse_size_in_bytes("2048").unwrap(), 2048);
    assert_eq!(parse_size_in_bytes("2.5 GB").unwrap(), 2_500_000_000);
    assert_eq!(parse_size_in_bytes("100b").unwrap(), 100);

    assert!(parse_size_in_bytes("invalid_size").is_err());
    assert!(parse_size_in_bytes("").is_err());
    assert!(parse_size_in_bytes("abcGB").is_err());
}

#[test]
fn test_exclusions_plan_writing() {
    use mac_artifact_cleaner::integration::tmutil::write_tm_exclusions_script;

    let tmp = tempfile::Builder::new().tempdir_in(".").unwrap();
    let script_path = tmp.path().join("tm-exclusions.sh");

    // Create a mock directory for candidate
    let mock_dir = tmp.path().join("mock-npm-cache");
    fs::create_dir(&mock_dir).unwrap();

    let candidates = vec![
        Candidate {
            path: mock_dir.clone(),
            reason: "Mock npm cache".to_string(),
        },
        Candidate {
            path: tmp.path().join("nonexistent-dir"),
            reason: "Nonexistent".to_string(),
        },
    ];

    write_tm_exclusions_script(&script_path, &candidates).unwrap();

    assert!(script_path.exists());
    let content = fs::read_to_string(&script_path).unwrap();
    assert!(content.contains("tmutil addexclusion"));
    assert!(content.contains("mock-npm-cache"));
    // Since nonexistent-dir doesn't exist on disk, it shouldn't be added to script
    assert!(!content.contains("nonexistent-dir"));
}

#[test]
fn test_tool_root_recommendation_logic() {
    use mac_artifact_cleaner::domain::tool_roots::{recommend_tool_root, ToolRootDef};

    let npm_def = ToolRootDef {
        path: PathBuf::from("/Users/user/.npm"),
        category: "node_package_cache",
        default_disposition: "cleanup_candidate",
    };

    let docker_def = ToolRootDef {
        path: PathBuf::from("/Users/user/.docker"),
        category: "container_state",
        default_disposition: "review_with_tool",
    };

    let kube_def = ToolRootDef {
        path: PathBuf::from("/Users/user/.kube"),
        category: "kubernetes_config",
        default_disposition: "keep",
    };

    // Stale and very large npm cache -> cleanup_candidate
    let (rec_npm, _) = recommend_tool_root(
        &npm_def,
        50 * 1024 * 1024 * 1024,
        Some(100),
        Some(100),
        Some(100),
    );
    assert_eq!(rec_npm, "cleanup_candidate");

    // Fresh and small npm cache -> low_priority
    let (rec_npm_fresh, _) = recommend_tool_root(&npm_def, 1024, Some(0), Some(0), Some(0));
    assert_eq!(rec_npm_fresh, "low_priority");

    // Stale docker container state -> review_with_tool
    let (rec_doc, _) = recommend_tool_root(
        &docker_def,
        15 * 1024 * 1024 * 1024,
        Some(90),
        Some(90),
        Some(90),
    );
    assert_eq!(rec_doc, "review_with_tool");

    // Kubernetes config -> always keep
    let (rec_kube, _) = recommend_tool_root(
        &kube_def,
        50 * 1024 * 1024 * 1024,
        Some(100),
        Some(100),
        Some(100),
    );
    assert_eq!(rec_kube, "keep");
}

#[test]
fn test_receipt_verification_and_plan_correlation() {
    use mac_artifact_cleaner::domain::plan::{DeletionPlan, PlanItem, PlanItemKind};
    use mac_artifact_cleaner::domain::receipt::{
        DeletionReceipt, DeletionResult, DeletionStatus, IssueType,
    };
    use std::fs;

    let tmp = tempfile::Builder::new().tempdir_in(".").unwrap();
    let file_to_delete = tmp.path().join("should_be_deleted.txt");
    fs::write(&file_to_delete, "some content").unwrap();

    let plan = DeletionPlan::new(
        vec![tmp.path().to_path_buf()],
        false,
        false,
        vec![PlanItem {
            path: file_to_delete.clone(),
            kind: PlanItemKind::File,
            reason: "temp file".to_string(),
        }],
        vec![],
    );

    // 1. Consistent receipt case: file is deleted (doesn't exist)
    fs::remove_file(&file_to_delete).unwrap();
    let consistent_receipt = DeletionReceipt::new(
        plan.created_unix,
        1716768000,
        1716768100,
        vec![DeletionResult {
            path: file_to_delete.clone(),
            status: DeletionStatus::Deleted,
            error: None,
        }],
    );
    let report = consistent_receipt.verify(Some(&plan));
    assert!(report.is_consistent);
    assert!(report.issues.is_empty());

    // 2. Inconsistent receipt case: file is marked as Deleted, but still exists on disk
    fs::write(&file_to_delete, "some content").unwrap();
    let report2 = consistent_receipt.verify(Some(&plan));
    assert!(!report2.is_consistent);
    assert_eq!(report2.issues.len(), 1);
    assert_eq!(report2.issues[0].issue_type, IssueType::PathStillExists);
    assert_eq!(report2.issues[0].path, file_to_delete);

    // 3. Plan mismatch case: extra receipt item not in plan
    fs::remove_file(&file_to_delete).unwrap();
    let mismatched_receipt = DeletionReceipt::new(
        plan.created_unix,
        1716768000,
        1716768100,
        vec![
            DeletionResult {
                path: file_to_delete.clone(),
                status: DeletionStatus::Deleted,
                error: None,
            },
            DeletionResult {
                path: tmp.path().join("extra_file.txt"),
                status: DeletionStatus::Deleted,
                error: None,
            },
        ],
    );
    let report3 = mismatched_receipt.verify(Some(&plan));
    assert!(!report3.is_consistent);
    assert!(report3
        .issues
        .iter()
        .any(|i| i.issue_type == IssueType::ExtraReceiptItem));
}

#[test]
fn test_ocel_validation_and_summarization() {
    use mac_artifact_cleaner::domain::ocel::{
        build_tool_roots_ocel, summarize_ocel_log, validate_ocel_log, OcelRelationship,
    };

    // 1. Valid empty log
    let log = build_tool_roots_ocel(&[]);
    let report = validate_ocel_log(&log);
    assert!(report.is_valid);
    assert!(report.errors.is_empty());

    let summary = summarize_ocel_log(&log);
    assert_eq!(summary.total_events, 1);
    assert_eq!(summary.total_objects, 1);

    // 2. Schema violation - undefined event type
    let mut invalid_log = log.clone();
    invalid_log.events[0].event_type = "non_existent_event_type".to_string();
    let report2 = validate_ocel_log(&invalid_log);
    assert!(!report2.is_valid);
    assert!(report2
        .errors
        .iter()
        .any(|e| e.contains("is not defined in eventTypes schema")));

    // 3. Referential integrity violation - dangling object reference
    let mut invalid_log2 = log.clone();
    invalid_log2.events[0].relationships.push(OcelRelationship {
        object_id: "non_existent_object_id".to_string(),
        qualifier: "test-ref".to_string(),
    });
    let report3 = validate_ocel_log(&invalid_log2);
    assert!(!report3.is_valid);
    assert!(report3
        .errors
        .iter()
        .any(|e| e.contains("pointing to non-existent object")));
}

#[test]
fn test_snapshot_and_exclusion_ocel_generation() {
    use mac_artifact_cleaner::domain::ocel::{
        build_exclusion_plan_ocel, build_snapshot_audit_ocel, build_snapshot_thin_ocel,
        validate_ocel_log,
    };

    // Test snapshot audit OCEL
    let audit_log = build_snapshot_audit_ocel(
        "/System/Volumes/Data",
        &["com.apple.TimeMachine.2026-05-26.local".to_string()],
    );
    println!("Object Types in Test: {:?}", audit_log.object_types);
    println!(
        "AUDIT_LOG_JSON: {}",
        serde_json::to_string_pretty(&audit_log).unwrap()
    );
    let audit_report = validate_ocel_log(&audit_log);
    if !audit_report.is_valid {
        println!("OCEL Validation Errors: {:?}", audit_report.errors);
    }
    assert!(audit_report.is_valid);
    assert!(audit_report.errors.is_empty());
    assert_eq!(audit_log.objects[0].object_type, "snapshot_state");
    assert_eq!(audit_log.events[0].event_type, "snapshot_state_observed");

    // Test snapshot thin OCEL
    let thin_log = build_snapshot_thin_ocel(
        "/",
        500_000_000,
        &["snap1".to_string(), "snap2".to_string()],
        &["snap2".to_string()],
        &["snap1".to_string()],
    );
    let thin_report = validate_ocel_log(&thin_log);
    assert!(thin_report.is_valid);
    assert!(thin_report.errors.is_empty());
    assert_eq!(thin_log.objects[0].object_type, "snapshot_state");
    assert_eq!(thin_log.events[0].event_type, "snapshot_thin_requested");

    // Test exclusion plan OCEL
    let exclusion_log = build_exclusion_plan_ocel("/Users/user/tm-exclusions.sh", 3);
    let exclusion_report = validate_ocel_log(&exclusion_log);
    assert!(exclusion_report.is_valid);
    assert!(exclusion_report.errors.is_empty());
    assert_eq!(exclusion_log.objects[0].object_type, "tm_exclusion_plan");
    assert_eq!(
        exclusion_log.events[0].event_type,
        "tm_exclusion_plan_written"
    );
}

#[test]
fn test_exclusion_planning_and_application() {
    use mac_artifact_cleaner::domain::plan::{DeletionPlan, PlanItem, PlanItemKind};
    use mac_artifact_cleaner::domain::tool_roots::ToolRootReport;
    use mac_artifact_cleaner::nouns::exclusion::{handle, ExclusionAction};

    let tmp = tempfile::Builder::new().tempdir_in(".").unwrap();
    let plan_path = tmp.path().join("deletion-plan.json");
    let script_path = tmp.path().join("tm-exclusions.sh");

    // Create directories so they exist and qualify for exclusions script
    let mock_project_dir = tmp.path().join("mock-project-target");
    let mock_tool_dir = tmp.path().join("mock-tool-cache");
    fs::create_dir(&mock_project_dir).unwrap();
    fs::create_dir(&mock_tool_dir).unwrap();

    let items = vec![PlanItem {
        path: mock_project_dir.clone(),
        kind: PlanItemKind::Dir,
        reason: "Mock rust target".to_string(),
    }];

    let tool_roots = vec![ToolRootReport {
        path: mock_tool_dir.to_string_lossy().to_string(),
        category: "node_package_cache".to_string(),
        bytes: 1024,
        human: "1KB".to_string(),
        files: 5,
        dirs: 1,
        created_unix: None,
        last_accessed_unix: None,
        last_modified_unix: None,
        metadata_changed_unix: None,
        newest_descendant_modified_unix: None,
        newest_descendant_path: None,
        days_since_modified: None,
        days_since_accessed: None,
        days_since_newest_descendant_modified: None,
        recommendation: "cleanup_candidate".to_string(),
        rationale: "mock cache".to_string(),
    }];

    let plan = DeletionPlan::new(
        vec![tmp.path().to_path_buf()],
        false,
        false,
        items,
        tool_roots,
    );
    let plan_json = serde_json::to_string(&plan).unwrap();
    fs::write(&plan_path, plan_json).unwrap();

    // Run the exclusion plan action
    let action = ExclusionAction::Plan {
        from: plan_path,
        output: script_path.clone(),
        ocel: None,
    };
    handle(action).unwrap();

    assert!(script_path.exists());
    let script_content = fs::read_to_string(&script_path).unwrap();

    // Verify both mock_project_dir and mock_tool_dir are included in the generated script
    assert!(script_content.contains(&mock_project_dir.to_string_lossy().to_string()));
    assert!(script_content.contains(&mock_tool_dir.to_string_lossy().to_string()));
}

#[test]
fn test_privacy_subcommands() {
    use mac_artifact_cleaner::nouns::privacy::{handle, PrivacyAction};

    let tmp = tempfile::Builder::new().tempdir_in(".").unwrap();
    let file_to_redact = tmp.path().join("leak.txt");
    fs::write(
        &file_to_redact,
        "Some password: \"my-secret-pw\" and secret: \"my-secret-val\" and /Users/some_other_user/path",
    )
    .unwrap();

    // Test redact subcommand
    let redact_action = PrivacyAction::Redact {
        file: file_to_redact.clone(),
    };
    handle(redact_action).unwrap();

    let redacted_content = fs::read_to_string(&file_to_redact).unwrap();
    assert!(redacted_content.contains("password: \"[REDACTED]\""));
    assert!(redacted_content.contains("secret: \"[REDACTED]\""));
    assert!(redacted_content.contains("/Users/<user>/path"));

    let _ = handle(PrivacyAction::Scan);
}
