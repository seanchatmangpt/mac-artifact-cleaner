# Privacy Model and Redaction Guidelines

This document outlines the privacy design and repository safety standards for `pentecost`. 

Developer machines contain highly sensitive data: usernames, private repository paths, proprietary directory layouts, and API keys. Because this project produces reviewable plan files, OCEL event logs, and cleanup receipts, it is crucial that the tool and its contributors enforce a strict privacy boundary.

---

## 1. The Redaction Philosophy

> **No real local machine path or personal identifier should ever be committed to the repository or transmitted over shared channels.**

All output files (like `.json` or `.jsonocel` reports) that are checked into the repository as examples or test fixtures must go through a **Privacy & Redaction Gate** (Gall Checkpoint G8) to strip identifying characteristics.

```text
Local Path:    /Users/sac/dev/company-x/auth-service/node_modules
Redacted Path: /Users/<user>/dev/company-x/auth-service/node_modules
```

---

## 2. Redaction Architecture

The core of the privacy boundary is implemented in [redaction.rs](../src/domain/redaction.rs). It defines how path strings are rewritten to hide local user profiles.

### 2.1 Redaction Strategy

The primary function `redact_path` targets macOS user directories under `/Users/`:

1. If a path starts with `/Users/`, the function splits the path by the directory separator (`/`).
2. The user profile directory (e.g., `/Users/user`) is identified (the third element in the split list).
3. The username element is replaced with `<user>`.
4. The remaining subdirectories (e.g., `dev/project-a/target`) are preserved to maintain structural validity for testing and auditing without leaking the identity of the developer.
5. Non-user paths (e.g., `/System/Library` or `/usr/local/bin`) are left untouched.

### 2.2 Domain Code Reference

Here is the logic in [redaction.rs](../src/domain/redaction.rs):

```rust
pub fn redact_path(path: &str) -> String {
    if path.starts_with("/Users/") {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() > 2 {
            let mut redacted = vec!["", "Users", "<user>"];
            redacted.extend_from_slice(&parts[3..]);
            return redacted.join("/");
        }
    }
    path.to_string()
}
```

> [!NOTE]
> This pattern matches home directories on macOS. It replaces exact paths dynamically and does not touch system directories or general paths that do not leak individual developer usernames.

---

## 3. Sensitive Data Classification

We categorize metadata according to its risk level:

| Classification | Examples | Disposition |
|---|---|---|
| **High Risk (Leakage)** | Home directory name, corporate repo names, custom secrets, local configuration keys. | Must be redacted or excluded. |
| **Medium Risk (Structural)** | Absolute subpaths, sizes, dates, counts. | Permitted in redacted files. |
| **Low Risk (Generic)** | System paths, build artifact directory names (`node_modules`, `target`), classifications. | Freely publishable. |

---

## 4. Repository Exclusion Rules

To guarantee that developers do not accidentally check in their live local plans or receipts, the repository's [.gitignore](../.gitignore) explicitly excludes all execution artifacts:

```text
cleanup-plan*.json
cleanup-plan*.jsonocel
deletion-plan*.json
deletion-plan*.jsonocel
delete-receipt*.json
delete-receipt*.jsonocel
disk-audit*.json
disk-audit*.jsonocel
tool-root-audit*.json
tool-root-audit*.jsonocel
*.log
*.trace
*.receipt
```

> [!IMPORTANT]
> If you need to add a new test fixture or example to the repository (e.g., under `examples/`), you must run it through a redaction process and verify that no real user directories or private names are present.

---

## 5. Privacy Verification in CI

The development workflow requires the privacy check to pass before any checkpoint promotion or release. When writing integration tests:
- Ensure mock home paths are generic (e.g., `/Users/user/` or `/Users/<user>/`).
- Use the privacy verification test suite to ensure redaction rules function properly.
