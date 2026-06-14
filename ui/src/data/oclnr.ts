import { exec } from "child_process";
import { promisify } from "util";
import { connection } from "next/server";
import fs from "fs/promises";
import path from "path";

const execAsync = promisify(exec);

// Absolute path to the compiled binary, relative to this Next.js project root
const BINARY = path.resolve(process.cwd(), "../target/release/oclnr");
const PROJECT_ROOT = path.resolve(process.cwd(), "..");

// ────────────────────────────────────────────────────────────
// Types
// ────────────────────────────────────────────────────────────

export type BreakdownEntry = {
  path: string;
  size_bytes: number;
  size_human: string;
  pct: number;
  category: "tool cache" | "build artifact" | "data" | "project/other";
};

export type BreakdownData = {
  disk_free_gb: number;
  disk_total_gb: number;
  disk_used_pct: number;
  total_scanned_human: string;
  entries: BreakdownEntry[];
};

export type Snapshot = {
  name: string;
  kind: "time-machine" | "os-update" | "other";
  date: string | null;
};

export type SnapshotsData = {
  volume: string;
  snapshots: Snapshot[];
};

export type CheckResult = {
  name: string;
  passed: boolean;
  output: string;
};

export type DoctorData = {
  checks: CheckResult[];
  all_passed: boolean;
};

export type SnapshotThinReceipt = {
  volume: string;
  requested_bytes: number;
  timestamp_unix: number;
  snapshots_before: string[];
  snapshots_after: string[];
  snapshots_thinned: string[];
};

// ────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────

function stripAnsi(s: string): string {
  // eslint-disable-next-line no-control-regex
  return s.replace(/\x1b\[[0-9;]*m/g, "");
}

function parseSize(s: string): number {
  const num = parseFloat(s);
  if (s.endsWith("TB")) return num * 1_000_000_000_000;
  if (s.endsWith("GB")) return num * 1_000_000_000;
  if (s.endsWith("MB")) return num * 1_000_000;
  if (s.endsWith("KB")) return num * 1_000;
  return num;
}

// ────────────────────────────────────────────────────────────
// Data fetchers — each calls connection() to opt out of prerender
// ────────────────────────────────────────────────────────────

export async function getBreakdown(): Promise<BreakdownData> {
  await connection();
  const { stdout } = await execAsync(`${BINARY} audit breakdown --min-mb 10 2>/dev/null`);
  const clean = stripAnsi(stdout);
  const lines = clean.split("\n");

  const diskLine = lines.find((l) => l.includes("free of"));
  let disk_free_gb = 0, disk_total_gb = 0, disk_used_pct = 0;
  if (diskLine) {
    const m = diskLine.match(/([\d.]+)\s*GB free of\s*([\d.]+)\s*GB\s*\((\d+)%/);
    if (m) {
      disk_free_gb = parseFloat(m[1]);
      disk_total_gb = parseFloat(m[2]);
      disk_used_pct = parseInt(m[3]);
    }
  }

  const scannedLine = lines.find((l) => l.includes("Total scanned:"));
  const total_scanned_human = scannedLine
    ? (scannedLine.match(/Total scanned:\s*([\d.]+ \w+)/) || [])[1] ?? ""
    : "";

  const entries: BreakdownEntry[] = [];
  for (const raw of lines) {
    const line = raw.trim();
    const m = line.match(/^(~\/\S+|~)\s+([\d.]+ \w+)\s+([\d.]+)%\s+(.+)$/);
    if (!m) continue;
    const rawCat = m[4].trim();
    const category = (
      rawCat.includes("tool cache") ? "tool cache"
        : rawCat.includes("build artifact") ? "build artifact"
        : rawCat.includes("data") ? "data"
        : "project/other"
    ) as BreakdownEntry["category"];
    entries.push({
      path: m[1],
      size_human: m[2],
      size_bytes: parseSize(m[2]),
      pct: parseFloat(m[3]),
      category,
    });
  }

  return { disk_free_gb, disk_total_gb, disk_used_pct, total_scanned_human, entries };
}

export async function getSnapshots(): Promise<SnapshotsData> {
  await connection();
  const { stdout } = await execAsync(`${BINARY} snapshot audit 2>/dev/null`);
  const lines = stdout.split("\n");
  const volumeLine = lines.find((l) => l.startsWith("Auditing local snapshots for:"));
  const volume = volumeLine ? volumeLine.split(": ")[1]?.trim() ?? "/" : "/";

  const snapshots: Snapshot[] = lines
    .filter((l) => l.trim().startsWith("- "))
    .map((l) => l.trim().replace(/^- /, ""))
    .map((name): Snapshot => {
      if (name.startsWith("com.apple.TimeMachine.")) {
        const date = name.replace("com.apple.TimeMachine.", "").replace(".local", "");
        return { name, kind: "time-machine", date };
      }
      if (name.startsWith("com.apple.os.update-")) return { name, kind: "os-update", date: null };
      return { name, kind: "other", date: null };
    });

  return { volume, snapshots };
}

export async function getDoctor(): Promise<DoctorData> {
  await connection();
  const run = async (name: string, args: string): Promise<CheckResult> => {
    try {
      const { stdout, stderr } = await execAsync(`${BINARY} ${args} 2>&1`, { cwd: PROJECT_ROOT });
      const output = (stdout + stderr).trim();
      const passed = output.includes("✅") || output.includes("passed") || output.includes("successfully");
      return { name, passed, output };
    } catch (e: unknown) {
      const err = e as { stdout?: string; stderr?: string; message?: string };
      const output = ((err.stdout ?? "") + (err.stderr ?? "") || err.message || String(e)).trim();
      return { name, passed: false, output };
    }
  };

  const checks = await Promise.all([
    run("Architecture", "doctor architecture"),
    run("Substrate", "doctor substrate"),
  ]);
  return { checks, all_passed: checks.every((c) => c.passed) };
}

export async function getReceipt(): Promise<SnapshotThinReceipt | null> {
  await connection();
  try {
    const raw = await fs.readFile(path.join(PROJECT_ROOT, "snapshot-thin-receipt.json"), "utf-8");
    return JSON.parse(raw) as SnapshotThinReceipt;
  } catch {
    return null;
  }
}
