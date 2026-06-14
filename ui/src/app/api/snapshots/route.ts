import { exec } from "child_process";
import { promisify } from "util";
import { NextResponse } from "next/server";
import path from "path";

const execAsync = promisify(exec);
const BINARY = path.resolve(process.cwd(), "../target/release/oclnr");

export const dynamic = "force-dynamic";

export type Snapshot = {
  name: string;
  kind: "time-machine" | "os-update" | "other";
  date: string | null;
};

export type SnapshotsResponse = {
  volume: string;
  snapshots: Snapshot[];
  captured_at_unix: number;
};

function classifySnapshot(name: string): Snapshot {
  if (name.startsWith("com.apple.TimeMachine.")) {
    const dateStr = name.replace("com.apple.TimeMachine.", "").replace(".local", "");
    return { name, kind: "time-machine", date: dateStr };
  }
  if (name.startsWith("com.apple.os.update-")) {
    return { name, kind: "os-update", date: null };
  }
  return { name, kind: "other", date: null };
}

export async function GET() {
  const { stdout } = await execAsync(
    `${BINARY} snapshot audit 2>/dev/null`
  );

  const lines = stdout.split("\n");
  const volumeLine = lines.find((l) => l.startsWith("Auditing local snapshots for:"));
  const volume = volumeLine ? volumeLine.split(": ")[1]?.trim() ?? "/" : "/";

  const snapshots: Snapshot[] = lines
    .filter((l) => l.trim().startsWith("- "))
    .map((l) => l.trim().replace(/^- /, ""))
    .map(classifySnapshot);

  return NextResponse.json<SnapshotsResponse>({
    volume,
    snapshots,
    captured_at_unix: Math.floor(Date.now() / 1000),
  });
}
