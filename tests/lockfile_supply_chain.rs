//! Supply-chain court for `Cargo.lock` (hardening for the dependabot
//! minor-and-patch bump, PR #13).
//!
//! A lockfile bump claims three things: the lock is well-formed, every
//! registry package carries a real content digest, and the locked set both
//! satisfies the manifest and stays above the advisory floors that
//! `cargo deny check advisories` enforced when the bump was admitted.
//! Each claim is checked against the real `Cargo.lock` / `Cargo.toml` on
//! disk, and each check is proven non-vacuous by feeding it a mutated lock
//! that it must refuse (malformed input, wrong digest, duplicate entry,
//! advisory downgrade, stale lock below the manifest requirement).

use std::collections::{BTreeMap, BTreeSet};

/// Advisory floors observed at admission (2026-09-26, cargo-deny 0.20.2):
/// the base lock on `main` failed `advisories` on these crates; the bumped
/// lock passes. A lock that regresses below any floor is refused.
const ADVISORY_FLOORS: &[(&str, (u64, u64, u64), &str)] = &[
    ("crossbeam-epoch", (0, 9, 20), "RUSTSEC-2026-0204 fmt::Pointer invalid deref"),
    ("anyhow", (1, 0, 104), "Error::downcast_mut unsoundness (flagged on 1.0.102)"),
    ("quick-xml", (0, 41, 0), "duplicate-attribute quadratic time / NsReader alloc DoS"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct LockedPackage {
    name: String,
    version: (u64, u64, u64),
    raw_version: String,
    source: Option<String>,
    checksum: Option<String>,
}

fn parse_version(raw: &str) -> Result<(u64, u64, u64), String> {
    // Drop build metadata (`1.1.6+spec-1.1.0`) and pre-release (`-rc.1`).
    let core = raw.split('+').next().unwrap_or(raw);
    let core = core.split('-').next().unwrap_or(core);
    let mut parts = core.split('.');
    let mut next = |label: &str| -> Result<u64, String> {
        match parts.next() {
            None => Ok(0),
            Some(p) => p.parse::<u64>().map_err(|e| format!("bad {label} in version {raw:?}: {e}")),
        }
    };
    let v = (next("major")?, next("minor")?, next("patch")?);
    if parts.next().is_some() {
        return Err(format!("too many components in version {raw:?}"));
    }
    Ok(v)
}

/// Parse and validate a lockfile's text. Refuses: non-TOML input, a lock
/// format other than v3/v4, a registry package without a 64-hex-digit
/// checksum, and a duplicated `(name, version)` entry.
fn admit_lock(text: &str) -> Result<Vec<LockedPackage>, String> {
    let table: toml::Table = toml::from_str(text).map_err(|e| format!("malformed lock: {e}"))?;
    match table.get("version").and_then(toml::Value::as_integer) {
        Some(3) | Some(4) => {}
        other => return Err(format!("unsupported lock format version: {other:?}")),
    }
    let pkgs = table
        .get("package")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "lock has no [[package]] array".to_string())?;

    let mut out = Vec::with_capacity(pkgs.len());
    let mut seen = BTreeSet::new();
    for p in pkgs {
        let name = p
            .get("name")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| format!("package without name: {p:?}"))?
            .to_string();
        let raw_version = p
            .get("version")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| format!("package {name} without version"))?
            .to_string();
        let version = parse_version(&raw_version)?;
        let source = p.get("source").and_then(toml::Value::as_str).map(str::to_string);
        let checksum = p.get("checksum").and_then(toml::Value::as_str).map(str::to_string);

        if source.as_deref().is_some_and(|s| s.starts_with("registry+")) {
            let ok = checksum.as_deref().is_some_and(|c| {
                c.len() == 64 && c.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            });
            if !ok {
                return Err(format!(
                    "registry package {name} {raw_version} has missing/invalid checksum {checksum:?}"
                ));
            }
        }
        if !seen.insert((name.clone(), raw_version.clone())) {
            return Err(format!("duplicate lock entry {name} {raw_version}"));
        }
        out.push(LockedPackage { name, version, raw_version, source, checksum });
    }
    Ok(out)
}

/// Caret-compatibility of a locked version against a plain manifest
/// requirement (`"1"`, `"1.8"`, `"1.8.5"`, `"26.6.2"`).
fn caret_satisfies(req: (u64, u64, u64), have: (u64, u64, u64)) -> bool {
    if have < req {
        return false;
    }
    match req {
        (0, 0, p) => have == (0, 0, p),
        (0, m, _) => have.0 == 0 && have.1 == m,
        (maj, _, _) => have.0 == maj,
    }
}

/// Plain `[dependencies]` requirements from the manifest: name -> req.
/// Non-plain requirements (`=`, `>=`, `~`, `*`, path/git deps) are skipped;
/// they are not what dependabot's minor-and-patch group moves.
fn manifest_requirements(manifest: &str) -> BTreeMap<String, (u64, u64, u64)> {
    let table: toml::Table = toml::from_str(manifest).expect("Cargo.toml parses");
    let mut out = BTreeMap::new();
    let Some(deps) = table.get("dependencies").and_then(toml::Value::as_table) else {
        return out;
    };
    for (name, spec) in deps {
        let req = match spec {
            toml::Value::String(s) => Some(s.as_str()),
            toml::Value::Table(t) => {
                let pkg_name = t.get("package").and_then(toml::Value::as_str);
                if pkg_name.is_some() || t.contains_key("path") || t.contains_key("git") {
                    None
                } else {
                    t.get("version").and_then(toml::Value::as_str)
                }
            }
            _ => None,
        };
        let Some(req) = req else { continue };
        let req = req.trim().trim_start_matches('^');
        if req.is_empty() || !req.bytes().next().is_some_and(|b| b.is_ascii_digit()) {
            continue;
        }
        if let Ok(v) = parse_version(req) {
            out.insert(name.clone(), v);
        }
    }
    out
}

/// Refuse a lock that no longer satisfies the manifest (stale subject) or
/// regresses below an advisory floor.
fn admit_against_manifest(
    pkgs: &[LockedPackage],
    reqs: &BTreeMap<String, (u64, u64, u64)>,
) -> Result<(), String> {
    for (name, req) in reqs {
        let candidates: Vec<_> = pkgs.iter().filter(|p| &p.name == name).collect();
        if candidates.is_empty() {
            return Err(format!("manifest dependency {name} missing from lock"));
        }
        if !candidates.iter().any(|p| caret_satisfies(*req, p.version)) {
            let have: Vec<_> = candidates.iter().map(|p| p.raw_version.as_str()).collect();
            return Err(format!("stale lock: {name} requires ^{req:?}, lock has {have:?}"));
        }
    }
    for (name, floor, why) in ADVISORY_FLOORS {
        for p in pkgs.iter().filter(|p| &p.name == name) {
            if p.version < *floor {
                return Err(format!(
                    "advisory regression: {name} {} below floor {floor:?} ({why})",
                    p.raw_version
                ));
            }
        }
    }
    Ok(())
}

fn real_lock() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.lock")).unwrap()
}

fn real_manifest() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap()
}

#[test]
fn real_lock_is_admitted() {
    let pkgs = admit_lock(&real_lock()).expect("real Cargo.lock must be admitted");
    assert!(pkgs.len() > 100, "lock unexpectedly small: {} packages", pkgs.len());
    let reqs = manifest_requirements(&real_manifest());
    assert!(reqs.len() >= 20, "manifest requirement extraction vacuous: {reqs:?}");
    admit_against_manifest(&pkgs, &reqs).expect("real lock must satisfy manifest and floors");
}

#[test]
fn every_advisory_floor_crate_is_present() {
    // Non-vacuity: a floor over a crate that is not in the lock checks nothing.
    let pkgs = admit_lock(&real_lock()).unwrap();
    for (name, _, _) in ADVISORY_FLOORS {
        assert!(pkgs.iter().any(|p| &p.name == name), "floor crate {name} absent from lock");
    }
}

#[test]
fn registry_packages_all_carry_checksums() {
    let pkgs = admit_lock(&real_lock()).unwrap();
    let registry: Vec<_> = pkgs
        .iter()
        .filter(|p| p.source.as_deref().is_some_and(|s| s.starts_with("registry+")))
        .collect();
    assert!(!registry.is_empty());
    assert!(registry.iter().all(|p| p.checksum.as_ref().is_some_and(|c| c.len() == 64)));
}

#[test]
fn refuses_malformed_lock() {
    let err = admit_lock("version = 4\n[[package]\nname = ").unwrap_err();
    assert!(err.starts_with("malformed lock"), "{err}");
    let err = admit_lock("").unwrap_err();
    assert!(err.contains("unsupported lock format"), "{err}");
    let err = admit_lock("version = 1\n").unwrap_err();
    assert!(err.contains("unsupported lock format"), "{err}");
}

#[test]
fn refuses_wrong_digest() {
    let lock = real_lock();
    // Truncate the first checksum by one hex digit.
    let idx = lock.find("checksum = \"").unwrap() + "checksum = \"".len();
    let mut short = lock.clone();
    short.remove(idx);
    let err = admit_lock(&short).unwrap_err();
    assert!(err.contains("invalid checksum"), "{err}");

    // Non-hex digest of the right length.
    let mut nonhex = lock.clone();
    nonhex.replace_range(idx..idx + 1, "Z");
    let err = admit_lock(&nonhex).unwrap_err();
    assert!(err.contains("invalid checksum"), "{err}");

    // Stripped checksum on a registry package.
    let line_end = lock[idx..].find('\n').unwrap() + idx;
    let line_start = lock[..idx].rfind('\n').unwrap();
    let mut stripped = lock.clone();
    stripped.replace_range(line_start..line_end, "");
    let err = admit_lock(&stripped).unwrap_err();
    assert!(err.contains("missing/invalid checksum"), "{err}");
}

#[test]
fn refuses_duplicate_delivery_of_same_package() {
    let lock = real_lock();
    let start = lock.find("[[package]]").unwrap();
    let end = lock[start + 1..].find("[[package]]").unwrap() + start + 1;
    let first = &lock[start..end];
    let doubled = format!("{lock}\n{first}");
    let err = admit_lock(&doubled).unwrap_err();
    assert!(err.starts_with("duplicate lock entry"), "{err}");
}

#[test]
fn reordered_lock_is_still_admitted_identically() {
    // Package order is not semantic: reversing [[package]] blocks must not
    // change the admitted set.
    let lock = real_lock();
    let first = lock.find("[[package]]").unwrap();
    let (header, body) = lock.split_at(first);
    let mut blocks: Vec<&str> =
        body.split("[[package]]").filter(|b| !b.trim().is_empty()).collect();
    blocks.reverse();
    let reordered =
        format!("{header}{}", blocks.iter().map(|b| format!("[[package]]{b}")).collect::<String>());
    let mut a = admit_lock(&lock).unwrap();
    let mut b = admit_lock(&reordered).unwrap();
    a.sort_by(|x, y| (&x.name, &x.raw_version).cmp(&(&y.name, &y.raw_version)));
    b.sort_by(|x, y| (&x.name, &x.raw_version).cmp(&(&y.name, &y.raw_version)));
    assert_eq!(a, b);
}

#[test]
fn refuses_advisory_downgrade() {
    let mut pkgs = admit_lock(&real_lock()).unwrap();
    let reqs = manifest_requirements(&real_manifest());
    let p = pkgs.iter_mut().find(|p| p.name == "crossbeam-epoch").unwrap();
    p.version = (0, 9, 18);
    p.raw_version = "0.9.18".into();
    let err = admit_against_manifest(&pkgs, &reqs).unwrap_err();
    assert!(err.contains("RUSTSEC-2026-0204"), "{err}");
}

#[test]
fn refuses_stale_lock_below_manifest_requirement() {
    let mut pkgs = admit_lock(&real_lock()).unwrap();
    let reqs = manifest_requirements(&real_manifest());
    let req = reqs["blake3"];
    assert_eq!(req, (1, 8, 5));
    for p in pkgs.iter_mut().filter(|p| p.name == "blake3") {
        p.version = (1, 8, 4);
        p.raw_version = "1.8.4".into();
    }
    let err = admit_against_manifest(&pkgs, &reqs).unwrap_err();
    assert!(err.starts_with("stale lock: blake3"), "{err}");
}

#[test]
fn refuses_major_jump_outside_caret_range() {
    let mut pkgs = admit_lock(&real_lock()).unwrap();
    let reqs = manifest_requirements(&real_manifest());
    for p in pkgs.iter_mut().filter(|p| p.name == "serde_json") {
        p.version = (2, 0, 0);
        p.raw_version = "2.0.0".into();
    }
    let err = admit_against_manifest(&pkgs, &reqs).unwrap_err();
    assert!(err.starts_with("stale lock: serde_json"), "{err}");
}

#[test]
fn refuses_dependency_missing_from_lock() {
    let pkgs: Vec<_> =
        admit_lock(&real_lock()).unwrap().into_iter().filter(|p| p.name != "sled").collect();
    let reqs = manifest_requirements(&real_manifest());
    let err = admit_against_manifest(&pkgs, &reqs).unwrap_err();
    assert!(err.contains("sled missing from lock"), "{err}");
}

#[test]
fn caret_semantics_boundaries() {
    assert!(caret_satisfies((1, 8, 5), (1, 8, 7)));
    assert!(caret_satisfies((1, 8, 5), (1, 9, 0)));
    assert!(!caret_satisfies((1, 8, 5), (1, 8, 4)));
    assert!(!caret_satisfies((1, 8, 5), (2, 0, 0)));
    assert!(caret_satisfies((0, 12, 0), (0, 12, 3)));
    assert!(!caret_satisfies((0, 12, 0), (0, 13, 0)));
    assert!(caret_satisfies((0, 0, 3), (0, 0, 3)));
    assert!(!caret_satisfies((0, 0, 3), (0, 0, 4)));
    assert_eq!(parse_version("1.1.6+spec-1.1.0"), Ok((1, 1, 6)));
    assert_eq!(parse_version("26"), Ok((26, 0, 0)));
    assert!(parse_version("1.x").is_err());
    assert!(parse_version("1.2.3.4").is_err());
}
