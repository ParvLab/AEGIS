import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";

export async function GET() {
  try {
    const engine = getEngine();
    const drafts = engine.listPolicyDrafts();
    const versions = engine.listPolicyVersions();
    return NextResponse.json({
      drafts: Array.isArray(drafts) ? drafts : [],
      versions: Array.isArray(versions) ? versions : [],
    });
  } catch (err: any) {
    return NextResponse.json({ drafts: [], versions: [], error: err.message ?? "Unknown error" }, { status: 500 });
  }
}

export async function POST(req: NextRequest) {
  try {
    const { action, draftId, name, description, reason, schemaJson, version } = await req.json();
    const engine = getEngine();

    let result: any;
    switch (action) {
      case "create": result = engine.createPolicyDraft(name, description); break;
      case "validate": result = engine.validatePolicyDraft(draftId); break;
      case "submit": result = engine.submitPolicyDraftForReview(draftId); break;
      case "approve": result = engine.approvePolicyDraft(draftId); break;
      case "reject": result = engine.rejectPolicyDraft(draftId, reason || "No reason provided"); break;
      case "publish": result = engine.publishPolicyDraft(draftId); break;
      case "archive": result = engine.archivePolicyDraft(draftId); break;
      case "rollback": result = engine.rollbackPolicy(Number(version)); break;
      case "update": result = engine.updatePolicyDraft(draftId, schemaJson); break;
      default: return NextResponse.json({ error: `Unknown action: ${action}` }, { status: 400 });
    }

    return NextResponse.json({ action, result });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}
