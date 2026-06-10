use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum UseCaseAction {
    /// 1. The Bloat Cascade Discovery
    BloatCascade,
    /// 2. Orphaned Toolchain Mapping
    OrphanedToolchains,
    /// 3. Polyglot Monorepo Synchronization
    PolyglotSync,
    /// 4. Cache Thrashing Detection
    CacheThrashing,
    /// 5. Implicit Dependency Discovery
    ImplicitDependencies,
    /// 6. Project Lifecycle Petri Nets
    ProjectLifecycle,
    /// 7. Developer Behavior Extraction
    DeveloperBehavior,
    /// 8. Time Machine Exclusion Loops
    TmExclusionLoops,
    /// 9. The Gall Pipeline Verification
    GallPipeline,
    /// 10. Stray Artifact Conformance
    StrayArtifacts,
    /// 11. Adversarial Audit Defense
    AdversarialAudit,
    /// 12. The "Clean" Rule Violation
    CleanRuleViolation,
    /// 13. Ghost File Detection
    GhostFiles,
    /// 14. Space Reclaim Verification
    SpaceReclaim,
    /// 15. System Protection Guarantee
    SystemProtection,
    /// 16. CI/CD Conformance Alignment
    CicdConformance,
    /// 17. Downtime Waste (Muda) Analysis
    DowntimeWaste,
    /// 18. Artifact Rework Metrics
    ArtifactRework,
    /// 19. Disk Spikes Alerting (Andon Oracle)
    DiskSpikes,
    /// 20. Bottleneck Identification
    Bottlenecks,
    /// 21. Storage ROI (Time-to-Value)
    StorageRoi,
    /// 22. Throughput Flow Tracking
    ThroughputFlow,
    /// 23. Predictive Disk Full Alerts
    PredictiveDiskFull,
    /// 24. Statistical Process Control (SPC) for Caches
    CacheSpc,
    /// 25. AutoProcess Autonomic Optimization
    AutonomicOptimization,
}

pub fn print_use_case_instructions(action: &UseCaseAction) {
    match action {
        UseCaseAction::BloatCascade => {
            println!("Use Case 1: The Bloat Cascade Discovery");
            println!("Command: oclnr wpm discover --log <log.jsonocel>");
            println!("Analysis: Use the Inductive Miner to discover the OCPN. Look for sequential causal chains where `git_clone` or `file_modified` (package.json) strongly precedes massive `artifact_candidate_proposed` events for node_modules.");
        }
        UseCaseAction::OrphanedToolchains => {
            println!("Use Case 2: Orphaned Toolchain Mapping");
            println!("Command: oclnr wpm discover --log <log.jsonocel>");
            println!("Analysis: Filter the event log by tool root objects. Orphaned toolchains will appear as disconnected nodes or source places with no subsequent transitions in the generated Petri net.");
        }
        UseCaseAction::PolyglotSync => {
            println!("Use Case 3: Polyglot Monorepo Synchronization");
            println!("Command: oclnr wpm discover --log <log.jsonocel>");
            println!("Analysis: Look for synchronization transitions (transitions with multiple incoming arcs from different object types, e.g., Cargo.toml and package.json) indicating simultaneous compilation triggers.");
        }
        UseCaseAction::CacheThrashing => {
            println!("Use Case 4: Cache Thrashing Detection");
            println!("Command: oclnr wpm discover --log <log.jsonocel>");
            println!("Analysis: Analyze the OCPN for length-one or length-two loops (cycles) involving `artifact_deleted` and `artifact_created` events on the exact same cache directory object.");
        }
        UseCaseAction::ImplicitDependencies => {
            println!("Use Case 5: Implicit Dependency Discovery");
            println!("Command: oclnr wpm discover --log <log.jsonocel>");
            println!("Analysis: Find causal relations (using the Alpha Miner's → relation) between artifacts in Project A and build events in Project B that are not explicitly defined in manifest files.");
        }
        UseCaseAction::ProjectLifecycle => {
            println!("Use Case 6: Project Lifecycle Petri Nets");
            println!("Command: oclnr wpm discover --log <log.jsonocel>");
            println!("Analysis: Project the OCEL log onto a specific `project` object type to visualize its lifecycle from initial observation to eventual deletion.");
        }
        UseCaseAction::DeveloperBehavior => {
            println!("Use Case 7: Developer Behavior Extraction");
            println!("Command: oclnr wpm discover --log <log.jsonocel>");
            println!("Analysis: Use heuristic mining on the aggregated event stream to extract the most frequent paths (the 'happy path') of local development activity.");
        }
        UseCaseAction::TmExclusionLoops => {
            println!("Use Case 8: Time Machine Exclusion Loops");
            println!("Command: oclnr wpm discover --log <log.jsonocel>");
            println!("Analysis: Trace the path from `artifact_candidate_proposed` to `tm_exclusion_plan_written` to verify the exact conditions that lead to exclusion.");
        }
        UseCaseAction::GallPipeline => {
            println!("Use Case 9: The Gall Pipeline Verification");
            println!("Command: oclnr wpm audit --log <log.jsonocel>");
            println!("Analysis: Check conformance against a normative Declare model specifying: `Response(artifact_deleted, deletion_plan_created)` and `Response(artifact_deleted, tm_exclusion_plan_written)`.");
        }
        UseCaseAction::StrayArtifacts => {
            println!("Use Case 10: Stray Artifact Conformance");
            println!("Command: oclnr wpm audit --log <log.jsonocel>");
            println!("Analysis: Identify non-conforming traces where a build artifact exists but the normative model's `Precedence(manifest_modified, artifact_created)` rule is violated.");
        }
        UseCaseAction::AdversarialAudit => {
            println!("Use Case 11: Adversarial Audit Defense");
            println!("Command: oclnr wpm audit --log <log.jsonocel>");
            println!("Analysis: Provide the log and the receipt blake3 hashes as unforgeable proof that deletion transitions fired correctly according to the formal schema.");
        }
        UseCaseAction::CleanRuleViolation => {
            println!("Use Case 12: The \"Clean\" Rule Violation");
            println!("Command: oclnr wpm audit --log <log.jsonocel>");
            println!("Analysis: Evaluate the LTL rule: `Always(os_update -> Eventually(cargo_clean))`. Flag instances where OS update events occurred without prior cleanup.");
        }
        UseCaseAction::GhostFiles => {
            println!("Use Case 13: Ghost File Detection");
            println!("Command: oclnr wpm audit --log <log.jsonocel>");
            println!("Analysis: Find objects of type `filesystem_object` that exist in the terminal marking but lack a well-formed creation trace in the log.");
        }
        UseCaseAction::SpaceReclaim => {
            println!("Use Case 14: Space Reclaim Verification");
            println!("Command: oclnr wpm audit --log <log.jsonocel>");
            println!("Analysis: Verify data perspective conformance: The attribute `bytes_freed` on `snapshot_thin_requested` must mathematically align with the sum of deleted artifact sizes.");
        }
        UseCaseAction::SystemProtection => {
            println!("Use Case 15: System Protection Guarantee");
            println!("Command: oclnr wpm audit --log <log.jsonocel>");
            println!("Analysis: Verify the negative Declare constraint: `NotChainSuccession(scan_root_started, system_directory_modified)`.");
        }
        UseCaseAction::CicdConformance => {
            println!("Use Case 16: CI/CD Conformance Alignment");
            println!("Command: oclnr wpm audit --log <log.jsonocel>");
            println!("Analysis: Compute the fitness (using alignment-based conformance checking) between your local laptop's event log and the standard CI/CD pipeline Petri net.");
        }
        UseCaseAction::DowntimeWaste => {
            println!("Use Case 17: Downtime Waste (Muda) Analysis");
            println!("Command: oclnr wpm lean --log <log.jsonocel>");
            println!("Analysis: Calculate the sojourn time in the `compiling` state across all projects. High aggregate sojourn time indicates Muda.");
        }
        UseCaseAction::ArtifactRework => {
            println!("Use Case 18: Artifact Rework Metrics");
            println!("Command: oclnr wpm lean --log <log.jsonocel>");
            println!("Analysis: Count the frequency of `artifact_created` -> `artifact_deleted` -> `artifact_created` cycles for the exact same object ID (e.g., a specific Docker image).");
        }
        UseCaseAction::DiskSpikes => {
            println!("Use Case 19: Disk Spikes Alerting (Andon Oracle)");
            println!("Command: oclnr wpm oracle --log <log.jsonocel>");
            println!("Analysis: Stream events to the oracle. If a sequence of events forms an impossible prefix for a stable disk state (e.g., rapid unbound allocations), trigger an Andon alert.");
        }
        UseCaseAction::Bottlenecks => {
            println!("Use Case 20: Bottleneck Identification");
            println!("Command: oclnr wpm lean --log <log.jsonocel>");
            println!("Analysis: Analyze the performance perspective of the OCPN. Places with the highest waiting times (e.g., waiting for DerivedData locks) are the system bottlenecks.");
        }
        UseCaseAction::StorageRoi => {
            println!("Use Case 21: Storage ROI (Time-to-Value)");
            println!("Command: oclnr wpm lean --log <log.jsonocel>");
            println!("Analysis: Correlate the `bytes` attribute of an object with its access frequency. Large bytes + low access frequency = low ROI.");
        }
        UseCaseAction::ThroughputFlow => {
            println!("Use Case 22: Throughput Flow Tracking");
            println!("Command: oclnr wpm lean --log <log.jsonocel>");
            println!("Analysis: Measure Little's Law on the local disk: Work-In-Progress (total artifacts) = Throughput (creation rate) × Lead Time (time until deletion).");
        }
        UseCaseAction::PredictiveDiskFull => {
            println!("Use Case 23: Predictive Disk Full Alerts");
            println!("Command: oclnr wpm spc --log <log.jsonocel>");
            println!("Analysis: Use predictive monitoring (e.g., LSTM on event traces) to extrapolate the trajectory of `bytes_seen` and predict the timestamp of 100% utilization.");
        }
        UseCaseAction::CacheSpc => {
            println!("Use Case 24: Statistical Process Control (SPC) for Caches");
            println!("Command: oclnr wpm spc --log <log.jsonocel>");
            println!("Analysis: Plot `tool_root_observed` sizes on an X-bar chart. Flag any cache that exceeds 3 sigma (the Upper Control Limit) from its historical mean.");
        }
        UseCaseAction::AutonomicOptimization => {
            println!("Use Case 25: AutoProcess Autonomic Optimization");
            println!("Command: oclnr wpm autoprocess --log <log.jsonocel>");
            println!("Analysis: Train an RL agent on the OCEL log. The agent's action space is `propose_deletion`. The reward function maximizes free space while minimizing the penalty of deleting actively used caches.");
        }
    }
}
