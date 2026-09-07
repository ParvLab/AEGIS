import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";

interface SubRecord {
  sub: any;
  createdAt: number;
}

// Global in-memory map to hold subscriptions with creation timestamps
const subscriptions: Map<string, SubRecord> = new Map();
let subCounter = 0;

// Cleanup expired subscriptions (older than 10 minutes)
function cleanExpired() {
  const now = Date.now();
  const TEN_MINUTES = 10 * 60 * 1000;
  for (const [id, record] of subscriptions.entries()) {
    if (now - record.createdAt > TEN_MINUTES) {
      try {
        record.sub.unsubscribe();
      } catch {}
      subscriptions.delete(id);
    }
  }
}

export async function GET() {
  cleanExpired();
  const list = Array.from(subscriptions.entries()).map(([id, record]) => ({
    subscriptionId: id,
    ageSeconds: Math.floor((Date.now() - record.createdAt) / 1000),
  }));
  return NextResponse.json({ subscriptions: list });
}

export async function POST(req: NextRequest) {
  try {
    cleanExpired();
    const { action, subjectType, relation, objectType, eventTypes, subscriptionId } = await req.json();
    const engine = getEngine();

    if (action === "watch") {
      const sub = engine.watch(subjectType || undefined, relation || undefined, objectType || undefined);
      const id = `sub_${++subCounter}`;
      subscriptions.set(id, { sub, createdAt: Date.now() });
      return NextResponse.json({ subscriptionId: id });
    }

    if (action === "subscribe") {
      const sub = engine.subscribe(eventTypes || []);
      const id = `sub_${++subCounter}`;
      subscriptions.set(id, { sub, createdAt: Date.now() });
      return NextResponse.json({ subscriptionId: id });
    }

    if (action === "poll") {
      const record = subscriptions.get(subscriptionId);
      if (!record) {
        return NextResponse.json(
          { error: "Subscription expired or server was restarted", code: "SubscriptionNotFound" },
          { status: 404 }
        );
      }
      const event = record.sub.poll();
      return NextResponse.json({ event: event ?? null });
    }

    if (action === "unsubscribe") {
      const record = subscriptions.get(subscriptionId);
      if (record) {
        try {
          record.sub.unsubscribe();
        } catch {}
        subscriptions.delete(subscriptionId);
      }
      return NextResponse.json({ success: true });
    }

    return NextResponse.json({ error: `Unknown action: ${action}` }, { status: 400 });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}

