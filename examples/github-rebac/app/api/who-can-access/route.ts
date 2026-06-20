import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";

export async function POST(req: NextRequest) {
  try {
    const { permission, resource, pageOffset, pageLimit, includePaths } = await req.json();
    const engine = getEngine();
    const raw = engine.whoCanAccess(
      permission,
      resource,
      pageOffset != null ? Number(pageOffset) : undefined,
      pageLimit != null ? Number(pageLimit) : undefined,
      includePaths ?? true,
    );
    const parsed = typeof raw === "string" ? JSON.parse(raw) : raw;
    const subjects = parsed.subjects ?? [];
    return NextResponse.json({
      subjects,
      subjectNames: subjects.map((s: any) => s.subject),
      totalCount: parsed.total ?? parsed.totalCount ?? subjects.length,
      nextOffset: parsed.nextOffset ?? parsed.nextOffset,
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error", subjects: [], subjectNames: [], totalCount: 0 }, { status: 500 });
  }
}
