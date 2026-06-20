import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";
import { SCHEMA } from "@/lib/schema";

export async function POST(req: NextRequest) {
  try {
    const { mode, subject, permission, resource, schemaBefore, schemaAfter } = await req.json();
    const engine = getEngine();

    if (mode === "dry-run") {
      const result = engine.checkDryRun(subject, permission, resource);
      return NextResponse.json({
        dryRunResult: {
          allowed: result.allowed ?? false,
          revision: result.revision ?? 0,
          durationMs: result.durationMs ?? result.duration_ms ?? 0,
        },
      });
    }

    if (mode === "access-diff") {
      const before = schemaBefore || SCHEMA;
      const after = schemaAfter || SCHEMA;
      const result = engine.accessDiff(before, after);
      const parsed = typeof result === "string" ? JSON.parse(result) : result;
      return NextResponse.json(parsed);
    }

    if (mode === "dry-run-write") {
      const { subject, relation, resource } = await req.json();
      const result = engine.writeDryRun(subject, relation, resource);
      return NextResponse.json({
        dryRunResult: {
          allowed: result.allowed ?? false,
          revision: result.revision ?? 0,
          durationMs: result.durationMs ?? result.duration_ms ?? 0,
        },
      });
    }

    return NextResponse.json({ error: `Unknown mode: ${mode}` }, { status: 400 });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}
