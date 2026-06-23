import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";
import { TENANTS } from "@/lib/tenants";

export async function GET() {
  try {
    const engine = getEngine();
    const partitions = engine.listPartitions();
    const active = engine.activePartition();
    return NextResponse.json({ partitions, active });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}

export async function POST(req: NextRequest) {
  try {
    const { action, id, partitionA, partitionB, subject, permission, resource } = await req.json();
    const engine = getEngine();

    if (action === "create") {
      if (!id) return NextResponse.json({ error: "Missing partition id" }, { status: 400 });
      engine.createPartition(id);
      return NextResponse.json({ success: true });
    }

    if (action === "delete") {
      if (!id) return NextResponse.json({ error: "Missing partition id" }, { status: 400 });
      if (id === "default") return NextResponse.json({ error: "Cannot delete default partition" }, { status: 400 });
      engine.deletePartition(id);
      return NextResponse.json({ success: true });
    }

    if (action === "switch") {
      if (!id) return NextResponse.json({ error: "Missing partition id" }, { status: 400 });
      engine.switchPartition(id);
      return NextResponse.json({ success: true, active: engine.activePartition() });
    }

    if (action === "seed") {
      if (!id) return NextResponse.json({ error: "Missing partition id" }, { status: 400 });
      const tenant = TENANTS.find(t => t.id === id);
      if (!tenant) return NextResponse.json({ error: "Tenant seed not found" }, { status: 404 });

      // Create partition if it doesn't exist, switch to it, seed it, switch back
      engine.createPartition(id);
      const originalActive = engine.activePartition();
      engine.switchPartition(id);

      // Write batch of tuples
      const tuplesToWrite = tenant.tuples.map(t => ({
        subject: t.subject,
        relation: t.relation,
        object: t.object,
      }));
      engine.writeBatch(tuplesToWrite);

      engine.switchPartition(originalActive);
      return NextResponse.json({ success: true });
    }

    if (action === "isolation-test") {
      if (!partitionA || !partitionB || !subject || !permission || !resource) {
        return NextResponse.json({ error: "Missing isolation test parameters" }, { status: 400 });
      }

      const originalActive = engine.activePartition();

      // Check A
      engine.switchPartition(partitionA);
      const resA = engine.check(subject, permission, resource);

      // Check B
      engine.switchPartition(partitionB);
      const resB = engine.check(subject, permission, resource);

      // Switch back
      engine.switchPartition(originalActive);

      return NextResponse.json({
        allowedA: resA.allowed ?? false,
        allowedB: resB.allowed ?? false,
        revisionA: resA.revision ?? 0,
        revisionB: resB.revision ?? 0,
      });
    }

    return NextResponse.json({ error: `Unknown action: ${action}` }, { status: 400 });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}
