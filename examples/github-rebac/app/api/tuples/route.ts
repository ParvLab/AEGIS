import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";

export async function POST(req: NextRequest) {
  try {
    const body = await req.json();
    const { action } = body;
    const engine = getEngine();

    let result: any;
    switch (action) {
      case "write": {
        const { subject, relation, resource, condition, metadata, validUntil } = body;
        result = engine.write(subject, relation, resource, condition || undefined, metadata || undefined, validUntil || undefined);
        break;
      }
      case "delete": {
        const { subject, relation, resource } = body;
        result = engine.delete(subject, relation, resource);
        break;
      }
      case "ban": {
        const { subject, resource } = body;
        result = engine.write(subject, "banned", resource);
        break;
      }
      case "unban": {
        const { subject, resource } = body;
        result = engine.delete(subject, "banned", resource);
        break;
      }
      case "batch-write": {
        const { tuples } = body;
        result = engine.writeBatch(tuples || []);
        break;
      }
      case "dry-run-write": {
        const { subject, relation, resource, condition, metadata, validUntil } = body;
        result = engine.writeDryRun(subject, relation, resource, condition || undefined, metadata || undefined, validUntil || undefined);
        break;
      }
      case "list-by-object": {
        const { object, relation } = body;
        const tuples = engine.listByObject(object, relation || undefined);
        return NextResponse.json({ tuples: Array.isArray(tuples) ? tuples : [] });
      }
      case "list-by-subject": {
        const { subject, relation } = body;
        const tuples = engine.listBySubject(subject, relation || undefined);
        return NextResponse.json({ tuples: Array.isArray(tuples) ? tuples : [] });
      }
      case "delete-object": {
        const { object } = body;
        result = engine.deleteObject(object);
        break;
      }
      case "delete-subject-with-policy": {
        const { subject, policy, transferToSubject } = body;
        result = engine.deleteSubjectWithPolicy(subject, policy, transferToSubject || undefined);
        break;
      }
      default:
        return NextResponse.json({ error: `Unknown action: ${action}` }, { status: 400 });
    }

    return NextResponse.json({ revision: result?.revision ?? 0, action, result });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}
