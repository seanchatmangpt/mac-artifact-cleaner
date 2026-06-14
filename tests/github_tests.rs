use osx_clnr::domain::github::GithubTarget;
use osx_clnr::domain::plan::{DeletionPlan, PlanItem, PlanItemKind};
use osx_clnr::domain::receipt::{DeletionReceipt, DeletionResult, DeletionStatus};
use osx_clnr::integration::github::MockCommandExecutor;
use osx_clnr::nouns::github::discover_candidates;
use std::path::PathBuf;

#[test]
fn test_github_target_parsing_and_roundtrip() {
    // Repo
    let p_repo = PathBuf::from("github://repo/owner/my-repo");
    let target_repo = GithubTarget::parse(&p_repo).unwrap();
    assert_eq!(
        target_repo,
        GithubTarget::Repo {
            owner: "owner".to_string(),
            repo: "my-repo".to_string()
        }
    );
    assert_eq!(target_repo.to_path_buf(), p_repo);

    // Branch
    let p_branch = PathBuf::from("github://branch/owner/my-repo/feature/foo-bar");
    let target_branch = GithubTarget::parse(&p_branch).unwrap();
    assert_eq!(
        target_branch,
        GithubTarget::Branch {
            owner: "owner".to_string(),
            repo: "my-repo".to_string(),
            branch: "feature/foo-bar".to_string()
        }
    );
    assert_eq!(target_branch.to_path_buf(), p_branch);

    // Run
    let p_run = PathBuf::from("github://run/owner/my-repo/998877");
    let target_run = GithubTarget::parse(&p_run).unwrap();
    assert_eq!(
        target_run,
        GithubTarget::Run {
            owner: "owner".to_string(),
            repo: "my-repo".to_string(),
            run_id: 998877
        }
    );
    assert_eq!(target_run.to_path_buf(), p_run);

    // Release
    let p_release = PathBuf::from("github://release/owner/my-repo/v1.0.0-draft/1");
    let target_release = GithubTarget::parse(&p_release).unwrap();
    assert_eq!(
        target_release,
        GithubTarget::Release {
            owner: "owner".to_string(),
            repo: "my-repo".to_string(),
            tag: "v1.0.0-draft/1".to_string()
        }
    );
    assert_eq!(target_release.to_path_buf(), p_release);
}

#[test]
fn test_github_discover_candidates() {
    let mock = MockCommandExecutor::new();

    // 1. Mock repo list command
    // We have three repos:
    // - "empty-repo" (isEmpty = true) -> Repo Candidate
    // - "stale-repo" (isEmpty = false, updatedAt/pushedAt = older) -> Repo Candidate
    // - "active-repo" (isEmpty = false, active) -> We will scan branches, runs, releases
    let repo_list_json = r#"[
        {
            "name": "empty-repo",
            "nameWithOwner": "my-org/empty-repo",
            "owner": {"login": "my-org"},
            "isArchived": false,
            "isFork": false,
            "isEmpty": true,
            "updatedAt": "2026-06-10T12:00:00Z",
            "createdAt": "2026-06-10T12:00:00Z",
            "diskUsage": 0,
            "visibility": "PUBLIC"
        },
        {
            "name": "stale-repo",
            "nameWithOwner": "my-org/stale-repo",
            "owner": {"login": "my-org"},
            "isArchived": false,
            "isFork": false,
            "isEmpty": false,
            "pushedAt": "2025-12-10T12:00:00Z",
            "updatedAt": "2025-12-10T12:00:00Z",
            "createdAt": "2025-12-10T12:00:00Z",
            "diskUsage": 100,
            "visibility": "PUBLIC"
        },
        {
            "name": "active-repo",
            "nameWithOwner": "my-org/active-repo",
            "owner": {"login": "my-org"},
            "isArchived": false,
            "isFork": false,
            "isEmpty": false,
            "pushedAt": "2026-06-14T12:00:00Z",
            "updatedAt": "2026-06-14T12:00:00Z",
            "createdAt": "2026-06-10T12:00:00Z",
            "diskUsage": 200,
            "visibility": "PUBLIC"
        }
    ]"#;
    mock.add_response(
        "gh",
        &[
            "repo",
            "list",
            "--limit",
            "1000",
            "--json",
            "name,nameWithOwner,owner,isArchived,isFork,isEmpty,pushedAt,updatedAt,createdAt,diskUsage,visibility",
        ],
        0,
        repo_list_json,
        "",
    );

    // Stub runs, branches, and releases for empty-repo and stale-repo to not be called (they are candidates and skipped from further scans)

    // For active-repo:
    // Runs:
    // - Run 1: completed, older than 30 days -> Run Candidate
    // - Run 2: in_progress -> Ignored
    let run_list_json = r#"[
        {
            "databaseId": 111,
            "id": 111,
            "number": 1,
            "name": "CI",
            "headBranch": "main",
            "status": "completed",
            "conclusion": "success",
            "createdAt": "2026-05-10T12:00:00Z",
            "updatedAt": "2026-05-10T12:30:00Z"
        },
        {
            "databaseId": 222,
            "id": 222,
            "number": 2,
            "name": "CI",
            "headBranch": "feature",
            "status": "in_progress",
            "conclusion": null,
            "createdAt": "2026-06-14T12:00:00Z",
            "updatedAt": "2026-06-14T12:00:00Z"
        }
    ]"#;
    mock.add_response(
        "gh",
        &[
            "run",
            "list",
            "--repo",
            "my-org/active-repo",
            "--limit",
            "100",
            "--json",
            "databaseId,number,name,headBranch,status,conclusion,createdAt,updatedAt",
        ],
        0,
        run_list_json,
        "",
    );

    // Branches:
    // - main (default) -> Ignored
    // - feature/merged (compare status identical) -> Branch Candidate
    // - feature/active (compare status ahead) -> Ignored
    let branch_list_json = r#"[
        {
            "name": "main",
            "commit": {"sha": "aaa", "url": "url-main"},
            "protected": true
        },
        {
            "name": "feature/merged",
            "commit": {"sha": "bbb", "url": "url-merged"},
            "protected": false
        },
        {
            "name": "feature/active",
            "commit": {"sha": "ccc", "url": "url-active"},
            "protected": false
        }
    ]"#;
    mock.add_response(
        "gh",
        &["api", "repos/my-org/active-repo/branches"],
        0,
        branch_list_json,
        "",
    );

    let compare_merged_json = r#"{"status": "identical", "ahead_by": 0, "behind_by": 0, "total_commits": 0}"#;
    mock.add_response(
        "gh",
        &["api", "repos/my-org/active-repo/compare/main...feature/merged"],
        0,
        compare_merged_json,
        "",
    );

    let compare_active_json = r#"{"status": "ahead", "ahead_by": 2, "behind_by": 0, "total_commits": 2}"#;
    mock.add_response(
        "gh",
        &["api", "repos/my-org/active-repo/compare/main...feature/active"],
        0,
        compare_active_json,
        "",
    );

    // Releases:
    // - v1.0.0-draft (isDraft = true) -> Release Candidate
    // - v0.9.0 (published, 2026-06-13, not older than 30 days) -> Ignored
    let release_list_json = r#"[
        {
            "tagName": "v1.0.0-draft",
            "name": "Draft 1.0",
            "isDraft": true,
            "isPrerelease": false,
            "createdAt": "2026-06-12T12:00:00Z",
            "publishedAt": null
        },
        {
            "tagName": "v0.9.0",
            "name": "Release 0.9",
            "isDraft": false,
            "isPrerelease": false,
            "createdAt": "2026-06-13T12:00:00Z",
            "publishedAt": "2026-06-13T12:05:00Z"
        }
    ]"#;
    mock.add_response(
        "gh",
        &[
            "release",
            "list",
            "--repo",
            "my-org/active-repo",
            "--limit",
            "100",
            "--json",
            "tagName,name,isDraft,isPrerelease,createdAt,publishedAt",
        ],
        0,
        release_list_json,
        "",
    );

    // Perform scan discovery
    // We assume current date in tests is around 2026-06-14
    let candidates = discover_candidates(&mock, 180, 30, 30).unwrap();

    assert_eq!(candidates.len(), 5);

    assert_eq!(candidates[0].path, PathBuf::from("github://repo/my-org/empty-repo"));
    assert_eq!(candidates[0].kind, PlanItemKind::GithubRepo);
    assert_eq!(candidates[0].reason, "Empty repository");

    assert_eq!(candidates[1].path, PathBuf::from("github://repo/my-org/stale-repo"));
    assert_eq!(candidates[1].kind, PlanItemKind::GithubRepo);
    assert_eq!(candidates[1].reason, "Stale repository (inactive for > 180 days)");

    assert_eq!(candidates[2].path, PathBuf::from("github://run/my-org/active-repo/111"));
    assert_eq!(candidates[2].kind, PlanItemKind::GithubRun);
    assert_eq!(candidates[2].reason, "Completed workflow run older than 30 days (run #1)");

    assert_eq!(candidates[3].path, PathBuf::from("github://branch/my-org/active-repo/feature/merged"));
    assert_eq!(candidates[3].kind, PlanItemKind::GithubBranch);
    assert_eq!(candidates[3].reason, "Branch fully merged into default branch (main)");

    assert_eq!(candidates[4].path, PathBuf::from("github://release/my-org/active-repo/v1.0.0-draft"));
    assert_eq!(candidates[4].kind, PlanItemKind::GithubRelease);
    assert_eq!(candidates[4].reason, "Draft release");
}

#[test]
fn test_github_deletions_execution() {
    let mock = MockCommandExecutor::new();

    // Mock successful deletions
    mock.add_response("gh", &["repo", "delete", "my-org/empty-repo", "--confirm"], 0, "", "");
    mock.add_response("gh", &["run", "delete", "111", "--repo", "my-org/active-repo"], 0, "", "");
    mock.add_response("gh", &["api", "-X", "DELETE", "repos/my-org/active-repo/git/refs/heads/feature/merged"], 0, "", "");
    mock.add_response("gh", &["release", "delete", "v1.0.0-draft", "--repo", "my-org/active-repo", "--yes", "--cleanup-tag"], 0, "", "");

    let items = vec![
        PlanItem {
            path: PathBuf::from("github://repo/my-org/empty-repo"),
            kind: PlanItemKind::GithubRepo,
            reason: "Empty repository".to_string(),
            bytes: 0,
        },
        PlanItem {
            path: PathBuf::from("github://run/my-org/active-repo/111"),
            kind: PlanItemKind::GithubRun,
            reason: "Stale workflow run".to_string(),
            bytes: 0,
        },
        PlanItem {
            path: PathBuf::from("github://branch/my-org/active-repo/feature/merged"),
            kind: PlanItemKind::GithubBranch,
            reason: "Merged branch".to_string(),
            bytes: 0,
        },
        PlanItem {
            path: PathBuf::from("github://release/my-org/active-repo/v1.0.0-draft"),
            kind: PlanItemKind::GithubRelease,
            reason: "Draft release".to_string(),
            bytes: 0,
        },
    ];

    let plan = DeletionPlan::new(
        vec![PathBuf::from("github://")],
        false,
        false,
        items,
        vec![],
    );

    // Let's manually run the delete execution using our mock executor to test the logic
    let start_time = 0;
    let mut results = Vec::new();

    for item in &plan.items {
        let res = if let Some(target) = GithubTarget::parse(&item.path) {
            let delete_result = match target {
                GithubTarget::Repo { owner, repo } => osx_clnr::integration::github::delete_repository(&mock, &owner, &repo),
                GithubTarget::Branch { owner, repo, branch } => osx_clnr::integration::github::delete_branch(&mock, &owner, &repo, &branch),
                GithubTarget::Run { owner, repo, run_id } => osx_clnr::integration::github::delete_run(&mock, &owner, &repo, run_id),
                GithubTarget::Release { owner, repo, tag } => osx_clnr::integration::github::delete_release(&mock, &owner, &repo, &tag),
            };
            match delete_result {
                Ok(()) => DeletionResult {
                    path: item.path.clone(),
                    status: DeletionStatus::Deleted,
                    error: None,
                    blake3_hash: None,
                    bytes_freed: 0,
                },
                Err(e) => DeletionResult {
                    path: item.path.clone(),
                    status: DeletionStatus::Failed,
                    error: Some(e.to_string()),
                    blake3_hash: None,
                    bytes_freed: 0,
                },
            }
        } else {
            DeletionResult {
                path: item.path.clone(),
                status: DeletionStatus::Failed,
                error: Some("Invalid path".to_string()),
                blake3_hash: None,
                bytes_freed: 0,
            }
        };
        results.push(res);
    }

    let end_time = 1;
    let receipt = DeletionReceipt::new(
        "test-github-deletion".to_string(),
        plan.created_unix,
        start_time,
        end_time,
        results,
        None,
        None,
    );

    // Verify receipt has 4 deleted items
    assert_eq!(receipt.execution_record.results.len(), 4);
    for r in &receipt.execution_record.results {
        assert_eq!(r.status, DeletionStatus::Deleted);
        assert!(r.error.is_none());
        assert_eq!(r.bytes_freed, 0);
    }

    // Verify correct commands were made
    let calls = mock.get_calls();
    assert_eq!(calls.len(), 4);
    assert_eq!(calls[0], vec!["gh".to_string(), "repo".to_string(), "delete".to_string(), "my-org/empty-repo".to_string(), "--confirm".to_string()]);
    assert_eq!(calls[1], vec!["gh".to_string(), "run".to_string(), "delete".to_string(), "111".to_string(), "--repo".to_string(), "my-org/active-repo".to_string()]);
    assert_eq!(calls[2], vec!["gh".to_string(), "api".to_string(), "-X".to_string(), "DELETE".to_string(), "repos/my-org/active-repo/git/refs/heads/feature/merged".to_string()]);
    assert_eq!(calls[3], vec!["gh".to_string(), "release".to_string(), "delete".to_string(), "v1.0.0-draft".to_string(), "--repo".to_string(), "my-org/active-repo".to_string(), "--yes".to_string(), "--cleanup-tag".to_string()]);
}

#[test]
fn test_github_zero_thresholds() {
    let mock = MockCommandExecutor::new();

    // Repo list: one repo that is active but not empty
    let repo_list_json = r#"[
        {
            "name": "active-repo",
            "nameWithOwner": "my-org/active-repo",
            "owner": {"login": "my-org"},
            "isArchived": false,
            "isFork": false,
            "isEmpty": false,
            "pushedAt": "2026-06-14T12:00:00Z",
            "updatedAt": "2026-06-14T12:00:00Z",
            "createdAt": "2026-06-10T12:00:00Z",
            "diskUsage": 200,
            "visibility": "PUBLIC"
        }
    ]"#;
    mock.add_response(
        "gh",
        &[
            "repo",
            "list",
            "--limit",
            "1000",
            "--json",
            "name,nameWithOwner,owner,isArchived,isFork,isEmpty,pushedAt,updatedAt,createdAt,diskUsage,visibility",
        ],
        0,
        repo_list_json,
        "",
    );

    // Call discover_candidates with 0-day threshold
    // Even if it was pushed 1 second ago, it will be marked stale
    let candidates = discover_candidates(&mock, 0, 0, 0).unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].path, PathBuf::from("github://repo/my-org/active-repo"));
    assert_eq!(candidates[0].reason, "Stale repository (inactive for > 0 days)");
}

#[test]
fn test_github_custom_default_branch_heuristic_issue() {
    let mock = MockCommandExecutor::new();

    // Repo list with active repo
    let repo_list_json = r#"[
        {
            "name": "active-repo",
            "nameWithOwner": "my-org/active-repo",
            "owner": {"login": "my-org"},
            "isArchived": false,
            "isFork": false,
            "isEmpty": false,
            "pushedAt": "2026-06-14T12:00:00Z",
            "updatedAt": "2026-06-14T12:00:00Z",
            "createdAt": "2026-06-10T12:00:00Z",
            "diskUsage": 200,
            "visibility": "PUBLIC"
        }
    ]"#;
    mock.add_response(
        "gh",
        &[
            "repo",
            "list",
            "--limit",
            "1000",
            "--json",
            "name,nameWithOwner,owner,isArchived,isFork,isEmpty,pushedAt,updatedAt,createdAt,diskUsage,visibility",
        ],
        0,
        repo_list_json,
        "",
    );

    // No runs
    mock.add_response(
        "gh",
        &[
            "run",
            "list",
            "--repo",
            "my-org/active-repo",
            "--limit",
            "100",
            "--json",
            "databaseId,number,name,headBranch,status,conclusion,createdAt,updatedAt",
        ],
        0,
        "[]",
        "",
    );

    // Branches: feature/merged is listed first, then the actual default branch develop.
    // Since neither "main" nor "master" exists, the heuristic selects the first branch ("feature/merged") as default.
    let branch_list_json = r#"[
        {
            "name": "feature/merged",
            "commit": {"sha": "bbb", "url": "url-merged"},
            "protected": false
        },
        {
            "name": "develop",
            "commit": {"sha": "aaa", "url": "url-develop"},
            "protected": false
        }
    ]"#;
    mock.add_response(
        "gh",
        &["api", "repos/my-org/active-repo/branches"],
        0,
        branch_list_json,
        "",
    );

    // feature/merged is treated as default.
    // develop is compared against feature/merged.
    // If develop is ahead, it is not merged.
    let compare_develop_json = r#"{"status": "ahead", "ahead_by": 5, "behind_by": 0, "total_commits": 5}"#;
    mock.add_response(
        "gh",
        &["api", "repos/my-org/active-repo/compare/feature/merged...develop"],
        0,
        compare_develop_json,
        "",
    );

    // No releases
    mock.add_response(
        "gh",
        &[
            "release",
            "list",
            "--repo",
            "my-org/active-repo",
            "--limit",
            "100",
            "--json",
            "tagName,name,isDraft,isPrerelease,createdAt,publishedAt",
        ],
        0,
        "[]",
        "",
    );

    let candidates = discover_candidates(&mock, 180, 30, 30).unwrap();

    // feature/merged was ignored because it was incorrectly selected as the default branch!
    // So we found 0 candidates instead of finding feature/merged as merged into develop.
    assert_eq!(candidates.len(), 0);
}

