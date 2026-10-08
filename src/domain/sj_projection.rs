//! Machine `sj:` projection of r-receipts — the declared mapping in
//! `docs/sjira-mapping.md`, implemented 1:1.
//!
//! [`SjWorkOrderRecord`] carries the semantic-jira pack's `sj:` field names;
//! [`from_r_receipt`] is the total 1:1 projection from [`RReceipt`]; [`to_sj_ttl`]
//! emits deterministic Turtle (records in input order, fields in fixed order,
//! no timestamps). The native receipt stays the source of truth; the r-projection
//! references it by `output_sha256`; this is the fleet `sj:` shape on top.
//! Zero `std::fs`/`std::process` here.

use crate::domain::r_projection::{RCommand, RReceipt};

/// Constant naming the semantic-jira refusal-registry seam that every
/// `REFUSED(...)` standing is recorded against (docs/sjira-mapping.md,
/// "REFUSED(no-work-order) → sj: Refusal Registry Seam").
pub const SJ_REFUSAL_REGISTRY_SEAM: &str = "sj:refusal-registry";

/// The `sj:` refusal registry entry class for `REFUSED(no-work-order)`
/// (`broken_term = R_missing_authority`, `r_projection.rs:152`).
pub const SJ_REFUSAL_NO_WORK_ORDER: &str = "REFUSED(no-work-order)";

/// Machine record in the semantic-jira pack's `sj:` vocabulary. Field names
/// mirror the sj: terms 1:1 (docs/sjira-mapping.md equivalence table).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SjWorkOrderRecord {
    /// `sj:subject` — identity.subject, qualified by identity.repo.
    pub subject: String,
    /// `sj:repo` — the provenance checkout qualifying the subject.
    pub repo: String,
    /// `sj:baseSha` — identity.base_sha (== subject_sha locally).
    pub base_sha: String,
    /// `sj:authorityCeiling` — literal `"DO"`.
    pub authority_ceiling: String,
    /// `sj:grant`.
    pub grant: String,
    /// `sj:actor`.
    pub actor: String,
    /// `sj:replayIdentity` — replay.commands' `output_sha256` joined to the
    /// native receipt + its `durable_location`.
    pub replay_identity: String,
    /// `sj:standing` — `ALIVE` / `PARTIAL_ALIVE` / `BLOCKED:` / `REFUSED(...)`.
    pub standing: String,
    /// `sj:brokenTerm` — the typed refusal term, when standing is a refusal.
    pub broken_term: Option<String>,
    /// `sj:workOrder` — the admitted order this execution discharged.
    pub work_order: String,
    /// `sj:originAuthority` — identical to authority (no intermediate relay).
    pub origin_authority_ceiling: String,
    /// `sj:provider` / `sj:providerExecutionId`.
    pub provider: String,
    pub provider_execution_id: String,
    /// `sj:receipt` payload commitment: the affidavit chain_hash
    /// (blake3, hex) over the exact consequence payload bytes. `None` until
    /// sealed by `affidavit_integration`.
    pub chain_hash: Option<String>,
    /// The refusal-registry seam this record's refusal (if any) is registered
    /// against.
    pub refusal_registry: &'static str,
}

impl SjWorkOrderRecord {
    /// True when this record is a typed refusal that belongs in the `sj:`
    /// refusal registry.
    pub fn is_refusal(&self) -> bool {
        self.standing.starts_with("REFUSED")
    }
}

/// Total 1:1 projection `RReceipt -> SjWorkOrderRecord` per the declared
/// mapping (docs/sjira-mapping.md).
pub fn from_r_receipt(r: &RReceipt) -> SjWorkOrderRecord {
    let replay_identity = r
        .replay
        .commands
        .first()
        .map(|c: &RCommand| {
            format!("{}@{}", c.output_sha256, r.replay.durable_location)
        })
        .unwrap_or_else(|| r.replay.durable_location.clone());
    SjWorkOrderRecord {
        subject: r.identity.subject.clone(),
        repo: r.identity.repo.clone(),
        base_sha: r.identity.base_sha.clone(),
        authority_ceiling: r.authority.ceiling.clone(),
        grant: r.authority.grant.clone(),
        actor: r.authority.actor.clone(),
        replay_identity,
        standing: r.standing.value.clone(),
        broken_term: r.standing.broken_term.clone(),
        work_order: r.work_order_id.clone(),
        origin_authority_ceiling: r.origin_authority.ceiling.clone(),
        provider: r.provider.name.clone(),
        provider_execution_id: r.provider_execution_id.clone(),
        chain_hash: None,
        refusal_registry: SJ_REFUSAL_REGISTRY_SEAM,
    }
}

/// Escape a string for emission as a Turtle `"..."` literal.
fn ttl_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

/// Emit the records as deterministic Turtle: input order, fixed field order,
/// no timestamps, stable blank-free IRIs keyed by `provider_execution_id`.
pub fn to_sj_ttl(records: &[SjWorkOrderRecord]) -> String {
    let mut out = String::new();
    out.push_str("@prefix sj: <tag:osx-clnr,2026:sj:> .\n");
    for rec in records {
        let iri = format!("sj:wo/{}", ttl_escape(&rec.provider_execution_id));
        out.push_str(&format!("\n{iri} a sj:WorkOrderRecord ;\n"));
        let mut triples: Vec<String> = Vec::new();
        let mut push = |pred: &str, obj: String| {
            triples.push(format!("    sj:{pred} \"{obj}\""));
        };
        push("subject", ttl_escape(&rec.subject));
        push("repo", ttl_escape(&rec.repo));
        push("baseSha", ttl_escape(&rec.base_sha));
        push("authorityCeiling", ttl_escape(&rec.authority_ceiling));
        push("grant", ttl_escape(&rec.grant));
        push("actor", ttl_escape(&rec.actor));
        push("replayIdentity", ttl_escape(&rec.replay_identity));
        push("standing", ttl_escape(&rec.standing));
        if let Some(bt) = &rec.broken_term {
            push("brokenTerm", ttl_escape(bt));
        }
        push("workOrder", ttl_escape(&rec.work_order));
        push("originAuthorityCeiling", ttl_escape(&rec.origin_authority_ceiling));
        push("provider", ttl_escape(&rec.provider));
        push("providerExecutionId", ttl_escape(&rec.provider_execution_id));
        if let Some(ch) = &rec.chain_hash {
            push("receiptCommitment", ttl_escape(ch));
        }
        push("refusalRegistry", rec.refusal_registry.to_string());
        out.push_str(&triples.join(" ;\n"));
        out.push_str(" .\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_receipt() -> RReceipt {
        let sha40 = "a".repeat(40);
        let authority = crate::domain::r_projection::RAuthority {
            ceiling: "DO".into(),
            grant: "plan:approved".into(),
            actor: "oclnr-cli".into(),
        };
        RReceipt {
            identity: crate::domain::r_projection::RIdentity {
                subject: format!("osx-clnr@{}", &sha40[..7]),
                repo: "/src/osx-clnr".into(),
                subject_sha: sha40.clone(),
                base_sha: sha40,
            },
            authority: authority.clone(),
            consequence: crate::domain::r_projection::RConsequence {
                commits: vec![],
                files_changed: vec!["/tmp/a.log".into()],
                remote_effects: vec!["deleted 3 files".into()],
            },
            replay: crate::domain::r_projection::RReplay {
                commands: vec![crate::domain::r_projection::RCommand {
                    cmd: "osx-clnr clean --plan p1".into(),
                    cwd: "/src/osx-clnr".into(),
                    exit: 0,
                    summary: "3 reclaimed".into(),
                    output_sha256: "f".repeat(64),
                }],
                durable_location: "/tmp/clean.r.json".into(),
            },
            standing: crate::domain::r_projection::RStanding {
                value: "ALIVE".into(),
                derived_from: "replay[0] exit 0".into(),
                broken_term: None,
            },
            work_order_id: "oclnr-plan:abc123".into(),
            origin_authority: authority,
            provider: crate::domain::r_projection::RProvider {
                name: "oclnr".into(),
                transport: "local-process".into(),
                authority_ceiling: "DO".into(),
                receipt_protocol: "oclnr-native+affidavit-core/v1+r-projection/v2".into(),
            },
            provider_execution_id: format!("oclnr:sha256:{}", "f".repeat(64)),
        }
    }

    #[test]
    fn fixture_maps_to_expected_record() {
        let rec = from_r_receipt(&fixture_receipt());
        assert_eq!(rec.subject,  "osx-clnr@aaaaaaa");
        assert_eq!(rec.repo, "/src/osx-clnr");
        assert_eq!(rec.base_sha, "a".repeat(40));
        assert_eq!(rec.authority_ceiling, "DO");
        assert_eq!(rec.grant, "plan:approved");
        assert_eq!(rec.actor, "oclnr-cli");
        assert_eq!(
            rec.replay_identity,
            format!("{}@/tmp/clean.r.json", "f".repeat(64))
        );
        assert_eq!(rec.standing, "ALIVE");
        assert_eq!(rec.broken_term, None);
        assert_eq!(rec.work_order, "oclnr-plan:abc123");
        assert_eq!(rec.provider, "oclnr");
        assert_eq!(rec.chain_hash, None);
        assert!(!rec.is_refusal());
    }

    #[test]
    fn refused_no_work_order_registers_at_the_seam() {
        let mut r = fixture_receipt();
        r.standing.value = SJ_REFUSAL_NO_WORK_ORDER.into();
        r.standing.broken_term = Some("R_missing_authority".into());
        let rec = from_r_receipt(&r);
        assert!(rec.is_refusal());
        assert_eq!(rec.refusal_registry, SJ_REFUSAL_REGISTRY_SEAM);
        assert_eq!(rec.broken_term.as_deref(), Some("R_missing_authority"));
    }

    #[test]
    fn ttl_is_deterministic_and_round_trip_stable() {
        let r = fixture_receipt();
        let recs = vec![from_r_receipt(&r)];
        let t1 = to_sj_ttl(&recs);
        let t2 = to_sj_ttl(&recs);
        assert_eq!(t1, t2, "two emissions of the same records must be identical");
        assert!(t1.contains("sj:subject \"osx-clnr@aaaaaaa\""));
        assert!(t1.contains("sj:baseSha \""));
        assert!(t1.contains("sj:standing \"ALIVE\""));
        assert!(t1.contains("sj:workOrder \"oclnr-plan:abc123\""));
        assert!(t1.ends_with(" .\n"));
        // Field order is fixed: subject precedes standing precedes workOrder.
        let subject_pos = t1.find("sj:subject").unwrap();
        let standing_pos = t1.find("sj:standing").unwrap();
        let work_order_pos = t1.find("sj:workOrder").unwrap();
        assert!(subject_pos < standing_pos && standing_pos < work_order_pos);
        // Projection is total and lossless on the mapped fields: re-projecting
        // the same receipt yields the same record and the same TTL.
        let recs2 = vec![from_r_receipt(&fixture_receipt())];
        assert_eq!(recs, recs2);
        assert_eq!(to_sj_ttl(&recs2), t1);
    }

    #[test]
    fn ttl_escapes_quotes_and_newlines() {
        let mut rec = from_r_receipt(&fixture_receipt());
        rec.subject = "weird \"subject\"\nline2".into();
        let ttl = to_sj_ttl(&[rec]);
        assert!(ttl.contains("\\\"subject\\\""));
        assert!(ttl.contains("\\nline2"));
    }

    #[test]
    fn seal_closes_chain_hash_seam_deterministically() {
        let mut rec = from_r_receipt(&fixture_receipt());
        assert_eq!(rec.chain_hash, None);

        // Fresh empty base chain (no prior events).
        let empty_base = affidavit::chain::ChainAssembler::new().finalize();
        let sealed1 = crate::domain::affidavit_integration::seal_sj_record(&mut rec, &empty_base)
            .expect("empty base seals");
        assert_eq!(
            rec.chain_hash.as_deref(),
            Some(sealed1.chain_hash.as_hex()),
            "chain_hash must equal the sealed chain's rolling hash"
        );

        // Deterministic: same record + same base -> same chain_hash.
        let mut rec2 = from_r_receipt(&fixture_receipt());
        let sealed2 = crate::domain::affidavit_integration::seal_sj_record(&mut rec2, &empty_base)
            .expect("second seal of identical input");
        assert_eq!(rec.chain_hash, rec2.chain_hash);
        assert_eq!(sealed1.chain_hash, sealed2.chain_hash);

        // The record is itself chain-attested: it appears in the chain events.
        let event = sealed1.events.last().expect("record event appended");
        assert_eq!(event.event_type, "sj_work_order_sealed");
        assert_eq!(event.seq, 0);
        assert_eq!(event.objects[0].obj_type, "sj_work_order_record");
        assert_eq!(event.objects[0].id, rec.provider_execution_id);
        // Commitment is recomputable from the record's canonical emission.
        let expected_commitment = affidavit::Blake3Hash::from_bytes(
            to_sj_ttl(&[from_r_receipt(&fixture_receipt())]).as_bytes(),
        );
        assert_eq!(event.payload_commitment, expected_commitment);

        // Extending a non-empty base chain: hash differs from the empty base.
        let r = fixture_receipt();
        let base = crate::domain::affidavit_integration::build_deletion_affidavit(
            &crate::domain::receipt::DeletionReceipt::new(0, 1, 2, vec![], None, None),
        )
        .expect("base seals");
        let mut rec3 = from_r_receipt(&r);
        let sealed3 = crate::domain::affidavit_integration::seal_sj_record(&mut rec3, &base)
            .expect("non-empty base seals");
        assert_eq!(sealed3.events.len(), base.events.len() + 1);
        assert_ne!(sealed3.chain_hash, sealed1.chain_hash);
        assert_eq!(sealed3.events[..base.events.len()], base.events[..]);
    }
}
