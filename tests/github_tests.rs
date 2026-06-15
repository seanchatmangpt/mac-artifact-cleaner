use osx_clnr::domain::github::GithubTarget;
use osx_clnr::domain::plan::{DeletionPlan, PlanItem, PlanItemKind};
use osx_clnr::domain::receipt::{DeletionReceipt, DeletionResult, DeletionStatus};
use osx_clnr::integration::github::MockCommandExecutor;
use osx_clnr::nouns::github::{discover_candidates, handle, GithubAction};
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

    // Cache
    let p_cache = PathBuf::from("github://cache/owner/my-repo/12345/my-key/v1");
    let target_cache = GithubTarget::parse(&p_cache).unwrap();
    assert_eq!(
        target_cache,
        GithubTarget::Cache {
            owner: "owner".to_string(),
            repo: "my-repo".to_string(),
            cache_id: 12345,
            key: "my-key/v1".to_string()
        }
    );
    assert_eq!(target_cache.to_path_buf(), p_cache);

    // Issue
    let p_issue = PathBuf::from("github://issue/owner/my-repo/42");
    let target_issue = GithubTarget::parse(&p_issue).unwrap();
    assert_eq!(
        target_issue,
        GithubTarget::Issue {
            owner: "owner".to_string(),
            repo: "my-repo".to_string(),
            number: 42
        }
    );
    assert_eq!(target_issue.to_path_buf(), p_issue);

    // PR
    let p_pr = PathBuf::from("github://pr/owner/my-repo/101");
    let target_pr = GithubTarget::parse(&p_pr).unwrap();
    assert_eq!(
        target_pr,
        GithubTarget::Pr {
            owner: "owner".to_string(),
            repo: "my-repo".to_string(),
            number: 101
        }
    );
    assert_eq!(target_pr.to_path_buf(), p_pr);

    // ReleaseAsset
    let p_asset = PathBuf::from("github://release-asset/owner/my-repo/555/large-asset.zip");
    let target_asset = GithubTarget::parse(&p_asset).unwrap();
    assert_eq!(
        target_asset,
        GithubTarget::ReleaseAsset {
            owner: "owner".to_string(),
            repo: "my-repo".to_string(),
            asset_id: 555,
            asset_name: "large-asset.zip".to_string()
        }
    );
    assert_eq!(target_asset.to_path_buf(), p_asset);
}

#[test]
fn test_github_discover_candidates() {
    let mock = MockCommandExecutor::new();

    // 1. Mock repo list command
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
            "visibility": "PUBLIC",
            "defaultBranchRef": null
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
            "visibility": "PUBLIC",
            "defaultBranchRef": null
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
            "visibility": "PUBLIC",
            "defaultBranchRef": {
                "name": "main"
            }
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
            "name,nameWithOwner,owner,isArchived,isFork,isEmpty,pushedAt,updatedAt,createdAt,diskUsage,visibility,defaultBranchRef",
        ],
        0,
        repo_list_json,
        "",
    );

    // 2. Mock runs for active-repo
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
            "1000",
            "--json",
            "databaseId,number,name,headBranch,status,conclusion,createdAt,updatedAt",
        ],
        0,
        run_list_json,
        "",
    );

    // 3. Mock branches for active-repo
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
        &[
            "api",
            "repos/my-org/active-repo/branches?per_page=100&page=1",
        ],
        0,
        branch_list_json,
        "",
    );

    let compare_merged_json =
        r#"{"status": "identical", "ahead_by": 0, "behind_by": 0, "total_commits": 0}"#;
    mock.add_response(
        "gh",
        &[
            "api",
            "repos/my-org/active-repo/compare/main...feature/merged",
        ],
        0,
        compare_merged_json,
        "",
    );

    let compare_active_json =
        r#"{"status": "ahead", "ahead_by": 2, "behind_by": 0, "total_commits": 2}"#;
    mock.add_response(
        "gh",
        &[
            "api",
            "repos/my-org/active-repo/compare/main...feature/active",
        ],
        0,
        compare_active_json,
        "",
    );

    // 4. Mock releases for active-repo
    let release_list_json = r#"[
        {
            "tagName": "v1.0.0-draft",
            "name": "Draft 1.0",
            "isDraft": true,
            "isPrerelease": false,
            "createdAt": "2026-06-12T12:00:00Z",
            "publishedAt": null,
            "assets": []
        },
        {
            "tagName": "v0.9.0",
            "name": "Release 0.9",
            "isDraft": false,
            "isPrerelease": false,
            "createdAt": "2026-06-13T12:05:00Z",
            "publishedAt": "2026-06-13T12:05:00Z",
            "assets": [
                {
                    "id": 555,
                    "name": "large-asset.zip",
                    "size": 10485760
                },
                {
                    "id": 666,
                    "name": "small-asset.txt",
                    "size": 1024
                }
            ]
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
            "1000",
            "--json",
            "tagName,name,isDraft,isPrerelease,createdAt,publishedAt,assets",
        ],
        0,
        release_list_json,
        "",
    );

    // 5. Mock caches for active-repo
    let cache_list_json = r#"[
        {
            "id": 333,
            "key": "stale-cache",
            "sizeInBytes": 5000,
            "createdAt": "2025-12-10T12:00:00Z",
            "lastAccessedAt": "2025-12-10T12:00:00Z"
        },
        {
            "id": 444,
            "key": "active-cache",
            "sizeInBytes": 2000,
            "createdAt": "2026-06-13T12:00:00Z",
            "lastAccessedAt": "2026-06-13T12:00:00Z"
        }
    ]"#;
    mock.add_response(
        "gh",
        &[
            "cache",
            "list",
            "--repo",
            "my-org/active-repo",
            "--limit",
            "1000",
            "--json",
            "id,key,sizeInBytes,createdAt,lastAccessedAt",
        ],
        0,
        cache_list_json,
        "",
    );

    // 6. Mock issues for active-repo
    let issue_list_json = r#"[
        {
            "number": 12,
            "title": "Stale issue",
            "state": "OPEN",
            "createdAt": "2025-12-10T12:00:00Z",
            "updatedAt": "2025-12-10T12:00:00Z",
            "labels": []
        },
        {
            "number": 13,
            "title": "Closed issue",
            "state": "CLOSED",
            "createdAt": "2025-12-10T12:00:00Z",
            "updatedAt": "2025-12-10T12:00:00Z",
            "labels": []
        }
    ]"#;
    mock.add_response(
        "gh",
        &[
            "issue",
            "list",
            "--repo",
            "my-org/active-repo",
            "--state",
            "all",
            "--limit",
            "1000",
            "--json",
            "number,title,state,createdAt,updatedAt,labels",
        ],
        0,
        issue_list_json,
        "",
    );

    // 7. Mock PRs for active-repo
    let pr_list_json = r#"[
        {
            "number": 24,
            "title": "Stale PR",
            "state": "OPEN",
            "createdAt": "2025-12-10T12:00:00Z",
            "updatedAt": "2025-12-10T12:00:00Z",
            "labels": []
        },
        {
            "number": 25,
            "title": "Merged PR",
            "state": "MERGED",
            "createdAt": "2025-12-10T12:00:00Z",
            "updatedAt": "2025-12-10T12:00:00Z",
            "labels": []
        }
    ]"#;
    mock.add_response(
        "gh",
        &[
            "pr",
            "list",
            "--repo",
            "my-org/active-repo",
            "--state",
            "all",
            "--limit",
            "1000",
            "--json",
            "number,title,state,createdAt,updatedAt,labels",
        ],
        0,
        pr_list_json,
        "",
    );

    // Perform scan discovery
    let candidates = discover_candidates(&mock, 180, 30, 30, 30, 30, 30, 5).unwrap();

    assert_eq!(candidates.len(), 9);

    assert_eq!(
        candidates[0].path,
        PathBuf::from("github://repo/my-org/empty-repo")
    );
    assert_eq!(candidates[0].kind, PlanItemKind::GithubRepo);
    assert_eq!(candidates[0].reason, "Empty repository");

    assert_eq!(
        candidates[1].path,
        PathBuf::from("github://repo/my-org/stale-repo")
    );
    assert_eq!(candidates[1].kind, PlanItemKind::GithubRepo);
    assert_eq!(
        candidates[1].reason,
        "Stale repository (inactive for > 180 days)"
    );

    assert_eq!(
        candidates[2].path,
        PathBuf::from("github://run/my-org/active-repo/111")
    );
    assert_eq!(candidates[2].kind, PlanItemKind::GithubRun);
    assert_eq!(
        candidates[2].reason,
        "Completed workflow run older than 30 days (run #1)"
    );

    assert_eq!(
        candidates[3].path,
        PathBuf::from("github://branch/my-org/active-repo/feature/merged")
    );
    assert_eq!(candidates[3].kind, PlanItemKind::GithubBranch);
    assert_eq!(
        candidates[3].reason,
        "Branch fully merged into default branch (main)"
    );

    assert_eq!(
        candidates[4].path,
        PathBuf::from("github://release/my-org/active-repo/v1.0.0-draft")
    );
    assert_eq!(candidates[4].kind, PlanItemKind::GithubRelease);
    assert_eq!(candidates[4].reason, "Draft release");

    assert_eq!(
        candidates[5].path,
        PathBuf::from("github://release-asset/my-org/active-repo/555/large-asset.zip")
    );
    assert_eq!(candidates[5].kind, PlanItemKind::GithubReleaseAsset);
    assert_eq!(candidates[5].bytes, 10485760);

    assert_eq!(
        candidates[6].path,
        PathBuf::from("github://cache/my-org/active-repo/333/stale-cache")
    );
    assert_eq!(candidates[6].kind, PlanItemKind::GithubCache);
    assert_eq!(candidates[6].bytes, 5000);

    assert_eq!(
        candidates[7].path,
        PathBuf::from("github://issue/my-org/active-repo/12")
    );
    assert_eq!(candidates[7].kind, PlanItemKind::GithubIssue);

    assert_eq!(
        candidates[8].path,
        PathBuf::from("github://pr/my-org/active-repo/24")
    );
    assert_eq!(candidates[8].kind, PlanItemKind::GithubPr);
}

#[test]
fn test_github_deletions_execution() {
    let mock = MockCommandExecutor::new();

    // Mock successful deletions
    mock.add_response(
        "gh",
        &["repo", "delete", "my-org/empty-repo", "--yes"],
        0,
        "",
        "",
    );
    mock.add_response(
        "gh",
        &["run", "delete", "111", "--repo", "my-org/active-repo"],
        0,
        "",
        "",
    );
    mock.add_response(
        "gh",
        &[
            "api",
            "-X",
            "DELETE",
            "repos/my-org/active-repo/git/refs/heads/feature/merged",
        ],
        0,
        "",
        "",
    );
    mock.add_response(
        "gh",
        &[
            "release",
            "delete",
            "v1.0.0-draft",
            "--repo",
            "my-org/active-repo",
            "--yes",
            "--cleanup-tag",
        ],
        0,
        "",
        "",
    );
    mock.add_response(
        "gh",
        &["cache", "delete", "333", "--repo", "my-org/active-repo"],
        0,
        "",
        "",
    );
    mock.add_response(
        "gh",
        &["issue", "close", "12", "--repo", "my-org/active-repo"],
        0,
        "",
        "",
    );
    mock.add_response(
        "gh",
        &["pr", "close", "24", "--repo", "my-org/active-repo"],
        0,
        "",
        "",
    );
    mock.add_response(
        "gh",
        &[
            "api",
            "-X",
            "DELETE",
            "repos/my-org/active-repo/releases/assets/555",
        ],
        0,
        "",
        "",
    );

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
        PlanItem {
            path: PathBuf::from("github://cache/my-org/active-repo/333/stale-cache"),
            kind: PlanItemKind::GithubCache,
            reason: "Stale cache".to_string(),
            bytes: 5000,
        },
        PlanItem {
            path: PathBuf::from("github://issue/my-org/active-repo/12"),
            kind: PlanItemKind::GithubIssue,
            reason: "Stale issue".to_string(),
            bytes: 0,
        },
        PlanItem {
            path: PathBuf::from("github://pr/my-org/active-repo/24"),
            kind: PlanItemKind::GithubPr,
            reason: "Stale PR".to_string(),
            bytes: 0,
        },
        PlanItem {
            path: PathBuf::from("github://release-asset/my-org/active-repo/555/large-asset.zip"),
            kind: PlanItemKind::GithubReleaseAsset,
            reason: "Large asset".to_string(),
            bytes: 10485760,
        },
    ];

    let plan = DeletionPlan::new(
        vec![PathBuf::from("github://")],
        false,
        false,
        items,
        vec![],
    );

    let start_time = 0;
    let mut results = Vec::new();

    for item in &plan.items {
        let res = if let Some(target) = GithubTarget::parse(&item.path) {
            let delete_result = match &target {
                GithubTarget::Repo { owner, repo } => {
                    osx_clnr::integration::github::delete_repository(&mock, owner, repo)
                }
                GithubTarget::Branch {
                    owner,
                    repo,
                    branch,
                } => osx_clnr::integration::github::delete_branch(&mock, owner, repo, branch),
                GithubTarget::Run {
                    owner,
                    repo,
                    run_id,
                } => osx_clnr::integration::github::delete_run(&mock, owner, repo, *run_id),
                GithubTarget::Release { owner, repo, tag } => {
                    osx_clnr::integration::github::delete_release(&mock, owner, repo, tag)
                }
                GithubTarget::Cache {
                    owner,
                    repo,
                    cache_id,
                    ..
                } => osx_clnr::integration::github::delete_cache(&mock, owner, repo, *cache_id),
                GithubTarget::Issue {
                    owner,
                    repo,
                    number,
                } => osx_clnr::integration::github::close_issue(&mock, owner, repo, *number),
                GithubTarget::Pr {
                    owner,
                    repo,
                    number,
                } => osx_clnr::integration::github::close_pr(&mock, owner, repo, *number),
                GithubTarget::ReleaseAsset {
                    owner,
                    repo,
                    asset_id,
                    ..
                } => osx_clnr::integration::github::delete_release_asset(
                    &mock, owner, repo, *asset_id,
                ),
            };
            match delete_result {
                Ok(()) => {
                    let bytes_freed = match target {
                        GithubTarget::Cache { .. } | GithubTarget::ReleaseAsset { .. } => {
                            item.bytes
                        }
                        _ => 0,
                    };
                    DeletionResult {
                        path: item.path.clone(),
                        status: DeletionStatus::Deleted,
                        error: None,
                        blake3_hash: None,
                        bytes_freed,
                    }
                }
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

    assert_eq!(receipt.execution_record.results.len(), 8);
    for (i, r) in receipt.execution_record.results.iter().enumerate() {
        assert_eq!(r.status, DeletionStatus::Deleted);
        assert!(r.error.is_none());
        if i == 4 {
            assert_eq!(r.bytes_freed, 5000);
        } else if i == 7 {
            assert_eq!(r.bytes_freed, 10485760);
        } else {
            assert_eq!(r.bytes_freed, 0);
        }
    }

    let calls = mock.get_calls();
    assert_eq!(calls.len(), 8);
    assert_eq!(
        calls[0],
        vec![
            "gh".to_string(),
            "repo".to_string(),
            "delete".to_string(),
            "my-org/empty-repo".to_string(),
            "--yes".to_string()
        ]
    );
    assert_eq!(
        calls[1],
        vec![
            "gh".to_string(),
            "run".to_string(),
            "delete".to_string(),
            "111".to_string(),
            "--repo".to_string(),
            "my-org/active-repo".to_string()
        ]
    );
    assert_eq!(
        calls[2],
        vec![
            "gh".to_string(),
            "api".to_string(),
            "-X".to_string(),
            "DELETE".to_string(),
            "repos/my-org/active-repo/git/refs/heads/feature/merged".to_string()
        ]
    );
    assert_eq!(
        calls[3],
        vec![
            "gh".to_string(),
            "release".to_string(),
            "delete".to_string(),
            "v1.0.0-draft".to_string(),
            "--repo".to_string(),
            "my-org/active-repo".to_string(),
            "--yes".to_string(),
            "--cleanup-tag".to_string()
        ]
    );
    assert_eq!(
        calls[4],
        vec![
            "gh".to_string(),
            "cache".to_string(),
            "delete".to_string(),
            "333".to_string(),
            "--repo".to_string(),
            "my-org/active-repo".to_string()
        ]
    );
    assert_eq!(
        calls[5],
        vec![
            "gh".to_string(),
            "issue".to_string(),
            "close".to_string(),
            "12".to_string(),
            "--repo".to_string(),
            "my-org/active-repo".to_string()
        ]
    );
    assert_eq!(
        calls[6],
        vec![
            "gh".to_string(),
            "pr".to_string(),
            "close".to_string(),
            "24".to_string(),
            "--repo".to_string(),
            "my-org/active-repo".to_string()
        ]
    );
    assert_eq!(
        calls[7],
        vec![
            "gh".to_string(),
            "api".to_string(),
            "-X".to_string(),
            "DELETE".to_string(),
            "repos/my-org/active-repo/releases/assets/555".to_string()
        ]
    );
}

#[test]
fn test_github_zero_thresholds() {
    let mock = MockCommandExecutor::new();

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
            "visibility": "PUBLIC",
            "defaultBranchRef": null
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
            "name,nameWithOwner,owner,isArchived,isFork,isEmpty,pushedAt,updatedAt,createdAt,diskUsage,visibility,defaultBranchRef",
        ],
        0,
        repo_list_json,
        "",
    );

    let candidates = discover_candidates(&mock, 0, 0, 0, 0, 0, 0, 0).unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(
        candidates[0].path,
        PathBuf::from("github://repo/my-org/active-repo")
    );
    assert_eq!(
        candidates[0].reason,
        "Stale repository (inactive for > 0 days)"
    );
}

#[test]
fn test_github_custom_default_branch_heuristic_issue() {
    let mock = MockCommandExecutor::new();

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
            "visibility": "PUBLIC",
            "defaultBranchRef": {
                "name": "develop"
            }
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
            "name,nameWithOwner,owner,isArchived,isFork,isEmpty,pushedAt,updatedAt,createdAt,diskUsage,visibility,defaultBranchRef",
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
            "1000",
            "--json",
            "databaseId,number,name,headBranch,status,conclusion,createdAt,updatedAt",
        ],
        0,
        "[]",
        "",
    );

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
        &[
            "api",
            "repos/my-org/active-repo/branches?per_page=100&page=1",
        ],
        0,
        branch_list_json,
        "",
    );

    let compare_develop_json =
        r#"{"status": "identical", "ahead_by": 0, "behind_by": 0, "total_commits": 0}"#;
    mock.add_response(
        "gh",
        &[
            "api",
            "repos/my-org/active-repo/compare/develop...feature/merged",
        ],
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
            "1000",
            "--json",
            "tagName,name,isDraft,isPrerelease,createdAt,publishedAt,assets",
        ],
        0,
        "[]",
        "",
    );

    // No caches
    mock.add_response(
        "gh",
        &[
            "cache",
            "list",
            "--repo",
            "my-org/active-repo",
            "--limit",
            "1000",
            "--json",
            "id,key,sizeInBytes,createdAt,lastAccessedAt",
        ],
        0,
        "[]",
        "",
    );

    // No issues
    mock.add_response(
        "gh",
        &[
            "issue",
            "list",
            "--repo",
            "my-org/active-repo",
            "--state",
            "all",
            "--limit",
            "1000",
            "--json",
            "number,title,state,createdAt,updatedAt,labels",
        ],
        0,
        "[]",
        "",
    );

    // No PRs
    mock.add_response(
        "gh",
        &[
            "pr",
            "list",
            "--repo",
            "my-org/active-repo",
            "--state",
            "all",
            "--limit",
            "1000",
            "--json",
            "number,title,state,createdAt,updatedAt,labels",
        ],
        0,
        "[]",
        "",
    );

    let candidates = discover_candidates(&mock, 180, 30, 30, 30, 30, 30, 5).unwrap();

    assert_eq!(candidates.len(), 1);
    assert_eq!(
        candidates[0].path,
        PathBuf::from("github://branch/my-org/active-repo/feature/merged")
    );
    assert_eq!(
        candidates[0].reason,
        "Branch fully merged into default branch (develop)"
    );
}

#[test]
fn test_github_confirmation_and_refused() {
    let plan_dir = tempfile::tempdir().unwrap();
    let plan_path = plan_dir.path().join("plan.json");
    let receipt_path = plan_dir.path().join("receipt.json");

    let items = vec![PlanItem {
        path: PathBuf::from("github://repo/my-org/empty-repo"),
        kind: PlanItemKind::GithubRepo,
        reason: "Empty repository".to_string(),
        bytes: 0,
    }];

    let plan = DeletionPlan::new(
        vec![PathBuf::from("github://")],
        false,
        false,
        items,
        vec![],
    );
    let plan_json = serde_json::to_string_pretty(&plan).unwrap();
    std::fs::write(&plan_path, plan_json).unwrap();

    // Since stdin is not a tty in test runner, dialoguer returns false.
    // So the item should be refused.
    handle(GithubAction::Delete {
        plan: plan_path.clone(),
        receipt: receipt_path.clone(),
        yes: false,
    })
    .unwrap();

    // Verify receipt has status Refused
    let receipt_content = std::fs::read_to_string(&receipt_path).unwrap();
    let receipt: DeletionReceipt = serde_json::from_str(&receipt_content).unwrap();
    assert_eq!(receipt.execution_record.results.len(), 1);
    assert_eq!(
        receipt.execution_record.results[0].status,
        DeletionStatus::Refused
    );
    assert_eq!(
        receipt.execution_record.results[0].error,
        Some("Deletion refused by user".to_string())
    );
}

#[test]
fn test_github_protection_markers() {
    use osx_clnr::domain::github::{is_issue_stale, is_pr_stale, GhIssue, GhLabel, GhPr};

    // 1. Issue protection checks
    let base_issue = GhIssue {
        number: 42,
        title: "A bug".into(),
        state: "OPEN".into(),
        created_at: "2026-05-10T12:00:00Z".into(),
        updated_at: "2026-05-10T12:00:00Z".into(),
        labels: vec![],
    };

    // Default stale issue
    assert!(is_issue_stale(&base_issue, 30, "2026-06-14T12:00:00Z"));

    // Stale issue with irrelevant label
    let mut bug_issue = base_issue.clone();
    bug_issue.labels.push(GhLabel { name: "bug".into() });
    assert!(is_issue_stale(&bug_issue, 30, "2026-06-14T12:00:00Z"));

    // Protected labels: keep, pinned, no-stale, critical (case insensitive)
    for protected_name in &[
        "keep", "pinned", "no-stale", "critical", "KeEp", "PINNED", "no-STALE", "CRITICAL",
    ] {
        let mut protected_issue = base_issue.clone();
        protected_issue.labels.push(GhLabel {
            name: protected_name.to_string(),
        });
        assert!(
            !is_issue_stale(&protected_issue, 30, "2026-06-14T12:00:00Z"),
            "Issue with label '{}' should not be stale",
            protected_name
        );
    }

    // 2. PR protection checks
    let base_pr = GhPr {
        number: 101,
        title: "A feature".into(),
        state: "OPEN".into(),
        created_at: "2026-05-10T12:00:00Z".into(),
        updated_at: "2026-05-10T12:00:00Z".into(),
        labels: vec![],
    };

    // Default stale PR
    assert!(is_pr_stale(&base_pr, 30, "2026-06-14T12:00:00Z"));

    // Stale PR with irrelevant label
    let mut refactor_pr = base_pr.clone();
    refactor_pr.labels.push(GhLabel {
        name: "refactor".into(),
    });
    assert!(is_pr_stale(&refactor_pr, 30, "2026-06-14T12:00:00Z"));

    // Protected labels
    for protected_name in &[
        "keep", "pinned", "no-stale", "critical", "KeEp", "PINNED", "no-STALE", "CRITICAL",
    ] {
        let mut protected_pr = base_pr.clone();
        protected_pr.labels.push(GhLabel {
            name: protected_name.to_string(),
        });
        assert!(
            !is_pr_stale(&protected_pr, 30, "2026-06-14T12:00:00Z"),
            "PR with label '{}' should not be stale",
            protected_name
        );
    }
}

#[test]
fn test_github_execute_delete_plan_helper_success() {
    use osx_clnr::nouns::github::execute_delete_plan_helper;
    let mock = MockCommandExecutor::new();
    // Register mock responses for all 8 target types
    mock.add_response(
        "gh",
        &["repo", "delete", "my-org/empty-repo", "--yes"],
        0,
        "",
        "",
    );
    mock.add_response(
        "gh",
        &["run", "delete", "111", "--repo", "my-org/active-repo"],
        0,
        "",
        "",
    );
    mock.add_response(
        "gh",
        &[
            "api",
            "-X",
            "DELETE",
            "repos/my-org/active-repo/git/refs/heads/feature/merged",
        ],
        0,
        "",
        "",
    );
    mock.add_response(
        "gh",
        &[
            "release",
            "delete",
            "v1.0.0-draft",
            "--repo",
            "my-org/active-repo",
            "--yes",
            "--cleanup-tag",
        ],
        0,
        "",
        "",
    );
    mock.add_response(
        "gh",
        &["cache", "delete", "333", "--repo", "my-org/active-repo"],
        0,
        "",
        "",
    );
    mock.add_response(
        "gh",
        &["issue", "close", "12", "--repo", "my-org/active-repo"],
        0,
        "",
        "",
    );
    mock.add_response(
        "gh",
        &["pr", "close", "24", "--repo", "my-org/active-repo"],
        0,
        "",
        "",
    );
    mock.add_response(
        "gh",
        &[
            "api",
            "-X",
            "DELETE",
            "repos/my-org/active-repo/releases/assets/555",
        ],
        0,
        "",
        "",
    );

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
        PlanItem {
            path: PathBuf::from("github://cache/my-org/active-repo/333/stale-cache"),
            kind: PlanItemKind::GithubCache,
            reason: "Stale cache".to_string(),
            bytes: 5000,
        },
        PlanItem {
            path: PathBuf::from("github://issue/my-org/active-repo/12"),
            kind: PlanItemKind::GithubIssue,
            reason: "Stale issue".to_string(),
            bytes: 0,
        },
        PlanItem {
            path: PathBuf::from("github://pr/my-org/active-repo/24"),
            kind: PlanItemKind::GithubPr,
            reason: "Stale PR".to_string(),
            bytes: 0,
        },
        PlanItem {
            path: PathBuf::from("github://release-asset/my-org/active-repo/555/large-asset.zip"),
            kind: PlanItemKind::GithubReleaseAsset,
            reason: "Large asset".to_string(),
            bytes: 10485760,
        },
    ];

    let plan = DeletionPlan::new(
        vec![PathBuf::from("github://")],
        false,
        false,
        items,
        vec![],
    );

    let plan_dir = tempfile::tempdir().unwrap();
    let receipt_path = plan_dir.path().join("receipt.json");

    let res = execute_delete_plan_helper(&mock, plan, receipt_path.clone(), true).unwrap();
    assert_eq!(res["status"], "success");
    assert_eq!(res["deleted_count"], 8);
    assert_eq!(res["failed_count"], 0);

    // Verify written receipt
    let receipt_content = std::fs::read_to_string(&receipt_path).unwrap();
    let receipt: DeletionReceipt = serde_json::from_str(&receipt_content).unwrap();
    assert_eq!(receipt.execution_record.results.len(), 8);
    for r in &receipt.execution_record.results {
        assert_eq!(r.status, DeletionStatus::Deleted);
    }
}

#[test]
fn test_github_execute_delete_plan_helper_failures() {
    use osx_clnr::nouns::github::execute_delete_plan_helper;
    let mock = MockCommandExecutor::new();

    // Target 1: Repo deletion returns status 1, generic error
    mock.add_response(
        "gh",
        &["repo", "delete", "my-org/repo-fail", "--yes"],
        1,
        "",
        "internal server error",
    );

    // Target 2: Branch deletion returns status 1, "not found" in stderr
    mock.add_response(
        "gh",
        &[
            "api",
            "-X",
            "DELETE",
            "repos/my-org/active-repo/git/refs/heads/feature/missing",
        ],
        1,
        "",
        "branch not found",
    );

    // Target 3: Issue close returns status 1, "404" in stderr
    mock.add_response(
        "gh",
        &["issue", "close", "999", "--repo", "my-org/active-repo"],
        1,
        "",
        "HTTP 404: Not Found",
    );

    let items = vec![
        // Target 1: Generic failure
        PlanItem {
            path: PathBuf::from("github://repo/my-org/repo-fail"),
            kind: PlanItemKind::GithubRepo,
            reason: "Failure repo".to_string(),
            bytes: 0,
        },
        // Target 2: Not found branch
        PlanItem {
            path: PathBuf::from("github://branch/my-org/active-repo/feature/missing"),
            kind: PlanItemKind::GithubBranch,
            reason: "Missing branch".to_string(),
            bytes: 0,
        },
        // Target 3: 404 issue
        PlanItem {
            path: PathBuf::from("github://issue/my-org/active-repo/999"),
            kind: PlanItemKind::GithubIssue,
            reason: "Missing issue".to_string(),
            bytes: 0,
        },
        // Target 4: Non-GitHub URI (Should be SkippedMissing)
        PlanItem {
            path: PathBuf::from("/Users/user/some-local-file"),
            kind: PlanItemKind::Dir,
            reason: "Local file".to_string(),
            bytes: 100,
        },
        // Target 5: Invalid GitHub URI (Should be Failed)
        PlanItem {
            path: PathBuf::from("github://invalid"),
            kind: PlanItemKind::GithubRepo,
            reason: "Invalid URI".to_string(),
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

    let plan_dir = tempfile::tempdir().unwrap();
    let receipt_path = plan_dir.path().join("receipt.json");

    let res = execute_delete_plan_helper(&mock, plan, receipt_path.clone(), true).unwrap();
    assert_eq!(res["status"], "success");
    assert_eq!(res["deleted_count"], 0);
    assert_eq!(res["failed_count"], 2); // Target 1 and Target 5
    assert_eq!(res["skipped_count"], 3); // Target 2, Target 3, and Target 4

    // Verify written receipt
    let receipt_content = std::fs::read_to_string(&receipt_path).unwrap();
    let receipt: DeletionReceipt = serde_json::from_str(&receipt_content).unwrap();

    assert_eq!(
        receipt.execution_record.results[0].status,
        DeletionStatus::Failed
    );
    assert!(receipt.execution_record.results[0]
        .error
        .as_ref()
        .unwrap()
        .contains("internal server error"));

    assert_eq!(
        receipt.execution_record.results[1].status,
        DeletionStatus::SkippedMissing
    );
    assert!(receipt.execution_record.results[1]
        .error
        .as_ref()
        .unwrap()
        .contains("branch not found"));

    assert_eq!(
        receipt.execution_record.results[2].status,
        DeletionStatus::SkippedMissing
    );
    assert!(receipt.execution_record.results[2]
        .error
        .as_ref()
        .unwrap()
        .contains("HTTP 404: Not Found"));

    assert_eq!(
        receipt.execution_record.results[3].status,
        DeletionStatus::SkippedMissing
    );
    assert!(receipt.execution_record.results[3]
        .error
        .as_ref()
        .unwrap()
        .contains("Non-GitHub item skipped"));

    assert_eq!(
        receipt.execution_record.results[4].status,
        DeletionStatus::Failed
    );
    assert!(receipt.execution_record.results[4]
        .error
        .as_ref()
        .unwrap()
        .contains("Invalid github:// URI"));
}

#[test]
fn test_github_list_branches_paging() {
    let mock = MockCommandExecutor::new();

    // Generate 100 branch items for page 1
    let mut page1_items = Vec::new();
    for i in 1..=100 {
        page1_items.push(serde_json::json!({
            "name": format!("branch-{}", i),
            "commit": {
                "sha": format!("sha-{}", i),
                "url": format!("url-{}", i),
            },
            "protected": false
        }));
    }
    let page1_json = serde_json::to_string(&page1_items).unwrap();

    // Generate 50 branch items for page 2
    let mut page2_items = Vec::new();
    for i in 101..=150 {
        page2_items.push(serde_json::json!({
            "name": format!("branch-{}", i),
            "commit": {
                "sha": format!("sha-{}", i),
                "url": format!("url-{}", i),
            },
            "protected": false
        }));
    }
    let page2_json = serde_json::to_string(&page2_items).unwrap();

    mock.add_response(
        "gh",
        &["api", "repos/my-org/my-repo/branches?per_page=100&page=1"],
        0,
        &page1_json,
        "",
    );

    mock.add_response(
        "gh",
        &["api", "repos/my-org/my-repo/branches?per_page=100&page=2"],
        0,
        &page2_json,
        "",
    );

    let branches =
        osx_clnr::integration::github::list_branches(&mock, "my-org", "my-repo").unwrap();
    assert_eq!(branches.len(), 150);
    assert_eq!(branches[0].name, "branch-1");
    assert_eq!(branches[99].name, "branch-100");
    assert_eq!(branches[100].name, "branch-101");
    assert_eq!(branches[149].name, "branch-150");
}

#[test]
fn test_github_integration_failures() {
    let mock = MockCommandExecutor::new();

    // Setup failures for list commands
    mock.add_response(
        "gh",
        &[
            "repo",
            "list",
            "--limit",
            "1000",
            "--json",
            "name,nameWithOwner,owner,isArchived,isFork,isEmpty,pushedAt,updatedAt,createdAt,diskUsage,visibility,defaultBranchRef",
        ],
        1,
        "",
        "authentication failed",
    );

    let err = osx_clnr::integration::github::list_repositories(&mock).unwrap_err();
    assert!(err
        .to_string()
        .contains("gh command failed: authentication failed"));
}
