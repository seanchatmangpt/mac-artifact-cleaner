//! Demonstrates `human_bytes` and `parse_size_in_bytes` — the crate's single
//! canonical SI/1000 size convention.
//!
//! Documented at: `src/domain/time.rs` (`human_bytes`, `parse_size_in_bytes`)
//! README reference: "Receipt records the result" workflow step
//!
//! What you should see:
//!   parse "1GB"   -> 1000000000 bytes
//!   format 1 GB   -> "1.00 GB"
//!   round-trip    -> lossless at unit boundaries
//!   parse "500mb" -> 500000000 bytes
//!   format 500 MB -> "500.00 MB"

use osx_clnr::domain::time::{human_bytes, parse_size_in_bytes};

fn main() {
    // ── parse → integer ───────────────────────────────────────────────────────
    let gb = parse_size_in_bytes("1GB").expect("1GB must parse");
    println!("parse \"1GB\"   -> {} bytes", gb);
    assert_eq!(gb, 1_000_000_000, "1GB must be 1_000_000_000 (SI/1000)");

    let mb = parse_size_in_bytes("500mb").expect("500mb must parse");
    println!("parse \"500mb\" -> {} bytes", mb);
    assert_eq!(mb, 500_000_000);

    // ── integer → human ───────────────────────────────────────────────────────
    let formatted_gb = human_bytes(1_000_000_000);
    println!("format 1 GB   -> {:?}", formatted_gb);
    assert_eq!(formatted_gb, "1.00 GB", "SI convention: 1_000_000_000 -> \"1.00 GB\"");

    let formatted_mb = human_bytes(500_000_000);
    println!("format 500 MB -> {:?}", formatted_mb);
    assert_eq!(formatted_mb, "500.00 MB");

    // ── round-trip: parse then format is lossless at unit boundaries ──────────
    let rt = human_bytes(parse_size_in_bytes("1GB").unwrap());
    println!("round-trip    -> {:?}", rt);
    assert_eq!(rt, "1.00 GB", "round-trip must be lossless");

    // ── refusal: invalid input is rejected ────────────────────────────────────
    assert!(parse_size_in_bytes("notasize").is_err(), "invalid input must error");
    assert!(parse_size_in_bytes("").is_err(), "empty string must error");

    println!("size_units: all assertions passed");
}
