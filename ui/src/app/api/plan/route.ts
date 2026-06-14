import { NextResponse } from "next/server";
import { getPlan } from "@/data/oclnr";

export const dynamic = "force-dynamic";

export async function GET() {
  const data = await getPlan();
  return NextResponse.json(data);
}
