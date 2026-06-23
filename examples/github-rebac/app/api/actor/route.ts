import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";

export async function GET() {
  try {
    const engine = getEngine();
    const actor = engine.activeActor();
    return NextResponse.json({ actor: actor ?? null });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}

export async function POST(req: NextRequest) {
  try {
    const { actor } = await req.json();
    const engine = getEngine();
    engine.setActor(actor || undefined);
    return NextResponse.json({ success: true, actor: engine.activeActor() ?? null });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}
