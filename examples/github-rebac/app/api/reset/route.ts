import { NextResponse } from "next/server";
import { resetEngine, getEngine } from "@/lib/engine";
import { SEED_MINIMAL } from "@/lib/seed";

export async function POST() {
  try {
    const engine = resetEngine();
    let revision = engine.initializeResult().revision;
    for (const t of SEED_MINIMAL) {
      const result = engine.write(t.subject, t.relation, t.object);
      revision = result.revision;
    }
    return NextResponse.json({ success: true, revision, tuplesWritten: SEED_MINIMAL.length });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error", success: false }, { status: 500 });
  }
}
