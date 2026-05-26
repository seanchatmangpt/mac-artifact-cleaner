# mac-disk-auditor

**A plan-bound macOS developer disk auditor and cleanup utility. It observes first, emits reviewable OCEL evidence, deletes only from approved plans, and records receipts.**

Unlike traditional cleaners that blindly execute `rm -rf` from a live scan, `mac-disk-auditor` enforces a strict, multi-phase execution pipeline based on Gall's Law: never increase destructive power without simultaneously increasing receipts.

## Architecture & Workflow

Deletion is plan-bound by design:

1. **Audit observes:** The filesystem is scanned with intelligent traversal barriers to avoid crawling massive dependencies (like `node_modules` or `target`) while accurately inventorying hidden tool caches (`.cargo`, `.cache`, `.npm`, etc.).
2. **Plan proposes:** A dry run generates a reviewable JSON plan identifying cleanup candidates based on age, size, and tool-specific heuristics.
3. **Human reviews:** The user inspects the plan or the emitted Object-Centric Event Log (OCEL v2) to verify what will be deleted.
4. **Delete executes only from a saved plan:** The scanner is disabled during deletion. The utility reads the reviewed plan and strictly deletes only the exact paths listed.
5. **Receipt records the result:** Progress and consequences are tracked without fresh discovery.

## Privacy and Safety

This tool is safe to publish as source code, but its generated reports are machine-local evidence files.

**Do not commit real output files such as:**

- `disk-audit.json`
- `disk-audit.jsonocel`
- `cleanup-plan.json`
- `cleanup-plan.jsonocel`
- `deletion-receipt.jsonocel`

These files can contain absolute paths, project names, hidden tool directories, timestamps, file sizes, and local development patterns. The included `.gitignore` will protect against accidental commits of these file patterns.

*(If you wish to share examples or file bug reports, a `--redact` flag is planned to sanitize local ontology paths like `/Users/user/...` into safe equivalents like `$HOME/workspace/project-a/...`)*

## Documentation
- [Gall Checkpoints: The Evolution of mac-artifact-cleaner](docs/GALL_CHECKPOINTS.md)
