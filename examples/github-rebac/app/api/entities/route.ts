import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";

export async function GET() {
  try {
    const engine = getEngine();
    // Query all existing tuples to extract unique subjects and objects
    const queryResult = engine.query({}, { limit: 10000 });
    const parsed = typeof queryResult === "string" ? JSON.parse(queryResult) : queryResult;
    const tuples = parsed.tuples ?? [];

    const subjectsMap = new Map<string, number>();
    const objectsMap = new Map<string, number>();

    for (const t of tuples) {
      if (t.subject) {
        subjectsMap.set(t.subject, (subjectsMap.get(t.subject) ?? 0) + 1);
      }
      if (t.object) {
        objectsMap.set(t.object, (objectsMap.get(t.object) ?? 0) + 1);
      }
    }

    const subjects = Array.from(subjectsMap.entries()).map(([name, count]) => ({ name, count }));
    const objects = Array.from(objectsMap.entries()).map(([name, count]) => ({ name, count }));

    return NextResponse.json({ subjects, objects });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}

export async function POST(req: NextRequest) {
  try {
    const { action, subject, relation, object, condition, metadata, validUntil } = await req.json();
    const engine = getEngine();

    if (action === "add") {
      if (!subject || !relation || !object) {
        return NextResponse.json({ error: "Missing tuple parameters" }, { status: 400 });
      }
      // Parse metadata if sent as key-value pairs or object
      let metaObj: Record<string, string> | undefined;
      if (metadata) {
        metaObj = typeof metadata === "string" ? JSON.parse(metadata) : metadata;
      }
      const result = engine.write(subject, relation, object, condition || undefined, metaObj, validUntil || undefined);
      return NextResponse.json({ success: true, revision: result.revision });
    }

    if (action === "remove") {
      if (!subject || !relation || !object) {
        return NextResponse.json({ error: "Missing tuple parameters" }, { status: 400 });
      }
      const result = engine.delete(subject, relation, object);
      return NextResponse.json({ success: true, revision: result.revision });
    }

    if (action === "remove-all") {
      if (!object) return NextResponse.json({ error: "Missing object" }, { status: 400 });
      const result = engine.deleteObject(object);
      return NextResponse.json({ success: true, revision: result.revision });
    }

    return NextResponse.json({ error: `Unknown action: ${action}` }, { status: 400 });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}
