import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";

interface TxRecord {
  tx: any;
  createdAt: number;
}

// Global in-memory map to hold transactions with creation timestamps
const transactions: Map<string, TxRecord> = new Map();
let txCounter = 0;

// Cleanup expired transactions (older than 10 minutes)
function cleanExpired() {
  const now = Date.now();
  const TEN_MINUTES = 10 * 60 * 1000;
  for (const [id, record] of transactions.entries()) {
    if (now - record.createdAt > TEN_MINUTES) {
      try {
        record.tx.rollback();
      } catch {}
      transactions.delete(id);
    }
  }
}

export async function GET() {
  cleanExpired();
  const list = Array.from(transactions.entries()).map(([id, record]) => ({
    transactionId: id,
    ageSeconds: Math.floor((Date.now() - record.createdAt) / 1000),
  }));
  return NextResponse.json({ transactions: list });
}

export async function POST(req: NextRequest) {
  try {
    cleanExpired();
    const { action, transactionId, subject, relation, resource, savepointName } = await req.json();
    const engine = getEngine();

    if (action === "begin") {
      const tx = engine.transaction();
      const id = `tx_${++txCounter}`;
      transactions.set(id, { tx, createdAt: Date.now() });
      return NextResponse.json({ transactionId: id });
    }

    const record = transactions.get(transactionId);
    if (!record) {
      return NextResponse.json(
        { error: "Transaction expired or server was restarted", code: "TransactionNotFound" },
        { status: 404 }
      );
    }

    const tx = record.tx;

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

