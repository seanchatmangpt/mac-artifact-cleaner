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

export type DoctestModule = {
  file: string;
  has_module_doc: boolean;
};

export type DoctestData = {
  passed: boolean;
  modules: DoctestModule[];
  functions_checked: number;
  missing_doctests: number;
  missing_functions: string[];
  output: string;
};

export type PrivacyViolation = { file: string; line: number; content: string };

export type PrivacyData = {
  passed: boolean;
  gitignore_ok: boolean;
  sensitive_files: string[];
  unredacted_paths: PrivacyViolation[];
  output: string;
};

export type PlanItem = {
  path: string;
  kind: string;
  reason: string;
  size_human: string;
};

export type PlanData = {
  version: number;
  created: string;
  roots: string[];
  flags: { deps: boolean; aggressive: boolean };
  total_items: number;
  items: PlanItem[];
  found: boolean;
};

export type LargeFile = {
  path: string;
  size_human: string;
  size_bytes: number;
};

export type FindLargeData = {
  min_mb: number;
  files: LargeFile[];
  files_scanned: number;
  timed_out: boolean;
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

function parseDoctests(stdout: string): DoctestData {
  const lines = stdout.split("\n");
  const modules: DoctestModule[] = lines
    .filter((l) => /\w+\.rs\s+: (Yes|No)/.test(l))
    .map((l) => {
      const m = l.match(/(\w+\.rs)\s+: (Yes|No)/);
      return m ? { file: m[1], has_module_doc: m[2] === "Yes" } : null;
    })
    .filter(Boolean) as DoctestModule[];
  const checkedMatch = stdout.match(/Checked public functions count:\s*(\d+)/);
  const missingMatch = stdout.match(/Missing doctests:\s*(\d+)/);
  const functions_checked = checkedMatch ? parseInt(checkedMatch[1]) : 0;
  const missing_doctests = missingMatch ? parseInt(missingMatch[1]) : 0;
  const missing_functions = lines.filter((l) => /^\s+- File:/.test(l)).map((l) => l.trim());
  const passed = missing_doctests === 0 && functions_checked > 0;
  return { passed, modules, functions_checked, missing_doctests, missing_functions, output: stdout.trim() };
}

export async function getDoctests(): Promise<DoctestData> {
  await connection();
  try {
    const { stdout } = await execAsync(`${BINARY} doctor doctests 2>&1`, { cwd: PROJECT_ROOT });
    return parseDoctests(stdout);
  } catch (e: unknown) {
    const err = e as { stdout?: string };
    return parseDoctests(err.stdout ?? "");
  }
}

function parsePrivacy(stdout: string): PrivacyData {
  const lines = stdout.split("\n");
  const gitignore_ok = lines.some((l) => l.includes(".gitignore contains all required patterns"));
  const sensitive_files = lines
    .filter((l) => /^\s+- \.\//.test(l) && !l.includes("->"))
    .map((l) => l.trim().replace(/^- /, ""));
  const unredacted_paths: PrivacyViolation[] = lines
    .filter((l) => /^\s+- .+:\d+ ->/.test(l))
    .map((l) => {
      const m = l.match(/^\s+- (.+?):(\d+) -> (.+)$/);
      return m ? { file: m[1], line: parseInt(m[2]), content: m[3].trim() } : null;
    })
    .filter(Boolean) as PrivacyViolation[];
  const passed = !stdout.includes("❌") && gitignore_ok;
  return { passed, gitignore_ok, sensitive_files, unredacted_paths, output: stdout.trim() };
}

export async function getPrivacy(): Promise<PrivacyData> {
  await connection();
  try {
    const { stdout } = await execAsync(`${BINARY} doctor privacy 2>&1`, { cwd: PROJECT_ROOT });
    return parsePrivacy(stdout);
  } catch (e: unknown) {
    const err = e as { stdout?: string };
    return parsePrivacy(err.stdout ?? "");
  }
}

export async function getPlan(): Promise<PlanData> {
  await connection();
  const planPath = path.join(PROJECT_ROOT, "cleanup-plan.json");
  try {
    const raw = await fs.readFile(planPath, "utf-8");
    const json = JSON.parse(raw) as {
      version: number;
      created_unix: number;
      roots: string[];
      deps: boolean;
      aggressive: boolean;
      items: Array<{ path: string; kind: string; reason: string }>;
    };
    return {
      version: json.version,
      created: new Date(json.created_unix * 1000).toLocaleString(),
      roots: json.roots,
      flags: { deps: json.deps ?? false, aggressive: json.aggressive ?? false },
      total_items: json.items.length,
      items: json.items.slice(0, 20).map((i) => ({
        path: i.path,
        kind: i.kind,
        reason: i.reason,
        size_human: "",
      })),
      found: true,
    };
  } catch {
    return {
      version: 0, created: "", roots: [], flags: { deps: false, aggressive: false },
      total_items: 0, items: [], found: false,
    };
  }
}

// find-large is slow — used only from a dedicated server action page
export async function findLargeFiles(minMb: number = 100, timeoutMs: number = 30000): Promise<FindLargeData> {
  await connection();
  try {
    const { stdout } = await execAsync(
      `${BINARY} audit find-large --min-mb ${minMb} --top 20 2>/dev/null`,
      { cwd: PROJECT_ROOT, timeout: timeoutMs }
    );
    const lines = stdout.split("\n");

    const scannedMatch = stdout.match(/scanned\s+([\d,]+)\s+files/g);
    const files_scanned = scannedMatch
      ? parseInt(scannedMatch[scannedMatch.length - 1].replace(/[^\d]/g, ""))
      : 0;

    const files: LargeFile[] = lines
      .filter((l) => /^\s+[\d.]+ (TB|GB|MB|KB)/.test(l.trim()))
      .map((l) => {
        const m = l.trim().match(/^([\d.]+ (?:TB|GB|MB|KB))\s+(.+)$/);
        return m ? { size_human: m[1], path: m[2], size_bytes: parseSize(m[1]) } : null;
      })
      .filter(Boolean) as LargeFile[];

    return { min_mb: minMb, files, files_scanned, timed_out: false };
  } catch (e: unknown) {
    const err = e as { killed?: boolean; stdout?: string };
    const timed_out = err.killed === true;
    const existing = ((err.stdout ?? "").match(/scanned\s+([\d,]+)\s+files/g) ?? []);
    const files_scanned = existing.length
      ? parseInt(existing[existing.length - 1].replace(/[^\d]/g, ""))
      : 0;
    return { min_mb: minMb, files: [], files_scanned, timed_out };
  }
}
