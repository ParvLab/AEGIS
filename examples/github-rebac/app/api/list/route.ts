import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";

export async function POST(req: NextRequest) {
  try {
    const { mode, target, relation } = await req.json();
    const engine = getEngine();

    let tuples: any[] = [];
    switch (mode) {
      case "object": tuples = engine.listByObject(target, relation || undefined); break;
      case "subject": tuples = engine.listBySubject(target, relation || undefined); break;
      case "relation": tuples = engine.listByRelation(target, relation); break;
      default: return NextResponse.json({ error: `Unknown mode: ${mode}` }, { status: 400 });
    }

    return NextResponse.json({ tuples });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error", tuples: [] }, { status: 500 });
  }
}
