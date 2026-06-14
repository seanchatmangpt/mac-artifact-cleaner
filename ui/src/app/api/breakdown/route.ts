import { exec } from "child_process";
import { promisify } from "util";
import { NextResponse } from "next/server";
import path from "path";

const execAsync = promisify(exec);

const BINARY = path.resolve(process.cwd(), "../target/release/oclnr");

export const dynamic = "force-dynamic";

export type BreakdownEntry = {
  path: string;
  size_bytes: number;
  size_human: string;
  pct: number;
  category: "tool cache" | "build artifact" | "data" | "project/other";
};

export type BreakdownResponse = {
  disk_free_gb: number;
  disk_total_gb: number;
  disk_used_pct: number;
  total_scanned_human: string;
  entries: BreakdownEntry[];
  captured_at_unix: number;
};

function parseSize(s: string): number {
  const num = parseFloat(s);
  if (s.endsWith("TB")) return num * 1_000_000_000_000;
  if (s.endsWith("GB")) return num * 1_000_000_000;
  if (s.endsWith("MB")) return num * 1_000_000;
  if (s.endsWith("KB")) return num * 1_000;
  return num;
}

function stripAnsi(s: string): string {
  // eslint-disable-next-line no-control-regex
  return s.replace(/\x1b\[[0-9;]*m/g, "");
}

export async function GET() {
  const { stdout } = await execAsync(
    `${BINARY} audit breakdown --min-mb 10 2>/dev/null`
  );

  const clean = stripAnsi(stdout);
  const lines = clean.split("\n");

  // Parse "Disk /: X free of Y (Z% used)"
  const diskLine = lines.find((l) => l.includes("free of"));
  let diskFreeGb = 0;
  let diskTotalGb = 0;
  let diskUsedPct = 0;
  if (diskLine) {
    const m = diskLine.match(/([\d.]+)\s*GB free of\s*([\d.]+)\s*GB\s*\((\d+)%/);
    if (m) {
      diskFreeGb = parseFloat(m[1]);
      diskTotalGb = parseFloat(m[2]);
      diskUsedPct = parseInt(m[3]);
    }
  }

  // Parse "Total scanned: X"
  const scannedLine = lines.find((l) => l.includes("Total scanned:"));
  const totalScannedHuman = scannedLine
    ? (scannedLine.match(/Total scanned:\s*([\d.]+ \w+)/) || [])[1] ?? ""
    : "";

  const entries: BreakdownEntry[] = [];
  for (const raw of lines) {
    const line = raw.trim();
    // Entry lines look like: "~/Library   86.29 GB   23.2%  data"
    const m = line.match(/^(~\/\S+|~)\s+([\d.]+ \w+)\s+([\d.]+)%\s+(.+)$/);
    if (!m) continue;
    const rawCat = m[4].trim();
    const category = (
      rawCat.includes("tool cache")
        ? "tool cache"
        : rawCat.includes("build artifact")
        ? "build artifact"
        : rawCat.includes("data")
        ? "data"
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

  const body: BreakdownResponse = {
    disk_free_gb: diskFreeGb,
    disk_total_gb: diskTotalGb,
    disk_used_pct: diskUsedPct,
    total_scanned_human: totalScannedHuman,
    entries,
    captured_at_unix: Math.floor(Date.now() / 1000),
  };

  return NextResponse.json(body);
}
