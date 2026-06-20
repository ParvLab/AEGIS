import { NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";

export async function GET() {
  try {
    const engine = getEngine();
    const h = engine.health();
    return NextResponse.json({
      healthy: h.healthy ?? false,
      revision: h.revision ?? 0,
      uptimeMs: h.uptimeMs ?? 0,
      backend: h.backend ?? "SQLite",
      cacheHitRate: h.cacheHitRate ?? 0,
      schemaVersion: h.schemaVersion ?? 0,
      totalChecks: h.totalChecks ?? 0,
      activeConnections: h.connections?.readActive ?? 0,
    });
  } catch (err: any) {
    return NextResponse.json({ healthy: false, error: err.message ?? "Unknown error" }, { status: 500 });
  }
}
