import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";

export async function POST(req: NextRequest) {
  try {
    const { subject, permission, resource, context, dryRun } = await req.json();
    const engine = getEngine();
    const ctx = context ?? {};

    let result: any;
    if (dryRun) {
      result = engine.checkDryRunWithContext(subject, permission, resource, ctx);
    } else {
      result = engine.checkWithContext(subject, permission, resource, ctx);
    }

    return NextResponse.json({
      allowed: result.allowed ?? false,
      revision: result.revision ?? 0,
      durationMs: result.durationMs ?? result.duration_ms ?? 0,
      dryRun: !!dryRun,
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}
