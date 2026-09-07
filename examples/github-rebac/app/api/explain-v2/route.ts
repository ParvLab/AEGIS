import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";

export async function POST(req: NextRequest) {
  try {
    const { subject, permission, resource, consistency } = await req.json();
    const engine = getEngine();
    const consistencyParam = consistency === "default" || !consistency ? undefined : consistency;
    const raw = engine.explainV2(subject, permission, resource, consistencyParam);
    const result = typeof raw === "string" ? JSON.parse(raw) : raw;
    return NextResponse.json({
      allowed: result.allowed ?? false,
      revision: result.revision ?? 0,
      trace: result.trace ?? [],
      resolvedVia: result.resolvedVia ?? "",
      durationMs: result.durationMs ?? 0,
      cacheHit: result.cacheHit ?? false,
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}

