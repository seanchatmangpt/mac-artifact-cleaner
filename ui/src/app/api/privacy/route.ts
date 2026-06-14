import { NextResponse } from "next/server";
import { getPrivacy } from "@/data/oclnr";

export const dynamic = "force-dynamic";

export async function GET() {
  const data = await getPrivacy();
  return NextResponse.json(data);
}
