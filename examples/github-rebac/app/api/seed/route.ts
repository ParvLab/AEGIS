import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";
import { SEED_MINIMAL, SEED_FULL } from "@/lib/seed";

export async function POST(req: NextRequest) {
  try {
    const body = await req.json();
    const level: string = body.level ?? "minimal";
    const tuples = level === "full" ? SEED_FULL : SEED_MINIMAL;

    const engine = getEngine();
    let revision = engine.initializeResult().revision;
    for (const t of tuples) {
      const result = engine.write(t.subject, t.relation, t.object);
      revision = result.revision;
    }
    return NextResponse.json({ tuplesWritten: tuples.length, revision });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}
