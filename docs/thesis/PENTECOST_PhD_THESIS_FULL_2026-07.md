---
title: |
  Receipted Execution: A Philosophy and Architecture for
  Trustworthy Destructive Computation
subtitle: "Pentecost — Filesystem Lifecycle Governance as Testimony Before Action"
author: "Sean Chatman"
date: "July 2026"
institution: "Open Source Research"
degree: "Doctor of Philosophy"
geometry: margin=1in
fontsize: 12pt
documentclass: report
toc: true
numbersections: true
header-includes:
  - \usepackage{setspace}
  - \setstretch{1.5}
  - \usepackage{amsmath}
  - \usepackage{amssymb}
---

\newpage

# Declaration of Authorship

I, **Sean Chatman**, declare that this thesis and the work presented in it are my own. Where I have consulted or built upon the published work of others — Object-Centric Process Mining, the BLAKE3 hash function, Gall's Law, Rice's Theorem, the Rust affine type system — this is clearly attributed. The system described here, **Pentecost** (`osx-clnr` / `oclnr`), is open-source software whose repository constitutes the executable artifact of this dissertation: every claim made in these pages is either enforced by the compiler, checked by a doctest, or explicitly flagged in Chapter 10 as an open limitation.

**Signed:** *Sean Chatman* — **Date:** July 2026

\newpage

# Abstract

Modern software development manufactures entropy. Package managers, build systems, container runtimes, IDEs, and simulators deposit hundreds of gigabytes of derived state onto developer machines, and the tools we use to reclaim that space — `rm -rf`, `find -delete`, GUI cleaners — operate on a model of destruction that has not changed since 1971: *inspect, guess, delete, hope*. They produce no evidence, admit no verification, and cannot be safely delegated to autonomous agents.

This dissertation argues that the deficiency is not incidental but structural, and proposes a replacement paradigm: **receipted execution** — the principle that a system's destructive power must never increase without a corresponding increase in the evidence it produces. We develop this philosophy from first principles, formalize it through three instruments — the **Chatman Equation** $A = \mu(O^*)$, the **Gall Pipeline** of LTL-constrained lifecycle phases, and **typestate-enforced admission control** — and instantiate it as Pentecost, a macOS disk auditor and cleanup utility written in Rust whose architecture makes the philosophy mechanically binding: *the scanner cannot delete, and the deleter cannot scan*.

Pentecost contributes: (1) a formalization of the POSIX filesystem lifecycle as an Object-Centric Event Log (OCEL v2), making deletion a discoverable, conformance-checkable process rather than an untraceable side effect; (2) a plan-bound deletion engine in which the destructive path reads only from human-reviewed, cryptographically approvable plan documents, enforced by Rust's affine type system; (3) provenance receipts sealed by BLAKE3 hash chains via the `affidavit` kernel, verified against measured free-space deltas; (4) an MCP (Model Context Protocol) server exposing the full workflow to AI agents, demonstrating that receipted execution is precisely the substrate that makes autonomous destructive delegation tenable; and (5) an incremental scanning architecture with Salsa-style early cutoff that preserves evidentiary completeness across cached re-scans — proving that performance optimization and evidence integrity need not trade off.

Beyond the artifact, this thesis is an argument about the future of computing: as agency migrates from humans to machines, the question "did the machine do the right thing?" must become answerable *from the machine's own output*. Systems must learn to testify before they act.

\newpage

# Chapter 1 — Introduction: The Silence After `rm`

## 1.1 The problem, concretely

Run `cargo build` in a Rust workspace and a `target/` directory appears — often 5–50 GB. Run `npm install` and `node_modules` materializes. Xcode deposits DerivedData and simulator runtimes; Docker accumulates dangling layers; Homebrew keeps superseded kegs; Time Machine pins local APFS snapshots that silently defeat deletion altogether (the bytes are "freed" into a snapshot and the disk stays full). A working developer's machine is a landfill managed by no one.

The reclamation tools available are all variations on the same act: a person (or increasingly, an AI agent) inspects a snapshot of disk state, forms a guess about what is safe to destroy, and issues an irreversible command. When it finishes, the system is silent. Did it delete what was intended? Only what was intended? Did space actually return? Was a snapshot pinning it? There is no artifact that answers these questions, because the operation was never designed to produce one.

## 1.2 The problem, formally

The guess at the heart of `rm -rf` is not merely risky — it is *provably* unsound. **Rice's Theorem** (1953) establishes that every non-trivial semantic property of a program is undecidable by static analysis. The property "this directory is a stale build artifact rather than a live dependency" is a semantic property of the *processes* that created and consume the directory, not a syntactic property of its path, size, or mtime. It is therefore not computable from a filesystem snapshot. Every static cleanup heuristic is an approximation of an undecidable predicate, and its failure modes are silent.

The escape hatch Rice permits is dynamic observation: watch the processes, record the events, and reason over the *history* rather than the snapshot. This is exactly the move that process mining made for business systems, and this thesis makes it for the filesystem.

## 1.3 Thesis statement

> **Destructive computational operations should be governed as evidenced processes: observed into structured event logs, admitted through machine-checkable plans, executed only from those plans, and sealed with cryptographic receipts verified against measured reality. A system built this way is safer for humans, and — critically — is the only kind of system to which destructive authority can responsibly be delegated to autonomous agents.**

## 1.4 The core invariant

Pentecost's entire design derives from one sentence, stated in the project's charter and repeated here as the thesis's north star:

> **Never increase destructive power without increasing receipts. The scanner cannot delete; the deleter cannot scan.**

The second clause is the architectural mechanism of the first. A component that can both discover targets and destroy them can destroy anything it discovers — its blast radius is the filesystem. Splitting discovery from destruction, with a human- (or agent-) reviewed plan document as the *only* channel between them, bounds the blast radius to the reviewed plan. Chapter 5 shows how Rust's type system makes this separation a compile-time fact rather than a convention.

## 1.5 Contributions and roadmap

- **Ch. 2** develops the philosophy: testimony before action, Gall's Law as an engineering method, and evidence as the unit of trust.
- **Ch. 3** presents the mathematical formalisms: the Chatman Equation, the Gall Pipeline's LTL constraints, and the OCEL v2 filesystem ontology.
- **Ch. 4** describes the system architecture: the domain/integration/nouns/mcp layering and its purity discipline.
- **Ch. 5** details the safety mechanics: typestate admission, plan-bound deletion, refusal-as-specification.
- **Ch. 6** covers cryptographic receipts and the `affidavit` provenance chain.
- **Ch. 7** treats incremental observation: the sled-backed scan cache with early cutoff, and the theorem that caching must preserve evidentiary completeness.
- **Ch. 8** extends the model across the developer environment: Docker, Homebrew, Xcode, toolchains, backups, and the disk-pressure monitor daemon.
- **Ch. 9** presents the MCP server and the case for receipted execution as the substrate of autonomous agency.
- **Ch. 10** is an honest evaluation: what holds, what is stubbed, and where the system's self-verification currently falls short of its own law.
- **Ch. 11** concludes with the 2030 vision: the autonomic, self-testifying developer environment.

\newpage

# Chapter 2 — Philosophy: Testimony Before Action

## 2.1 Measurements are not decisions

A disk-usage tool that reports "`node_modules` is 2.3 GB" has produced a *measurement*. The user must still convert it into a *decision* ("delete it") and an *action* (`rm -rf`), and the conversion happens entirely in their head — unrecorded, unreviewable, unverifiable. The philosophy of this work is that the conversion itself is the artifact that matters. Pentecost reifies each stage: the measurement becomes an **audit** (a JSON document plus an OCEL event log), the decision becomes a **plan** (a reviewable document listing exactly what will be destroyed and why), the action becomes an **execution bound to that plan**, and the aftermath becomes a **receipt** whose claims are checked against the physically measured free-space delta.

Each stage produces a durable object. Trust in the system is trust in the objects, not in the operator's memory or the tool's reputation.

## 2.2 Gall's Law as method, not slogan

> "A complex system that works is invariably found to have evolved from a simple system that worked." — John Gall

This project treats Gall's Law as a development discipline. Capability is added only through numbered checkpoints (G0–G9), each of which must leave the system *working and verified* before the next begins: G0 a compiling skeleton; G1 pure scanning; G2 architectural partition; G3 plan generation; G4 refusal logic; G5 plan-bound deletion; G6 receipts; G7 affidavit sealing; G8 privacy gating; G9 self-verification (the doctor). The checkpoint document in the repository (`docs/GALL_CHECKPOINTS.md`) is the ledger; a capability without its checkpoint's receipts is, by policy, not a capability but a liability. The law's corollary is this thesis's core invariant: each increase in what the system can *do* must be preceded by an increase in what the system can *prove*.

## 2.3 Refusal is a feature, and doctests are its specification

Most software documents what it does. Pentecost's domain layer additionally documents — as executable doctests — what it *refuses* to do. `validate_plan_item` carries doctests demonstrating that it rejects macOS system paths and rejects any path not literally present in the plan. These refusal doctests are treated as specification: removing or weakening one is defined as a defect regardless of whether the test suite passes. The philosophical claim is that a safety property that exists only in prose is a wish; a safety property that exists as a failing-if-violated test is a law.

## 2.4 Privacy as a property of evidence

Evidence that must be shared (bug reports, research corpora, this thesis) must not leak the observed machine. Pentecost's answer is structural: receipts identify filesystem objects by `BLAKE3(path)`, not by path; the redaction domain rewrites `/Users/<name>` and credential-shaped content before evidence leaves the machine; the doctor's privacy diagnostic scans the repository itself for unredacted real paths. The principle: *provenance must survive redaction*. A hash-identified receipt proves the same chain of custody whether or not you can read the paths.

## 2.5 The ethics of delegated destruction

The urgency of this philosophy is the arrival of capable AI agents. An agent asked to "free up disk space" and given a shell will run `rm -rf` on its best guess — inheriting every silent failure mode of Section 1.2, at machine speed, without the hesitation a human feels before pressing Enter. The correct response is neither to deny agents destructive authority (they will be given it regardless, because it is useful) nor to trust them (Rice forbids anyone, human or machine, from *deserving* that trust statically). The correct response is to change the *interface* of destruction so that the safe path is the only path: the agent may scan freely, must produce a plan, may execute only that plan, and must hand back a sealed receipt. Chapter 9 shows this interface implemented as an MCP server. Alignment, in this domain, is an architecture problem.

\newpage

# Chapter 3 — Formal Foundations

## 3.1 The Chatman Equation

The transition from raw observation to trustworthy evidence is written:

$$A = \mu(O^*)$$

where $O^*$ is the raw, continuous observation stream (directory structures, entry metadata, free-space samples, process events); $\mu$ is a structured, total, auditable transformation (the scanner, the plan compiler, the typestate adjudicator, the receipt sealer); and $A$ is discrete, unforgeable evidence (the admitted plan, the sealed receipt). The equation's force is in what it excludes: any action not derivable as $\mu$ applied to recorded observation is inadmissible. There is no side door from $O^*$ to the world that bypasses $\mu$ — that is precisely the "scanner cannot delete" clause. The same equation was originally developed for symbolic program structures (parsers and ASTs: $O^*$ = source text, $\mu$ = parser, $A$ = tree); its application here demonstrates domain generality: the filesystem is just another observable whose lifecycle can be compiled into evidence.

## 3.2 The Gall Pipeline and its LTL constraints

Lifecycle transitions must traverse, in order:

$$\text{Observe} \rightarrow \text{Plan} \rightarrow \text{Exclusion} \rightarrow \text{Deletion} \rightarrow \text{Receipt} \rightarrow \text{Verification}$$

Formalized over the OCEL event stream as Linear Temporal Logic:

1. **Precedence** $\Phi_1$: $\square(\text{artifact\_deleted} \rightarrow \lozenge_{\leq 0}\, \text{deletion\_plan\_created})$ — no deletion without a prior plan.
2. **Response** $\Phi_2$: $\square(\text{deletion\_plan\_created} \rightarrow \lozenge\, \text{tm\_exclusion})$ — planning must lead to Time Machine exclusion before execution (else "freed" bytes are pinned by snapshots).
3. **Chain succession** $\Phi_3$: $\square(\text{tm\_exclusion} \rightarrow \bigcirc\, \text{artifact\_deleted})$ — execution follows exclusion verification immediately, bounding the TOCTOU window.

Conformance checking of emitted OCEL logs against $\Phi_{1..3}$ turns "did the tool behave?" into a mechanical query over evidence.

## 3.3 The filesystem as an object-centric event log

Classical process mining assumes a single case notion; filesystem reality is many-to-many — one `cargo build` touches thousands of objects, one `target/` dir is touched by many builds. OCEL v2 resolves this: **objects** (`artifact_candidate`, `deletion_plan`, `deletion_receipt`, `filesystem_object`, `tool_root`) participate in **events** (`audit_completed`, `deletion_plan_created`, `snapshot_delete_requested`, `snapshot_thin_requested`) via typed relationships. Pentecost imposes truthfulness constraints at the schema level: a delete event *must* carry relationships to its plan, receipt, candidate, and filesystem object, and event types must name what actually happened (a thin is never logged as a delete). Referential integrity of these logs is validated at receipt-verification time.

## 3.4 Undecidability and its perimeter

Rice's Theorem bounds what any $\mu$ can do: no transformation of a snapshot can decide artifact liveness. Pentecost's response is layered: (a) detection heuristics (`domain::artifact`) are explicitly *candidate generators*, never authorities; (b) authority rests with the plan-review step, where a human or accountable agent converts candidates to commitments; (c) the receipt-verify step measures reality (free-space delta via `statvfs`) so that even a wrong decision is at least an *evidenced* wrong decision, correctable and attributable. The system does not claim to solve the undecidable; it claims to make every approximation accountable.

\newpage

# Chapter 4 — Architecture: Purity as Partition

## 4.1 The four layers

```
src/
  domain/       Pure logic. Zero std::fs, std::process, zero OS calls.
                Receives inert DTOs (EntrySnapshot, DirSnapshot).
  integration/  All I/O: filesystem walking, tmutil, Docker, brew,
                Xcode, launchd, notifications, sled scan cache.
  nouns/        CLI verb handlers (audit, plan, delete, receipt,
                snapshot, doctor, monitor, ...). Bridge layer.
  mcp/          JSON-RPC server exposing the workflow to agents.
```

The **domain purity constraint** is the load-bearing wall: because `domain/` cannot touch the OS, every safety-relevant function in it is deterministic, doctest-able, and incapable of causing harm during testing. The integration layer converts the world into DTOs; the domain converts DTOs into decisions; the nouns convert decisions back into (receipted) world-changes. The Chatman Equation is thus visible in the directory tree: `integration` gathers $O^*$, `domain` is $\mu$, and the JSON/OCEL outputs are $A$.

## 4.2 The execution pipeline as files

```
oclnr audit run        →  disk-audit.json + disk-audit.jsonocel
oclnr plan build       →  cleanup-plan.json        (human reviews)
oclnr delete execute   →  reads plan ONLY; writes deletion-receipt
oclnr receipt verify   →  checks claimed vs measured Δfree, OCEL integrity
oclnr receipt certify  →  affidavit seal (BLAKE3 chain)
```

Every arrow crosses through a durable file. This is deliberate: files are reviewable, diffable, attachable to tickets, and — unlike in-memory state — they survive the tool's own crash. The pipeline's state *is* its evidence.

## 4.3 The doctor: a system that examines itself

Checkpoint G9 requires the tool to verify its own operating law: `oclnr doctor` runs architecture, substrate, doctest-coverage, and privacy diagnostics over the repository itself. The doctest checker enforces that domain functions carry positive, negative, and refusal cases; the privacy checker scans for unredacted user paths. Chapter 10 evaluates honestly how far this self-verification currently reaches — notably, that the architecture check verifies layout but does not yet grep for purity violations, a gap this thesis names as its own highest-priority future work.

\newpage

# Chapter 5 — Safety Mechanics: The Compiler as Adjudicator

## 5.1 Typestate admission

A deletion plan enters the system as `Evidence<Plan, Raw, PlanSafetyWitness>`. The only function producing an `Admitted` value is the adjudicator, which runs the safety gauntlet: schema validity, macOS-system-path refusal (`is_macos_os_dir`), plan-membership checks. Rust's affine types guarantee the `Raw` value is consumed in the process and cannot be replayed or aliased into the executor. The executor's signature accepts only `Admitted` evidence. Consequently, "deletion of an unvalidated plan" is not a runtime error to be caught — it is a program that does not compile. The safety boundary $h_I$ is enforced by `rustc`, the most heavily audited component in the toolchain.

## 5.2 Plan-bound execution

`delete execute` performs no filesystem discovery: no `read_dir`, no walking, no globbing (verified by grep and by integration test). It parses the saved plan, re-validates each item through the domain guard (`validate_plan_item` — refusal doctests attached), and delegates the physical unlink to `integration::fs`. Anything on disk that is not named in the plan is, from the executor's perspective, invisible. The time-of-check/time-of-use window is acknowledged and narrowed by $\Phi_3$'s chain-succession constraint rather than pretended away.

## 5.3 Snapshot truthfulness

Deleting APFS-snapshot-pinned data frees nothing; the honest operation is snapshot *thinning* via `tmutil`. Pentecost refuses the euphemism structurally: the OCEL schema separates `snapshot_delete_requested` from `snapshot_thin_requested`, and receipt verification compares the claimed reclamation against the *measured* `statvfs` delta — so a thin that reclaimed nothing is exposed by its own receipt.

\newpage

# Chapter 6 — Cryptographic Receipts and the Affidavit Chain

## 6.1 From log to testimony

A receipt that can be edited is a diary, not testimony. After execution, `domain::affidavit_integration` projects the deletion receipt into an `affidavit::Receipt`: a BLAKE3 hash chain assembled over the operation's context — plan identity, tool root, artifact metadata, outcomes — such that any post-hoc modification breaks the chain. The design deliberately reuses the external `affidavit` kernel rather than a hand-rolled seal (the repository's history records the migration and the deletion of the in-house implementation): cryptographic trust concentrates in one audited component, honoring Gall's Law by *removing* complexity at the moment power increased.

## 6.2 Privacy-preserving identity

Filesystem objects enter the chain as `BLAKE3(path)`. The seal therefore proves *which* objects were destroyed under *which* plan without disclosing the machine's directory structure — the receipt can be published, submitted as evidence, or included in a research corpus intact. Verification recomputes hashes from the local paths; third parties verify chain integrity without learning the paths. Provenance survives redaction (§2.4).

## 6.3 Verification against reality

`receipt verify` is deliberately two-eyed: it checks the cryptographic chain (internal consistency) *and* the physical world (claimed bytes vs measured free-space delta, OCEL referential integrity). A receipt can be unforged yet wrong — if the tool honestly recorded a deletion that freed nothing. Only the conjunction of cryptographic and empirical verification constitutes the standard of evidence this thesis demands.

\newpage

# Chapter 7 — Incremental Observation Without Evidentiary Loss

## 7.1 The performance problem

Full-disk audits are expensive; developers will not run a governance pipeline that takes minutes when `rm -rf` takes seconds. Adoption of receipted execution therefore depends on making observation cheap. Pentecost's answer (July 2026) is a persistent, sled-backed directory cache with **Salsa-style early cutoff**: each directory's cache key is a hash of its child names plus metadata; on re-scan, a subtree whose key is unchanged is *pruned* — not descended — and its previously computed results are folded into the running audit.

## 7.2 The completeness theorem (learned the hard way)

The first implementation of this cache contained an instructive defect: on a cache hit it folded back *shallow* per-directory statistics and — critically — did **not** re-emit the deletion candidates discovered inside the pruned subtree. A second audit of an unchanged tree therefore silently *lost candidates*, and any plan built from it was incomplete: an optimization had quietly violated the evidence layer that everything above it depends on. The corrected design states the requirement as an invariant:

> **Cache-hit equivalence:** for any filesystem state $F$, the audit evidence produced with the cache warm must equal the evidence produced cold: $\text{Audit}_{\text{warm}}(F) = \text{Audit}_{\text{cold}}(F)$ — identical candidate sets, identical aggregate statistics.

Cached entries now store true recursive subtree aggregates *and* the full candidate list for the subtree; a cache hit re-inserts those candidates exactly as if freshly discovered. A regression test in the integration suite runs the audit twice against the same tree and asserts equality of candidate sets and totals. The episode is reported here rather than hidden because it is the thesis in miniature: **every optimization of $O^*$-gathering must be proven to preserve $A$**, and the proof must be executable.

## 7.3 Concurrency

The candidate container moved from a mutex-guarded ordered set to a concurrent map keyed by path, and audit roots are scanned in parallel with scoped threads, errors aggregated across roots rather than short-circuiting. Determinism of the *evidence* (canonical ordering at serialization time) is preserved independent of scheduling.

\newpage

# Chapter 8 — Governing the Whole Developer Environment

The audit/plan/delete/receipt core governs the generic filesystem. The developer's disk, however, is dominated by *managed* stores whose owners provide their own safe reclamation verbs. Pentecost's July 2026 surfaces extend observation — deliberately read-only — across:

- **Docker** (`oclnr docker`): parses `docker system df`, previews prune-reclaimable space; refuses to prune, printing the owner's own command instead.
- **Homebrew** (`oclnr brew`): dry-run `brew cleanup`/`autoremove` parsing.
- **Xcode** (`oclnr xcode`): DerivedData and simulator-runtime accounting.
- **Backups** (`oclnr backup`): iOS device backup enumeration and sizing.
- **Toolchains & repo health** (`oclnr tools`): rustup/npm/pip cache sizing; git repository health (unpushed work detection — a *deletion contraindication* detector).
- **Monitor & daemon** (`oclnr monitor`, `oclnr daemon`): a launchd-installable disk-pressure watcher that raises desktop notifications when free space crosses a threshold — closing the loop from *reactive* cleanup to *anticipatory* governance.

The design rule across all of these: where a subsystem has an accountable owner (Docker, brew, Xcode), Pentecost **observes and recommends** but does not destroy — destruction through a foreign manager would produce receipts Pentecost cannot honestly seal. Destructive authority remains confined to the plan-bound pipeline. Power did not increase; only sight did.

\newpage

# Chapter 9 — The Agent Interface: MCP and Delegated Destruction

## 9.1 Seventeen tools, one narrow waist

The `oclnr-mcp` server exposes the workflow over JSON-RPC: `query_workflow_state`, `audit_scan`, `audit_parse`, `plan_build`, `plan_inspect`, `plan_validate`, `plan_approve`, `delete_dry_run`, `delete_execute`, `receipt_parse`, `receipt_verify`, `receipt_certify`, `safety_audit`, `snapshot_audit`, `emergency_reclaim`, `plan_rollback`, `clear_artifacts`. A workflow state machine with explicit legal transitions and per-state next-step guidance shepherds the caller through the Gall Pipeline; destructive tools require an explicit `confirm: true`; skipping from scan to execute is a rejected transition, not a scolding.

## 9.2 Why this is the alignment result

Give an agent a shell and ask it to free disk space: its cheapest action is catastrophic. Give the same agent *only* the MCP surface and its cheapest action is the safe one — the tools' affordances *are* the policy. The agent can be curious (scan, parse, inspect) without limit and destructive only through the reviewed-plan channel, and everything it does accretes into the same evidence chain a human operator would produce. The claim generalizes beyond disks: **any capability we intend to delegate to machines should first be rebuilt as a receipted pipeline, then delegated.** The receipt is what makes ex-post accountability possible; the typestate gate is what makes ex-ante trust unnecessary.

\newpage

# Chapter 10 — Honest Evaluation: The Gap Between Law and Enforcement

A dissertation about systems that testify truthfully must testify truthfully about itself. As of July 2026:

**What demonstrably holds.** The domain/integration partition and DTO discipline are real. The deleter performs no scanning (grep- and test-verified). Plan-bound deletion with typestate admission and refusal doctests works end-to-end in the integration suite (170 tests green: unit, integration, and 84 doctests). Affidavit sealing with hash-based object identity is implemented and exercised. The scan cache satisfies cache-hit equivalence under a dedicated regression test. The full lint/test/doctor pipeline runs clean except as noted below.

**What is honestly incomplete.**

1. **Domain purity is violated in three wired modules** — `domain/crypto.rs` (reads files to hash them), `domain/policy.rs` (loads its own config), `domain/ocl.rs` (opens a sled store). Each performs I/O that belongs in `integration`. The law is right; these files are wrong.
2. **The G9 doctor does not yet enforce the law it exists to enforce**: its architecture check verifies file *layout*, not the purity constraint — which is exactly how (1) survived. The single highest-value future change is a doctor pass that greps `src/domain/**` for `std::fs`/`std::process`/direct store access and fails loudly.
3. **G8 privacy gating is a detector, not yet a gate**: redaction primitives exist and the doctor flags unredacted paths, but reports are not automatically passed through redaction on write, and ~40 findings remain in documentation files.
4. **Parts of the MCP surface are scaffolding**: several tool bodies return well-shaped but hardcoded results, one subprocess binding targets a stale CLI contract, and `plan_approve`'s HMAC uses a fixed secret — approval theater until keyed properly.
5. **The build is not hermetic** (sibling path dependency, git-pinned affidavit rev), and one documented dependency landmine (the wasm-bindgen pin conflict) is defended only by prose.

By the project's own standard these are not embarrassments to be minimized but *receipts of the current state* — enumerated, prioritized, and (per Gall) to be closed one working checkpoint at a time. A system that cannot admit where its enforcement lags its law has already failed the thesis.

\newpage

# Chapter 11 — Conclusion: Toward the Self-Testifying Machine

This dissertation began with silence — the silence after `rm` — and proposed to fill it with evidence. The argument, compressed:

1. Deciding what to destroy is undecidable from snapshots (Rice); therefore observe processes, not states.
2. Observation must be compiled into unforgeable evidence ($A = \mu(O^*)$); therefore structure the pipeline so no action bypasses $\mu$.
3. Power must be partitioned from perception ("the scanner cannot delete; the deleter cannot scan"); therefore make the partition a compile-time fact.
4. Every destructive act must produce a receipt cryptographically bound to its justification and empirically checked against reality; therefore seal and verify.
5. Only such a system can be handed to an autonomous agent; therefore the MCP surface is not a convenience feature but the point.

The 2030 horizon is an **autonomic developer environment**: a monitor daemon that observes pressure continuously; a policy learner (the pipeline is already MDP-shaped: states are audit evidence, actions are plans, rewards are verified reclamation net of rebuild cost) that proposes plans from accumulated OCEL history; an agent that executes them through the typestate gate; and a growing chain of sealed receipts constituting the machine's testimony about its own stewardship. The human's role shifts from performing deletions to auditing testimony — and because the testimony is cryptographic, auditing scales.

The deeper claim outlives the disk cleaner. Every consequential capability we automate — deleting files today; migrating databases, rotating credentials, spending money, editing production tomorrow — will face the same trilemma of undecidable safety, useful power, and delegated agency. The resolution demonstrated here is general: **observe into evidence, admit through types, execute from plans, seal with receipts, verify against reality.** Machines are about to act on our behalf at scales no human can supervise directly. The systems worthy of that role will be the ones that learned, first, to testify.

\newpage

# References

1. van der Aalst, W.M.P. *Process Mining: Data Science in Action*, 2nd ed., Springer, 2016; and the OCEL 2.0 specification.
2. Rice, H.G. "Classes of Recursively Enumerable Sets and Their Decision Problems." *Trans. AMS* 74, 1953.
3. Gall, J. *Systemantics: How Systems Really Work and How They Fail*, 1975.
4. O'Connor, J., Aumasson, J-P., Neves, S., Wilcox-O'Hearn, Z. "BLAKE3: One Function, Fast Everywhere," 2020.
5. Strom, R.E., Yemini, S. "Typestate: A Programming Language Concept for Enhancing Software Reliability." *IEEE TSE*, 1986.
6. Jung, R., et al. "RustBelt: Securing the Foundations of the Rust Programming Language." *POPL*, 2018.
7. Pnueli, A. "The Temporal Logic of Programs." *FOCS*, 1977.
8. Matsakis, N. et al. Salsa: incremental recomputation framework (early-cutoff memoization), rust-lang, 2019–.
9. Apple Inc. *Apple File System (APFS) Reference*; `tmutil(8)`; `statvfs(3)`.
10. Anthropic. *Model Context Protocol Specification*, 2024–2026.
11. Chatman, S. "Formalizing Filesystem Lifecycle Semantics" (companion arXiv submission) and the Pentecost repository, `github.com/seanchatmangpt/mac-artifact-cleaner`, 2026 — the executable artifact of this thesis.
