import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";

export async function GET() {
  try {
    const engine = getEngine();
    const schedules = engine.listAnalysisSchedules();
    const runs = engine.getAnalysisRuns(20);
    return NextResponse.json({
      schedules: Array.isArray(schedules) ? schedules : [],
      runs: Array.isArray(runs) ? runs : [],
    });
  } catch (err: any) {
    return NextResponse.json({ schedules: [], runs: [], error: err.message ?? "Unknown error" }, { status: 500 });
  }
}

export async function POST(req: NextRequest) {
  try {
    const { action, name, intervalSeconds, queriesJson, compareSchemaJson, scheduleId } = await req.json();
    const engine = getEngine();
    let result: any;

    switch (action) {
      case "create":
        result = engine.createAnalysisSchedule(name, intervalSeconds, queriesJson, compareSchemaJson);
        break;
      case "delete":
        result = engine.deleteAnalysisSchedule(scheduleId);
        break;
      case "run":
        result = engine.runAnalysisNow(scheduleId || undefined);
        break;
      default:
        return NextResponse.json({ error: `Unknown action: ${action}` }, { status: 400 });
    }

    return NextResponse.json({ action, result });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}
