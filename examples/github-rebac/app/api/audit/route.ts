import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";

export async function POST(req: NextRequest) {
  try {
    const { object, fromRevision, toRevision, limit, all } = await req.json();
    const engine = getEngine();

    let entries: any[];
    if (all) {
      entries = engine.queryAuditAll(fromRevision, toRevision, limit ?? 100);
    } else {
      entries = engine.queryAudit(object, fromRevision, toRevision, limit ?? 100);
    }

    return NextResponse.json({
      entries: Array.isArray(entries) ? entries : [],
      total: Array.isArray(entries) ? entries.length : 0,
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error", entries: [], total: 0 }, { status: 500 });
  }
}
