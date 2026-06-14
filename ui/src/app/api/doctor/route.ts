import { exec } from "child_process";
import { promisify } from "util";
import { NextResponse } from "next/server";
import path from "path";

const execAsync = promisify(exec);
const PROJECT_ROOT = path.resolve(process.cwd(), "..");
const BINARY = path.resolve(PROJECT_ROOT, "target/release/oclnr");

export const dynamic = "force-dynamic";

export type CheckResult = {
  name: string;
  passed: boolean;
  output: string;
};

export type DoctorResponse = {
  checks: CheckResult[];
  all_passed: boolean;
  captured_at_unix: number;
};

async function runCheck(name: string, args: string): Promise<CheckResult> {
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
}

export async function GET() {
  const checks = await Promise.all([
    runCheck("Architecture", "doctor architecture"),
    runCheck("Substrate", "doctor substrate"),
  ]);

  return NextResponse.json<DoctorResponse>({
    checks,
    all_passed: checks.every((c) => c.passed),
    captured_at_unix: Math.floor(Date.now() / 1000),
  });
}
