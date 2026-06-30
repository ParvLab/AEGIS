import { NextRequest, NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";
import { handleEngineError } from "@/lib/errors";
import { initialize } from "@aegis-v/engine";

export async function POST(req: NextRequest) {
  try {
    const { trigger } = await req.json();
    const engine = getEngine();

    if (trigger === "invalid-subject") {
      // Invalid subject contains spaces or invalid characters
      engine.write("user:invalid space subject", "member", "repo:api");
      return NextResponse.json({ success: true });
    }

    if (trigger === "schema-error") {
      engine.reloadSchema("types:\n  user:\n    relations:\n      member: invalid_yaml_flow {");
      return NextResponse.json({ success: true });
    }

    if (trigger === "rate-limit") {
      // Set rate limits low: 1 check per second, 1 burst
      engine.setRateLimiter(1, 1, 1, 1, 10, 100, 1000);
      // Fire 5 checks rapidly to guarantee RateLimitExceeded
      for (let i = 0; i < 5; i++) {
        engine.check("user:alice", "push", "repo:api");
      }
      return NextResponse.json({ success: true });
    }

    if (trigger === "future-revision") {
      engine.check("user:alice", "push", "repo:api", "at_revision:999999");
      return NextResponse.json({ success: true });
    }

    if (trigger === "engine-closed") {
      // Initialize a temporary in-memory engine, close it, and call check
      const temp = initialize(":memory:", "types:\n  user: {}");
      temp.close();
      temp.check("user:alice", "push", "repo:api");
      return NextResponse.json({ success: true });
    }

    return NextResponse.json({ error: `Unknown trigger: ${trigger}` }, { status: 400 });
  } catch (err: any) {
    const formatted = handleEngineError(err);
    return NextResponse.json({ ...formatted, expected: true });
  }
}
