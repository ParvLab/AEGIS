import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";

export async function GET() {
  try {
    const engine = getEngine();
    const configRaw = engine.getEnforcementHistoryConfig();
    const config = typeof configRaw === "string" ? JSON.parse(configRaw) : configRaw;
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
      if (!configJson) return NextResponse.json({ error: "Missing config JSON" }, { status: 400 });
      const parsed = typeof configJson === "string" ? JSON.parse(configJson) : configJson;
      const normalized = {
        enabled: parsed.enabled ?? false,
        sampling: parsed.sampling === "all" || parsed.sampling === "All" ? "All" : "DeniedOnly",
        max_events_per_minute: Number(parsed.maxEventsPerMinute ?? parsed.max_events_per_minute ?? 10000),
        max_rows: Number(parsed.maxRows ?? parsed.max_rows ?? 100000),
        max_days: Number(parsed.maxDays ?? parsed.max_days ?? 7),
      };
      engine.setEnforcementHistoryConfig(JSON.stringify(normalized));
      return NextResponse.json({ success: true });
    }

    return NextResponse.json({ error: `Unknown action: ${action}` }, { status: 400 });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}
