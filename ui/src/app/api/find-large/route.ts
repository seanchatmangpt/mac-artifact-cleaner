import { NextRequest, NextResponse } from "next/server";
import { findLargeFiles } from "@/data/oclnr";

export const dynamic = "force-dynamic";

export async function GET(req: NextRequest) {
  const minMb = parseInt(req.nextUrl.searchParams.get("min_mb") ?? "100") || 100;
  const data = await findLargeFiles(minMb, 60000);
  return NextResponse.json(data);
}
