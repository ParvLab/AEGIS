import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";

export async function POST(req: NextRequest) {
  try {
    const { action, subject, resource } = await req.json();
    const engine = getEngine();

    if (action === "verify-audit-chain") {
      const integrity = engine.verifyAuditChain();
      return NextResponse.json({ integrity });
    }

    if (action === "analysis-report") {
      const report = engine.analysisReport();
      return NextResponse.json({ report });
    }

    if (action === "access-review-subject") {
      if (!subject) return NextResponse.json({ error: "Missing subject" }, { status: 400 });
      const raw = engine.accessReviewForSubject(subject);
      const parsed = typeof raw === "string" ? JSON.parse(raw) : raw;
      return NextResponse.json({ review: parsed });
    }

    if (action === "access-review-resource") {
      if (!resource) return NextResponse.json({ error: "Missing resource" }, { status: 400 });
      const raw = engine.accessReviewForResource(resource);
      const parsed = typeof raw === "string" ? JSON.parse(raw) : raw;
      return NextResponse.json({ review: parsed });
    }

    return NextResponse.json({ error: `Unknown action: ${action}` }, { status: 400 });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}
