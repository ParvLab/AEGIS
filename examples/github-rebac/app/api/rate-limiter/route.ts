import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";

export async function GET() {
  try {
    const engine = getEngine();
    // Use getEnforcementHistoryConfig or general config wrapper if needed.
    // In our NAPI, we have setRateLimiter(configJson) which applies a token-bucket rate limiter.
    return NextResponse.json({
      active: true,
      limits: {
        tokensPerSecond: 1000,
        burstCapacity: 5000,
      }
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}

export async function POST(req: NextRequest) {
  try {
    const { action, configJson, requestCount, subject, permission, resource } = await req.json();
    const engine = getEngine();

    if (action === "set") {
      if (!configJson) return NextResponse.json({ error: "Missing config JSON" }, { status: 400 });
      try {
        const cfg = JSON.parse(configJson);
        engine.setRateLimiter(
          cfg.checksPerSecond ?? cfg.checks_per_second ?? undefined,
          cfg.checkBurst ?? cfg.check_burst ?? undefined,
          cfg.writesPerSecond ?? cfg.writes_per_second ?? undefined,
          cfg.writeBurst ?? cfg.write_burst ?? undefined,
          cfg.maxTraversalDepth ?? cfg.max_traversal_depth ?? undefined,
          cfg.maxTraversalVisits ?? cfg.max_traversal_visits ?? undefined,
          cfg.maxKeys ?? cfg.max_keys ?? undefined
        );
      } catch (e: any) {
        console.error("Failed to set rate limiter:", e);
        return NextResponse.json({ error: `Invalid configuration: ${e.message}` }, { status: 400 });
      }
      return NextResponse.json({ success: true });
    }

    if (action === "stress") {
      const count = requestCount ? Number(requestCount) : 50;
      const sub = subject || "user:alice";
      const perm = permission || "push";
      const res = resource || "repo:api-gateway";

      let okCount = 0;
      let limitCount = 0;
      let errorCount = 0;
      const timeline: Array<{ id: number; ok: boolean; error?: string }> = [];

      for (let i = 0; i < count; i++) {
        try {
          // Perform permission checks in rapid succession
          const result = engine.check(sub, perm, res);
          okCount++;
          timeline.push({ id: i + 1, ok: true });
        } catch (err: any) {
          const msg = err.message ?? "";
          if (msg.includes("RateLimitExceeded") || msg.toLowerCase().includes("rate limit")) {
            limitCount++;
            timeline.push({ id: i + 1, ok: false, error: "RateLimitExceeded" });
          } else {
            errorCount++;
            timeline.push({ id: i + 1, ok: false, error: msg });
          }
        }
      }

      return NextResponse.json({
        total: count,
        ok: okCount,
        rateLimited: limitCount,
        otherErrors: errorCount,
        timeline,
      });
    }

    return NextResponse.json({ error: `Unknown action: ${action}` }, { status: 400 });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}
