import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";

export async function GET() {
  try {
    const engine = getEngine();
    const config = engine.getEnforcementHistoryConfig();
    const trends = engine.enforcementTrends(20);
    return NextResponse.json({
      config,
      trends: typeof trends === "string" ? JSON.parse(trends) : trends,
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}

export async function POST(req: NextRequest) {
  try {
    const { action, configJson } = await req.json();
    const engine = getEngine();

    if (action === "config") {
      engine.setEnforcementHistoryConfig(configJson);
      return NextResponse.json({ success: true });
    }

    return NextResponse.json({ error: `Unknown action: ${action}` }, { status: 400 });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}
