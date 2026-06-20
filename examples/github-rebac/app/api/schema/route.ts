import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";
import { SCHEMA } from "@/lib/schema";

let currentSchema = SCHEMA;

export async function GET() {
  return NextResponse.json({ schema: currentSchema });
}

export async function POST(req: NextRequest) {
  try {
    const { action, schema } = await req.json();
    const engine = getEngine();

    if (action === "validate") {
      const report = engine.checkSchema(schema || currentSchema);
      return NextResponse.json({ report });
    }

    if (action === "apply") {
      engine.reloadSchema(schema || currentSchema);
      currentSchema = schema || currentSchema;
      const health = engine.health();
      return NextResponse.json({ applied: true, revision: health.revision });
    }

    return NextResponse.json({ error: `Unknown action: ${action}` }, { status: 400 });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}
