# r-projection ↔ semantic-jira Field Mapping

Declares formally what was previously implicit: the `RReceipt` shape projected
by `src/domain/r_projection.rs` is structurally equivalent (field-for-field, 1:1)
to the semantic-jira pack's `sj:` receipt vocabulary.

- Protocol string: `oclnr-native+affidavit-core/v1+r-projection/v2`
  (`src/domain/r_projection.rs:210`, in `RProvider.receipt_protocol`,
  `src/domain/r_projection.rs:108-114`).
- Anchor source: `src/domain/r_projection.rs` @ main `88d221b`
  (v26.10.8 campaign).

## Structural Equivalence Table

| RReceipt field (r_projection.rs) | Line | sj: field (semantic-jira pack) | Notes |
|---|---|---|---|
| `identity.subject` / `identity.repo` | 63-69 | `sj:subject` | `repo` qualifies the subject's provenance checkout. |
| `identity.subject_sha` / `identity.base_sha` | 63-69 | `sj:baseSha` | Both pin the exact subject SHA; `base_sha` = `subject_sha` locally (no intermediate relay). |
| `authority.ceiling` | 71-76 | `sj:authorityCeiling` | Literal `"DO"` (`fn assemble`, line 185). |
| `authority.grant` / `authority.actor` | 71-76 | `sj:grant` / `sj:actor` | `grant` empty ⇒ `"NONE"`; empty order ⇒ REFUSED downstream. |
| `consequence` | 78-83 | `sj:consequence` | Same triple: `commits` / `oclnr` `files_changed` / `remote_effects` (deletion reclaims, snapshot lists). |
| `replay.commands` + `replay.durable_location` | 85-92 | `sj:replayIdentity` | `RCommand.output_sha256` = native receipt sha256 — the replay joins back to the source of truth; `durable_location` = the on-disk `<stem>.r.json` / native receipt path. |
| `standing.value` / `standing.derived_from` | 100-106 | `sj:standing` | `ALIVE` / `PARTIAL_ALIVE` / `BLOCKED:` / `REFUSED(...)` shared with the fleet schema. |
| `work_order_id` | 119-121 | `sj:workOrder` | The admitted order this execution discharged (e.g. `oclnr-plan:<plan_hash>`). |
| `origin_authority` | 123-127 | `sj:originAuthority` | Identical to `authority` — oclnr has no intermediate relay that could narrow it. |
| `provider` / `provider_execution_id` | 108-114, 131 | `sj:provider` / `sj:providerExecutionId` | `provider_execution_id` = `oclnr:sha256:<native sha256>` (line 212). |

## chain_hash → sj:receipt Payload Commitment

The affidavit chain (`src/domain/affidavit_integration.rs`, `verify_chain_hash`
at `src/domain/affidavit_integration.rs:268`) commits to the exact payload
bytes. Mapping:

```
affidavit chain_hash (blake3, hex)  →  sj:receipt payload commitment
```

- `SealedAffidavit.chain_hash` is the recomputed-then-sealed digest over the
  affidavit payload; `verify_chain_hash` (lines 268-286) refuses any stored
  value that disagrees with the recomputed digest.
- This plays the role of `sj:receipt`'s payload commitment: the semantic-jira
  receipt object carries its own commitment to the consequence payload, and the
  affidavit chain_hash satisfies it byte-exactly.

## REFUSED(no-work-order) → sj: Refusal Registry Seam

`assemble` (`src/domain/r_projection.rs:138-215`) emits `REFUSED(no-work-order)` with `broken_term = R_missing_authority`
(line 152) when `work_order_id` is empty. Under the mapping, this is the
condition the semantic-jira pack's refusal registry records: ggen's
append-only refusal registry consumes the typed refusal (`standing.value`
starting `REFUSED` plus `broken_term`) as an `sj:` refusal-registry entry, so
every projection refusal is first-class `sj:` demand-side evidence rather than
a dropped log line.

## Receipt-Protocol Disclosure

```
oclnr-native+affidavit-core/v1+r-projection/v2
```

Components: the native oclnr receipt (source of truth, referenced by
`output_sha256`), the affidavit-core v1 chain commitment, and the r-projection
v2 fleet-shape projection. This string is a constant in
`src/domain/r_projection.rs:210`; the mapping declared here is documentation-only — no code change, tests unchanged.
