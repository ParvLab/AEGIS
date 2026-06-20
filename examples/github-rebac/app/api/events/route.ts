import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";

let subscriptions: Map<string, any> = new Map();
let subCounter = 0;

export async function POST(req: NextRequest) {
  try {
    const { action, subjectType, relation, objectType, eventTypes, subscriptionId } = await req.json();
    const engine = getEngine();

    if (action === "watch") {
      const sub = engine.watch(subjectType || undefined, relation || undefined, objectType || undefined);
      const id = `sub_${++subCounter}`;
      subscriptions.set(id, sub);
      return NextResponse.json({ subscriptionId: id });
    }

    if (action === "subscribe") {
      const sub = engine.subscribe(eventTypes || []);
      const id = `sub_${++subCounter}`;
      subscriptions.set(id, sub);
      return NextResponse.json({ subscriptionId: id });
    }

    if (action === "poll") {
      const sub = subscriptions.get(subscriptionId);
      if (!sub) return NextResponse.json({ error: "Subscription not found" }, { status: 404 });
      const event = sub.poll();
      return NextResponse.json({ event: event ?? null });
    }

    if (action === "unsubscribe") {
      const sub = subscriptions.get(subscriptionId);
      if (sub) {
        sub.unsubscribe();
        subscriptions.delete(subscriptionId);
      }
      return NextResponse.json({ success: true });
    }

    return NextResponse.json({ error: `Unknown action: ${action}` }, { status: 400 });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}
