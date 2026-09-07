import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";

export async function POST(req: NextRequest) {
  try {
    const { action, targetVersion } = await req.json();
    const engine = getEngine();

    if (action === "invalidate") {
      engine.invalidateCache();
      return NextResponse.json({ success: true, action: "cache invalidated" });
    }

    if (action === "migrate") {
      engine.migrate(targetVersion ?? 1);
      const health = engine.health();
      return NextResponse.json({ success: true, action: "migrated", schemaVersion: health.schemaVersion });
    }

    return NextResponse.json({ error: `Unknown action: ${action}` }, { status: 400 });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}
