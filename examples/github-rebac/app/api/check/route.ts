import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";

export async function POST(req: NextRequest) {
  try {
    const { subject, permission, resource, dryRun, consistency } = await req.json();
    const engine = getEngine();
    const result = dryRun 
      ? engine.checkDryRun(subject, permission, resource, consistency) 
      : engine.check(subject, permission, resource, consistency);
    return NextResponse.json({
      allowed: result.allowed ?? false,
      revision: result.revision ?? 0,
      durationMs: result.durationMs ?? result.duration_ms ?? 0,
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error", allowed: false }, { status: 500 });
  }
}

