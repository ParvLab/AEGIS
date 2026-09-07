import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";

export async function POST(req: NextRequest) {
  try {
    const { subject } = await req.json();
    if (!subject) {
      return NextResponse.json({ error: "subject is required" }, { status: 400 });
    }
    const engine = getEngine();
    const result = engine.exportSubject(subject);
    const parsed = typeof result === "string" ? JSON.parse(result) : result;
    return NextResponse.json(parsed);
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}
