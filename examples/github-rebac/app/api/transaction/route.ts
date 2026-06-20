import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";

let transactions: Map<string, any> = new Map();
let txCounter = 0;

export async function POST(req: NextRequest) {
  try {
    const { action, transactionId, subject, relation, resource, savepointName } = await req.json();
    const engine = getEngine();

    if (action === "begin") {
      const tx = engine.transaction();
      const id = `tx_${++txCounter}`;
      transactions.set(id, tx);
      return NextResponse.json({ transactionId: id });
    }

    const tx = transactions.get(transactionId);
    if (!tx) return NextResponse.json({ error: "Transaction not found" }, { status: 404 });

    switch (action) {
      case "write":
        tx.write(subject, relation, resource);
        return NextResponse.json({ success: true });
      case "delete":
        tx.delete(subject, relation, resource);
        return NextResponse.json({ success: true });
      case "savepoint":
        tx.savepoint(savepointName);
        return NextResponse.json({ success: true });
      case "rollbackToSavepoint":
        tx.rollbackToSavepoint(savepointName);
        return NextResponse.json({ success: true });
      case "releaseSavepoint":
        tx.releaseSavepoint(savepointName);
        return NextResponse.json({ success: true });
      case "commit":
        const result = tx.commit();
        transactions.delete(transactionId);
        return NextResponse.json({ success: true, revision: result?.revision ?? 0, nodeId: result?.nodeId });
      case "rollback":
        tx.rollback();
        transactions.delete(transactionId);
        return NextResponse.json({ success: true, rolledBack: true });
      default:
        return NextResponse.json({ error: `Unknown action: ${action}` }, { status: 400 });
    }
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}
