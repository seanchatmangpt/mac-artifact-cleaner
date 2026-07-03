//! Deletion plan representation and build logic.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::domain::{
    artifact::Candidate,
    crypto::{hash_bytes, hmac_sha256_hex, verify_hmac_sha256},
    tool_roots::ToolRootReport,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeletionPlan {
    pub version: u32,
    pub created_unix: u64,
    pub roots: Vec<PathBuf>,
    pub deps: bool,
    pub aggressive: bool,
    pub items: Vec<PlanItem>,
    pub tool_roots: Vec<ToolRootReport>,
    /// Approval record binding this plan's content to a specific reviewer.
    /// `None` means the plan has never been approved. Populated by
    /// `plan_approve`, persisted into the plan file on disk, and re-verified
    /// by `delete execute` before any deletion runs — a plan whose content
    /// hash no longer matches `PlanApproval::plan_hash` (because it was
    /// hand-edited, or items were substituted after signing) is refused.
    #[serde(default)]
    pub approval: Option<PlanApproval>,
}

/// Approval record embedded in a deletion plan file.
///
/// `plan_hash` is a content hash of the plan's substantive fields (roots,
/// deps, aggressive, items, tool_roots) computed *excluding* this
/// `approval` field itself, so it can be recomputed from the plan at any
/// later point and compared to detect tampering.
/// `hmac_signature` is the actual security boundary: an HMAC-SHA256 over
/// `plan_hash` (plus `approver`/`approval_reason`), keyed with a secret that
/// is never present in the plan file itself (see
/// `integration::config::approval_secret`). `plan_hash` alone is a plain,
/// unkeyed content hash — trivially reproducible by anyone who can read the
/// plan file, so it is retained only as a human-readable diagnostic of *what*
/// was signed, never as the check that gates deletion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanApproval {
    pub approver: String,
    pub approval_reason: String,
    pub approved_at_unix: i64,
    pub plan_hash: String,
    /// Hex-encoded HMAC-SHA256(secret, plan_hash || approver || approval_reason).
    /// `#[serde(default)]` keeps plans signed before this field existed
    /// loadable, but such plans have an empty signature and therefore always
    /// fail [`DeletionPlan::verify_approval`] — they must be re-approved.
    #[serde(default)]
    pub hmac_signature: String,
}

impl PlanApproval {
    /// Builds the exact byte string that is HMAC-signed / verified, binding
    /// the signature to the plan's content hash *and* to who approved it and
    /// why (so swapping the approver name on an otherwise-valid signature
    /// doesn't verify).
    fn signing_input(plan_hash: &str, approver: &str, approval_reason: &str) -> Vec<u8> {
        format!("{plan_hash}:{approver}:{approval_reason}").into_bytes()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanItem {
    pub path: PathBuf,
    pub kind: PlanItemKind,
    pub reason: String,
    /// Physical disk allocation of this item in bytes (blocks × 512), so the plan
    /// shows reclaim impact before deletion. `#[serde(default)]` keeps plans
    /// written before this field was added loadable.
    #[serde(default)]
    pub bytes: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PlanItemKind {
    File,
    Dir,
    GithubRepo,
    GithubBranch,
    GithubRun,
    GithubRelease,
    GithubCache,
    GithubIssue,
    GithubPr,
    GithubReleaseAsset,
}

impl DeletionPlan {
    /// Creates a new deletion plan.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::plan::{DeletionPlan, PlanItem, PlanItemKind};
    /// use std::path::PathBuf;
    ///
    /// let items = vec![PlanItem {
    ///     path: PathBuf::from("/Users/user/dev/project/target"),
    ///     kind: PlanItemKind::Dir,
    ///     reason: "rust target".to_string(),
    ///     bytes: 0,
    /// }];
    /// let plan = DeletionPlan::new(
    ///     vec![PathBuf::from("/Users/user")],
    ///     false,
    ///     true,
    ///     items,
    ///     vec![],
    /// );
    /// assert_eq!(plan.version, 1);
    /// assert_eq!(plan.items.len(), 1);
    /// ```
    pub fn new(
        roots: Vec<PathBuf>,
        deps: bool,
        aggressive: bool,
        items: Vec<PlanItem>,
        tool_roots: Vec<ToolRootReport>,
    ) -> Self {
        let created_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            version: 1,
            created_unix,
            roots,
            deps,
            aggressive,
            items,
            tool_roots,
            approval: None,
        }
    }

    /// Computes a stable content hash over the plan's substantive fields,
    /// deliberately excluding `approval` itself so the hash can be verified
    /// against the field it lives next to.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::plan::{DeletionPlan, PlanItem, PlanItemKind};
    /// use std::path::PathBuf;
    ///
    /// let plan = DeletionPlan::new(
    ///     vec![PathBuf::from("/Users/user")],
    ///     false,
    ///     true,
    ///     vec![PlanItem {
    ///         path: PathBuf::from("/Users/user/dev/project/target"),
    ///         kind: PlanItemKind::Dir,
    ///         reason: "rust target".to_string(),
    ///         bytes: 0,
    ///     }],
    ///     vec![],
    /// );
    ///
    /// // Positive: hash is deterministic for identical content.
    /// assert_eq!(plan.content_hash(), plan.content_hash());
    ///
    /// // Negative: appending an item (tampering) changes the hash.
    /// let mut tampered = plan.clone();
    /// tampered.items.push(PlanItem {
    ///     path: PathBuf::from("/Users/user/injected"),
    ///     kind: PlanItemKind::Dir,
    ///     reason: "injected".to_string(),
    ///     bytes: 0,
    /// });
    /// assert_ne!(plan.content_hash(), tampered.content_hash());
    /// ```
    pub fn content_hash(&self) -> String {
        let mut unsigned = self.clone();
        unsigned.approval = None;
        let bytes = serde_json::to_vec(&unsigned).unwrap_or_default();
        hash_bytes(&bytes)
    }

    /// Produces a [`PlanApproval`] for this plan's *current* content,
    /// keyed-signed with `secret`.
    ///
    /// This is a pure function: it does not read the secret from anywhere —
    /// the caller (integration layer / MCP server) is responsible for
    /// sourcing `secret` from an environment variable or machine-local key
    /// file and passing the bytes in.
    ///
    /// ```
    /// use osx_clnr::domain::plan::{DeletionPlan, PlanItem, PlanItemKind};
    /// use std::path::PathBuf;
    ///
    /// let plan = DeletionPlan::new(
    ///     vec![PathBuf::from("/Users/user")],
    ///     false,
    ///     true,
    ///     vec![PlanItem {
    ///         path: PathBuf::from("/Users/user/dev/project/target"),
    ///         kind: PlanItemKind::Dir,
    ///         reason: "rust target".to_string(),
    ///         bytes: 0,
    ///     }],
    ///     vec![],
    /// );
    ///
    /// let approval = plan.sign_approval(b"real-secret", "alice", "cleanup");
    /// let mut signed = plan.clone();
    /// signed.approval = Some(approval);
    /// assert!(signed.verify_approval(b"real-secret").is_ok());
    ///
    /// // Refusal: verifying with a different secret than the one used to
    /// // sign fails, even though `plan_hash` still matches the content.
    /// assert!(signed.verify_approval(b"wrong-secret").is_err());
    /// ```
    pub fn sign_approval(
        &self,
        secret: &[u8],
        approver: &str,
        approval_reason: &str,
    ) -> PlanApproval {
        let plan_hash = self.content_hash();
        let signing_input = PlanApproval::signing_input(&plan_hash, approver, approval_reason);
        let hmac_signature = hmac_sha256_hex(secret, &signing_input);
        let approved_at_unix = chrono::Utc::now().timestamp();
        PlanApproval {
            approver: approver.to_string(),
            approval_reason: approval_reason.to_string(),
            approved_at_unix,
            plan_hash,
            hmac_signature,
        }
    }

    /// Verifies that this plan carries an approval whose recorded
    /// `plan_hash` matches the plan's current content hash, **and** whose
    /// `hmac_signature` is a genuine HMAC-SHA256 over that hash keyed with
    /// `secret`.
    ///
    /// The plain `plan_hash` comparison alone is *not* a security boundary —
    /// it is a BLAKE3 hash with no secret, so anyone who can write to the
    /// plan file can recompute it offline and forge an "approval" block
    /// without ever calling `plan_approve`. The `hmac_signature` check is
    /// what actually requires possession of `secret` (sourced from an
    /// environment variable or machine-local key file by the caller — never
    /// from the plan file itself).
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::plan::{DeletionPlan, PlanApproval, PlanItem, PlanItemKind};
    /// use std::path::PathBuf;
    ///
    /// let secret = b"the-real-secret";
    ///
    /// let mut plan = DeletionPlan::new(
    ///     vec![PathBuf::from("/Users/user")],
    ///     false,
    ///     true,
    ///     vec![PlanItem {
    ///         path: PathBuf::from("/Users/user/dev/project/target"),
    ///         kind: PlanItemKind::Dir,
    ///         reason: "rust target".to_string(),
    ///         bytes: 0,
    ///     }],
    ///     vec![],
    /// );
    ///
    /// // Refusal: unapproved plan (no approval field at all) is rejected.
    /// assert!(plan.verify_approval(secret).is_err());
    ///
    /// // Refusal: an attacker who can only write the plan file, and does not
    /// // know `secret`, hand-computes the *plain* content hash (exactly the
    /// // forgery the verifier demonstrated) and writes it in as `plan_hash`
    /// // with an empty/guessed `hmac_signature`. This must be rejected.
    /// let forged_hash = plan.content_hash();
    /// plan.approval = Some(PlanApproval {
    ///     approver: "attacker".to_string(),
    ///     approval_reason: "self-approved".to_string(),
    ///     approved_at_unix: 0,
    ///     plan_hash: forged_hash,
    ///     hmac_signature: String::new(),
    /// });
    /// assert!(plan.verify_approval(secret).is_err());
    ///
    /// // Positive: a real signature produced by `sign_approval` with the
    /// // correct secret verifies.
    /// let approval = plan.sign_approval(secret, "alice", "cleanup");
    /// plan.approval = Some(approval);
    /// assert!(plan.verify_approval(secret).is_ok());
    ///
    /// // Refusal: verifying with the wrong secret (e.g. an attacker's guess)
    /// // fails even though `plan_hash` matches content exactly.
    /// assert!(plan.verify_approval(b"wrong-guess").is_err());
    ///
    /// // Refusal: mutating the plan after approval invalidates the
    /// // signature, even if the approval field is left untouched (e.g. an
    /// // attacker substituted an item's path after signing).
    /// plan.items[0].path = PathBuf::from("/Users/user/totally-different-dir");
    /// assert!(plan.verify_approval(secret).is_err());
    /// ```
    pub fn verify_approval(&self, secret: &[u8]) -> Result<(), String> {
        match &self.approval {
            None => Err("plan has not been approved via plan_approve".to_string()),
            Some(approval) => {
                let expected_hash = self.content_hash();
                if approval.plan_hash != expected_hash {
                    return Err("plan approval does not match plan content — the plan was \
                                modified after approval and must be re-approved"
                        .to_string());
                }

                let signing_input = PlanApproval::signing_input(
                    &expected_hash,
                    &approval.approver,
                    &approval.approval_reason,
                );
                if verify_hmac_sha256(secret, &signing_input, &approval.hmac_signature) {
                    Ok(())
                } else {
                    Err("plan approval HMAC signature is invalid — the approval block was not \
                         produced by plan_approve with the correct approval secret (forged or \
                         signed with the wrong key)"
                        .to_string())
                }
            }
        }
    }
}

/// Extracts a sorted, deduplicated list of directories from a deletion plan
/// that are candidates for Time Machine exclusion (e.g., rebuildable project directories
/// or tool root cache directories).
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::plan::{DeletionPlan, PlanItem, PlanItemKind, extract_exclusion_candidates};
/// use osx_clnr::domain::tool_roots::ToolRootReport;
/// use std::path::PathBuf;
///
/// let items = vec![
///     PlanItem {
///         path: PathBuf::from("/Users/user/dev/project/target"),
///         kind: PlanItemKind::Dir,
///         reason: "rust target".to_string(),
///         bytes: 0,
///     },
///     PlanItem {
///         path: PathBuf::from("/Users/user/dev/project/src/main.rs"),
///         kind: PlanItemKind::File,
///         reason: "source file".to_string(),
///         bytes: 0,
///     },
/// ];
/// let tool_roots = vec![
///     ToolRootReport {
///         path: "/Users/user/.npm".to_string(),
///         category: "node_package_cache".to_string(),
///         bytes: 1000,
///         human: "1KB".to_string(),
///         files: 10,
///         dirs: 2,
///         created_unix: None,
///         last_accessed_unix: None,
///         last_modified_unix: None,
///         metadata_changed_unix: None,
///         newest_descendant_modified_unix: None,
///         newest_descendant_path: None,
///         days_since_modified: None,
///         days_since_accessed: None,
///         days_since_newest_descendant_modified: None,
///         recommendation: "cleanup_candidate".to_string(),
///         rationale: "cache".to_string(),
///     }
/// ];
/// let plan = DeletionPlan::new(
///     vec![PathBuf::from("/Users/user")],
///     false,
///     true,
///     items,
///     tool_roots,
/// );
///
/// let candidates = extract_exclusion_candidates(&plan);
/// assert_eq!(candidates.len(), 2);
/// assert_eq!(candidates[0].path, PathBuf::from("/Users/user/.npm"));
/// assert_eq!(candidates[1].path, PathBuf::from("/Users/user/dev/project/target"));
/// ```
pub fn extract_exclusion_candidates(plan: &DeletionPlan) -> Vec<Candidate> {
    let mut candidates: Vec<Candidate> = plan
        .items
        .iter()
        .filter(|item| matches!(item.kind, PlanItemKind::Dir))
        .map(|item| Candidate { path: item.path.clone(), reason: item.reason.clone() })
        .collect();

    for tr in &plan.tool_roots {
        if tr.category.ends_with("_cache")
            || tr.category.ends_with("_caches")
            || tr.recommendation == "cleanup_candidate"
        {
            candidates.push(Candidate {
                path: PathBuf::from(&tr.path),
                reason: format!("Tool root cache: {}", tr.category),
            });
        }
    }

    // Deduplicate
    let mut seen = std::collections::HashSet::new();
    candidates.retain(|c| seen.insert(c.path.clone()));

    // Sort path-wise
    candidates.sort_by(|a, b| a.path.cmp(&b.path));

    candidates
}
