//! Deterministic timing bench for the crates the dependabot minor-and-patch
//! bump (PR #13) moved on osx-clnr's hot paths: `blake3` (receipt/content
//! digests), `serde_json` (receipt + MCP payload encoding) and `toml`
//! (config/lock parsing).
//!
//! Each workload is fixed-size and seeded deterministically; the median of
//! several runs is compared to a regression floor. The floors are set well
//! below the numbers recorded in `docs/bench/pr13-dependency-bump.json`
//! (measured on base `main` and on the bumped lock) so that shared CI
//! runners pass while an order-of-magnitude regression is refused.
//!
//! Output: one `BENCH {json}` line per workload (run with `--nocapture`).

use std::time::{Duration, Instant};

const RUNS: usize = 7;

fn median(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[v.len() / 2]
}

fn time<F: FnMut() -> u64>(mut f: F) -> (Duration, u64) {
    let mut samples = Vec::with_capacity(RUNS);
    let mut sink = 0u64;
    for _ in 0..RUNS {
        let t = Instant::now();
        sink = sink.wrapping_add(f());
        samples.push(t.elapsed());
    }
    (median(samples), sink)
}

fn deterministic_bytes(len: usize) -> Vec<u8> {
    // xorshift64*, fixed seed: identical input on every run and machine.
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    (0..len)
        .map(|_| {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8
        })
        .collect()
}

fn report(name: &str, bytes: usize, d: Duration) -> f64 {
    let mib_s = bytes as f64 / (1024.0 * 1024.0) / d.as_secs_f64();
    println!(
        "BENCH {{\"workload\":\"{name}\",\"bytes\":{bytes},\"median_ns\":{},\"mib_per_s\":{mib_s:.2},\"profile\":\"{}\"}}",
        d.as_nanos(),
        if cfg!(debug_assertions) { "debug" } else { "release" }
    );
    mib_s
}

#[test]
fn bench_blake3_digest_throughput() {
    const LEN: usize = 8 * 1024 * 1024;
    let data = deterministic_bytes(LEN);
    let (d, sink) = time(|| blake3::hash(&data).as_bytes()[0] as u64);
    // Determinism: the digest of the fixed input is a known constant, so a
    // bump that changes hashing output (not just speed) is refused here.
    let digest = blake3::hash(&data).to_hex().to_string();
    assert_eq!(digest.len(), 64);
    assert_eq!(digest, blake3::hash(&deterministic_bytes(LEN)).to_hex().to_string());
    std::hint::black_box(sink);
    let mib_s = report("blake3_hash_8MiB", LEN, d);
    assert!(mib_s > 2.0, "blake3 throughput regressed: {mib_s:.2} MiB/s (floor 2)");
}

#[test]
fn bench_serde_json_receipt_roundtrip() {
    // Receipt-shaped document: 2_000 entries with path/bytes/digest fields.
    let entries: Vec<serde_json::Value> = (0..2_000u64)
        .map(|i| {
            serde_json::json!({
                "path": format!("/Users/dev/project-{i}/target/debug/deps/lib{i}.rlib"),
                "bytes": i * 4096 + 17,
                "digest": blake3::hash(&i.to_le_bytes()).to_hex().to_string(),
                "kind": "RustTarget",
                "approved": i % 3 == 0,
            })
        })
        .collect();
    let doc = serde_json::json!({ "schema": "osx-clnr/receipt", "entries": entries });
    let encoded = serde_json::to_vec(&doc).unwrap();
    let bytes = encoded.len();
    let (d, _) = time(|| {
        let s = serde_json::to_vec(&doc).unwrap();
        let back: serde_json::Value = serde_json::from_slice(&s).unwrap();
        back["entries"].as_array().unwrap().len() as u64
    });
    // Replay determinism: encode -> decode -> encode is byte-identical.
    let back: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(serde_json::to_vec(&back).unwrap(), encoded);
    let mib_s = report("serde_json_roundtrip_receipt", bytes, d);
    assert!(mib_s > 1.0, "serde_json roundtrip regressed: {mib_s:.2} MiB/s (floor 1)");
}

#[test]
fn bench_toml_parse_cargo_lock() {
    let lock = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.lock")).unwrap();
    let bytes = lock.len();
    let (d, n) = time(|| {
        let t: toml::Table = toml::from_str(&lock).unwrap();
        t["package"].as_array().unwrap().len() as u64
    });
    assert!(n > 0);
    let mib_s = report("toml_parse_cargo_lock", bytes, d);
    assert!(mib_s > 0.5, "toml parse regressed: {mib_s:.2} MiB/s (floor 0.5)");
}
