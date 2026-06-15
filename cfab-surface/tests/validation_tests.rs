use cfab_surface::{Fabric, Relation, RelationKind, Surface};
use url::Url;

#[test]
fn test_construct_all_surface_types() {
    let mut fabric = Fabric::new();

    // 1. Construct nodes of each Surface type (Local, Github, Plan, Receipt, Document)
    let local_url = Url::parse("file:///path/to/local").unwrap();
    let github_url = Url::parse("github://owner/repo").unwrap();
    let plan_url = Url::parse("plan:///path/to/plan.json").unwrap();
    let receipt_url = Url::parse("receipt:///path/to/receipt.json").unwrap();
    let doc_url = Url::parse("doc:///path/to/doc.md").unwrap();

    let s_local = Surface::new("local_node".to_string(), local_url, "Local Dir".to_string());
    let s_github = Surface::new(
        "github_node".to_string(),
        github_url,
        "GitHub Repo".to_string(),
    );
    let s_plan = Surface::new("plan_node".to_string(), plan_url, "Plan File".to_string());
    let s_receipt = Surface::new(
        "receipt_node".to_string(),
        receipt_url,
        "Receipt File".to_string(),
    );
    let s_doc = Surface::new("doc_node".to_string(), doc_url, "Doc File".to_string());

    fabric.add_surface(s_local).unwrap();
    fabric.add_surface(s_github).unwrap();
    fabric.add_surface(s_plan).unwrap();
    fabric.add_surface(s_receipt).unwrap();
    fabric.add_surface(s_doc).unwrap();

    assert_eq!(fabric.len(), 5);
}

#[test]
fn test_invalid_relationship_rejection() {
    let mut fabric = Fabric::new();

    let plan_url = Url::parse("plan:///path/to/plan.json").unwrap();
    let receipt_url = Url::parse("receipt:///path/to/receipt.json").unwrap();

    let s_plan = Surface::new("plan_node".to_string(), plan_url, "Plan File".to_string());
    let s_receipt = Surface::new(
        "receipt_node".to_string(),
        receipt_url,
        "Receipt File".to_string(),
    );

    fabric.add_surface(s_plan).unwrap();
    fabric.add_surface(s_receipt).unwrap();

    let result = fabric.connect(
        "receipt_node",
        "plan_node",
        Relation::new(RelationKind::Dependency, 1.0),
    );

    assert!(
        result.is_err(),
        "Connecting receipt_node -> plan_node should be rejected as an invalid relationship"
    );
}

#[test]
fn test_cycle_detection() {
    let mut fabric = Fabric::new();

    let local_url = Url::parse("file:///path/to/local").unwrap();
    let github_url = Url::parse("github://owner/repo").unwrap();

    let s_local = Surface::new("local_node".to_string(), local_url, "Local Dir".to_string());
    let s_github = Surface::new(
        "github_node".to_string(),
        github_url,
        "GitHub Repo".to_string(),
    );

    fabric.add_surface(s_local).unwrap();
    fabric.add_surface(s_github).unwrap();

    fabric
        .connect(
            "local_node",
            "github_node",
            Relation::new(RelationKind::Dependency, 1.0),
        )
        .unwrap();
    let result = fabric.connect(
        "github_node",
        "local_node",
        Relation::new(RelationKind::Dependency, 1.0),
    );

    assert!(matches!(
        result,
        Err(cfab_surface::FabricError::CycleDetected)
    ));
}

#[test]
fn test_overwrite_kind_fails_validation() {
    let mut fabric = Fabric::new();

    let s1 = Surface::new(
        "s1".to_string(),
        Url::parse("plan:///path/to/plan.json").unwrap(),
        "Plan".to_string(),
    );
    let s2 = Surface::new(
        "s2".to_string(),
        Url::parse("receipt:///path/to/receipt.json").unwrap(),
        "Receipt".to_string(),
    );

    fabric.add_surface(s1).unwrap();
    fabric.add_surface(s2).unwrap();

    fabric
        .connect("s1", "s2", Relation::new(RelationKind::Dependency, 1.0))
        .unwrap();

    let s2_new = Surface::new(
        "s2".to_string(),
        Url::parse("github://owner/repo").unwrap(),
        "GH Repo".to_string(),
    );
    let result = fabric.add_surface(s2_new);

    assert!(result.is_err());
    if let Err(cfab_surface::FabricError::InvalidConnection { from, to, reason }) = result {
        assert_eq!(from, "s1");
        assert_eq!(to, "s2");
        assert!(reason.contains("Plan cannot point to a GitHubRepository"));
    } else {
        panic!("Expected InvalidConnection error");
    }

    let path = fabric.find_path("s1", "s2").unwrap();
    assert_eq!(path.len(), 2);
    assert_eq!(path[0].kind, cfab_surface::SurfaceKind::Plan);
    assert_eq!(path[1].kind, cfab_surface::SurfaceKind::Receipt);
}

#[test]
fn test_edge_kind_invalidation_on_update() {
    let mut fabric = Fabric::new();

    let plan_url = Url::parse("plan:///path/to/plan.json").unwrap();
    let receipt_url = Url::parse("receipt:///path/to/receipt.json").unwrap();

    let s_plan = Surface::new(
        "node_1".to_string(),
        plan_url.clone(),
        "Plan File".to_string(),
    );
    let s_receipt = Surface::new(
        "node_2".to_string(),
        receipt_url.clone(),
        "Receipt File".to_string(),
    );

    fabric.add_surface(s_plan).unwrap();
    fabric.add_surface(s_receipt).unwrap();

    fabric
        .connect(
            "node_1",
            "node_2",
            Relation::new(RelationKind::Dependency, 1.0),
        )
        .unwrap();

    let s_receipt_new = Surface::new(
        "node_1".to_string(),
        receipt_url.clone(),
        "Receipt File".to_string(),
    );
    fabric.add_surface(s_receipt_new).unwrap();

    let s_plan_new = Surface::new(
        "node_2".to_string(),
        plan_url.clone(),
        "Plan File".to_string(),
    );
    let result = fabric.add_surface(s_plan_new);

    assert!(result.is_err());
    if let Err(cfab_surface::FabricError::InvalidConnection { from, to, reason }) = result {
        assert_eq!(from, "node_1");
        assert_eq!(to, "node_2");
        assert!(reason.contains("Receipt cannot point to a Plan"));
    } else {
        panic!("Expected InvalidConnection error");
    }

    let path = fabric.find_path("node_1", "node_2").unwrap();
    assert_eq!(path.len(), 2);
    assert_eq!(path[0].kind, cfab_surface::SurfaceKind::Receipt);
    assert_eq!(path[1].kind, cfab_surface::SurfaceKind::Receipt);
}

#[test]
fn test_duplicate_edge_detection() {
    let mut fabric = Fabric::new();

    let plan_url = Url::parse("plan:///path/to/plan.json").unwrap();
    let receipt_url = Url::parse("receipt:///path/to/receipt.json").unwrap();

    let s_plan = Surface::new("node_1".to_string(), plan_url, "Plan File".to_string());
    let s_receipt = Surface::new(
        "node_2".to_string(),
        receipt_url,
        "Receipt File".to_string(),
    );

    fabric.add_surface(s_plan).unwrap();
    fabric.add_surface(s_receipt).unwrap();

    fabric
        .connect(
            "node_1",
            "node_2",
            Relation::new(RelationKind::Dependency, 1.0),
        )
        .unwrap();

    let result = fabric.connect(
        "node_1",
        "node_2",
        Relation::new(RelationKind::Dependency, 1.0),
    );

    assert!(result.is_err());
    if let Err(cfab_surface::FabricError::InvalidConnection { from, to, reason }) = result {
        assert_eq!(from, "node_1");
        assert_eq!(to, "node_2");
        assert!(reason.contains("already exists"));
    } else {
        panic!("Expected InvalidConnection error");
    }
}

#[test]
fn test_observe_state_percent_encoding() {
    let temp_dir = std::env::temp_dir();
    let file_name = "test file space.txt";
    let temp_file_path = temp_dir.join(file_name);
    std::fs::write(&temp_file_path, b"test content").unwrap();

    let file_url_str = format!("file://{}", temp_file_path.to_string_lossy());
    let url = Url::parse(&file_url_str).unwrap();

    let s = Surface::new("temp_file".to_string(), url, "Temp File".to_string());

    let state = s.observe_state().unwrap();
    let _ = std::fs::remove_file(&temp_file_path);

    assert!(state.exists, "Failed to locate file with space in path");
    assert_eq!(
        state.metadata.get("path").unwrap(),
        &temp_file_path.to_string_lossy().to_string()
    );
}

#[test]
fn test_nan_weight_validation() {
    let mut fabric = Fabric::new();

    let plan_url = Url::parse("plan:///path/to/plan.json").unwrap();
    let receipt_url = Url::parse("receipt:///path/to/receipt.json").unwrap();

    let s_plan = Surface::new("node_1".to_string(), plan_url, "Plan".to_string());
    let s_receipt = Surface::new("node_2".to_string(), receipt_url, "Receipt".to_string());

    fabric.add_surface(s_plan).unwrap();
    fabric.add_surface(s_receipt).unwrap();

    let relation = Relation {
        kind: RelationKind::Dependency,
        weight: f64::NAN,
    };
    let result = fabric.connect("node_1", "node_2", relation);
    assert!(result.is_err());
    if let Err(cfab_surface::FabricError::InvalidConnection { from, to, reason }) = result {
        assert_eq!(from, "node_1");
        assert_eq!(to, "node_2");
        assert!(reason.contains("weight must be finite"));
    } else {
        panic!("Expected InvalidConnection error");
    }

    let relation_inf = Relation {
        kind: RelationKind::Dependency,
        weight: f64::INFINITY,
    };
    let result_inf = fabric.connect("node_1", "node_2", relation_inf);
    assert!(result_inf.is_err());
}

#[test]
#[should_panic(expected = "Relation weight must be finite")]
fn test_nan_weight_panic() {
    let _ = Relation::new(RelationKind::Dependency, f64::NAN);
}

#[test]
#[should_panic(expected = "Relation weight must be finite")]
fn test_infinity_weight_panic() {
    let _ = Relation::new(RelationKind::Dependency, f64::INFINITY);
}
