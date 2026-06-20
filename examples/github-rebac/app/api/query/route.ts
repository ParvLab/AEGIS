import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";

export async function POST(req: NextRequest) {
  try {
    const { subjectType, relation, objectType, metadataKey, metadataValue, limit, cursorOffset } = await req.json();
    const engine = getEngine();

    const filter: any = {};
    if (subjectType) filter.subjectType = subjectType;
    if (relation) filter.relation = relation;
    if (objectType) filter.objectType = objectType;
    if (metadataKey) filter.metadataKey = metadataKey;
    if (metadataValue) filter.metadataValue = metadataValue;

    const pagination: any = { limit: limit ?? 50 };
    if (cursorOffset != null) pagination.cursorOffset = cursorOffset;

    const result = engine.query(filter, pagination);
    const parsed = typeof result === "string" ? JSON.parse(result) : result;

    return NextResponse.json({
      tuples: parsed.tuples ?? [],
      nextCursor: parsed.nextCursor ?? null,
      revision: parsed.revision ?? 0,
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error", tuples: [], nextCursor: null, revision: 0 }, { status: 500 });
  }
}
