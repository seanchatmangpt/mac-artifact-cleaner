# Future Capabilities Report: osx-clnr Ecosystem Review

This report summarizes the research conducted by five specialized sub-agents (May 2026) to identify potential directions for the `osx-clnr` project.

---

## 1. Hardware & System Vitality
**Focus:** Correlating artifact cleanup with physical system health.

### Recommended Integration:
- **`darwin-metrics`**: Provides native CPU pressure, memory pressure, and thermal states.
- **`sysinfo`**: Correlates file artifacts with running processes (e.g., "Do not delete `DerivedData` for an active Xcode process").
- **`macsmc`**: Monitors fan speeds and thermal throttling to provide a "System Stress" audit.

**Impact:** Allows the tool to provide a "Cleanliness vs. Performance" report, showing how much disk pressure is impacting system thermals or memory swap.

---

## 2. macOS Native Frameworks (The "Native" Shift)
**Focus:** Moving from hardcoded heuristics to OS-backed authority.

### Recommended Integration:
- **`objc2-foundation`**: Replaces hardcoded strings with `NSSearchPathForDirectoriesInDomains` and `NSFileManager` for robust, version-aware path resolution.
- **`security-framework`**: Audits the User Keychain for orphaned development certificates and API tokens left by tools like `npm`, `cargo`, or `docker`.
- **`system-configuration`**: Identifies stale network profiles and VPN configurations from uninstalled tools.

**Impact:** Increases the "mac-native" status of the tool, ensuring it works across localized systems and future macOS updates.

---

## 3. Deep Filesystem & Metadata
**Focus:** Using APFS and Spotlight for superior classification.

### Recommended Integration:
- **`xattr`**: Reads `com.apple.metadata:kMDItemWhereFroms` to identify the origin URL of a large artifact (e.g., "This 10GB folder was downloaded from `huggingface.co`").
- **`reflink-copy`**: Identifies cloned or sparse files to provide a more accurate "Actual Reclaimed Space" estimate.
- **`mdquery-rs` / `mdls`**: Interfaces with the Spotlight index to instantly find files by metadata (e.g., "Find all `zip` files downloaded more than 6 months ago that have not been opened").

**Impact:** Significantly improves the "Evidence" gate of the Gall Checkpoints by providing verifiable provenance for every cleanup candidate.

---

## 4. Application Lifecycle & Package Analysis
**Focus:** Managing the state of software distributions.

### Recommended Integration:
- **`plist`**: Standardizes the parsing of `Info.plist` and `config.plist` files.
- **`apple-bundles` / `goblin`**: Analyzes `.app` bundles to detect Rosetta 2 (Intel) apps on Apple Silicon and identifies orphaned `.dylib` references.
- **`homebrew`**: Integrates with the `brew` graph to find cached files that belong to uninstalled or old versions of packages.

**Impact:** Transforms `tool-roots` from a folder scanner into a lifecycle manager that identifies legacy or redundant software state.

---

## 5. Telemetry & Persistence (Security Layer)
**Focus:** Identifying "Zombie" processes and startup residue.

### Recommended Integration:
- **`persistence` / `launchd`**: Scans `~/Library/LaunchAgents` to find background tasks pointing to missing or deleted binaries.
- **`macos-unifiedlogs`**: Correlates install events with current disk artifacts.
- **`endpoint-sec`**: (Advanced) Provides real-time monitoring of which processes are creating the most artifacts.

**Impact:** Introduces a "Security & Persistence" audit, ensuring that a cleanup actually *stays* clean by removing the agents that recreate junk.

---

## Strategic Roadmap Conclusion
The highest priority next step for the project's "Promotion" beyond G9 is the integration of **`objc2-foundation`** for path resolution and **`plist`** for application analysis. These provide the strongest foundation for moving from a "heuristics-based" tool to a "system-aware" utility.
