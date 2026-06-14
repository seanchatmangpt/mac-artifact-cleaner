import { NextResponse } from "next/server";
import { getDoctests } from "@/data/oclnr";

export const dynamic = "force-dynamic";

export async function GET() {
  const data = await getDoctests();
  return NextResponse.json(data);
}
