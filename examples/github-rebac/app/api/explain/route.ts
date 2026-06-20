import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";

export async function POST(req: NextRequest) {
  try {
    const { subject, permission, resource } = await req.json();
    const engine = getEngine();
    const result = engine.explain(subject, permission, resource);
    return NextResponse.json({
      allowed: result.allowed ?? false,
      resolvedVia: result.resolvedVia ?? "unknown",
      durationMs: result.durationMs ?? result.duration_ms ?? 0,
      trace: result.trace ?? [],
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error", allowed: false }, { status: 500 });
  }
}
