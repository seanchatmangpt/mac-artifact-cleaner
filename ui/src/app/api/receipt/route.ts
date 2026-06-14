import { NextResponse } from "next/server";
import fs from "fs/promises";
import path from "path";

export const dynamic = "force-dynamic";

const PROJECT_ROOT = path.resolve(process.cwd(), "..");

export type SnapshotThinReceipt = {
  volume: string;
  requested_bytes: number;
  timestamp_unix: number;
  snapshots_before: string[];
  snapshots_after: string[];
  snapshots_thinned: string[];
};

export type ReceiptResponse = {
  snapshot_thin: SnapshotThinReceipt | null;
  found_at: string | null;
};

export async function GET() {
  const receiptPath = path.join(PROJECT_ROOT, "snapshot-thin-receipt.json");
  try {
    const raw = await fs.readFile(receiptPath, "utf-8");
    const receipt = JSON.parse(raw) as SnapshotThinReceipt;
    return NextResponse.json<ReceiptResponse>({
      snapshot_thin: receipt,
      found_at: receiptPath,
    });
  } catch {
    return NextResponse.json<ReceiptResponse>({ snapshot_thin: null, found_at: null });
  }
}
