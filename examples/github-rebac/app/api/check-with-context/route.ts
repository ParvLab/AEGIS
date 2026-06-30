import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";

export async function POST(req: NextRequest) {
  try {
    const { subject, permission, resource, context, dryRun, consistency } = await req.json();
    const engine = getEngine();
    const ctx = context ?? {};
    const consistencyParam = consistency === "default" || !consistency ? undefined : consistency;

    let result: any;
    if (dryRun) {
      result = engine.checkDryRunWithContext(subject, permission, resource, ctx, consistencyParam);
    } else {
      result = engine.checkWithContext(subject, permission, resource, ctx, consistencyParam);
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

