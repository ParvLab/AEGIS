import { NextRequest, NextResponse } from "next/server";
import { getEngine, readCurrentSchema, writeCurrentSchema } from "@/lib/engine";

export async function GET() {
  return NextResponse.json({ schema: readCurrentSchema() });
}

export async function POST(req: NextRequest) {
  try {
    const { action, schema } = await req.json();
    const engine = getEngine();
    const activeSchema = schema || readCurrentSchema();

    if (action === "validate") {
      const report = engine.checkSchema(activeSchema);
      return NextResponse.json({ report });
    }

    if (action === "apply") {
      engine.reloadSchema(activeSchema);
      writeCurrentSchema(activeSchema);
      const health = engine.health();
      return NextResponse.json({ applied: true, revision: health.revision });
    }

    return NextResponse.json({ error: `Unknown action: ${action}` }, { status: 400 });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}
