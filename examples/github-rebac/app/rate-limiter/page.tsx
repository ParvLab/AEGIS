"use client";

import { useState } from "react";
import { useDiscovery } from "@/lib/useDiscovery";

export default function RateLimiterPage() {
  const { subjects, permissions, objects } = useDiscovery();

  // Config form state
  const [tokensPerSec, setTokensPerSec] = useState("10");
  const [burstCapacity, setBurstCapacity] = useState("20");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [success, setSuccess] = useState("");

  // Stress Test State
  const [requestCount, setRequestCount] = useState("50");
  const [testSubject, setTestSubject] = useState("");
  const [testPermission, setTestPermission] = useState("");
  const [testResource, setTestResource] = useState("");
  const [stressLoading, setStressLoading] = useState(false);
  const [stressResult, setStressResult] = useState<{
    total: number;
    ok: number;
    rateLimited: number;
    otherErrors: number;
    timeline: Array<{ id: number; ok: boolean; error?: string }>;
  } | null>(null);

  async function handleApplyConfig() {
    setLoading(true);
    setError("");
    setSuccess("");
    try {
      // Build a rate-limiter config JSON matching crates/aegis-core/src/limiter/mod.rs logic
      const configObj = {
        enabled: true,
        tokens_per_second: Number(tokensPerSec),
        burst_capacity: Number(burstCapacity),
      };

      const res = await fetch("/api/rate-limiter", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action: "set", configJson: JSON.stringify(configObj) }),
      });
      const data = await res.json();
      if (data.error) {
        setError(data.error);
      } else {
        setSuccess(`Rate Limiter applied: ${tokensPerSec} tokens/sec, burst capacity = ${burstCapacity}`);
      }
    } catch {
      setError("Failed to apply rate limiter config");
    } finally {
      setLoading(false);
    }
  }

  async function handleRunStressTest() {
    setStressLoading(true);
    setError("");
    setStressResult(null);
    try {
      const res = await fetch("/api/rate-limiter", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          action: "stress",
          requestCount: Number(requestCount),
          subject: testSubject || subjects[0],
          permission: testPermission || permissions[0],
          resource: testResource || objects[0],
        }),
      });
      const data = await res.json();
      if (data.error) {
        setError(data.error);
      } else {
        setStressResult(data);
      }
    } catch {
      setError("Failed to execute stress test");
    } finally {
      setStressLoading(false);
    }
  }

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold text-aegis-text">Token Bucket Rate Limiter</h2>
        <p className="text-aegis-muted text-sm mt-1">
          Apply a real-time token-bucket rate limiter to GraphEngine check requests and run concurrent query stress tests.
        </p>
      </div>

      {error && (
        <div className="p-4 bg-aegis-red/10 border border-aegis-red/30 rounded-lg text-sm text-aegis-red">
          {error}
        </div>
      )}

      {success && (
        <div className="p-4 bg-aegis-green/10 border border-aegis-green/30 rounded-lg text-sm text-aegis-green">
          {success}
        </div>
      )}

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* Rate Limiter Config Card */}
        <div className="bg-aegis-card border border-aegis-border rounded-xl p-6 space-y-4 h-fit">
          <h4 className="text-lg font-bold text-aegis-text">Rate Limiter Configuration</h4>
          <p className="text-xs text-aegis-muted leading-relaxed">
            Limit permissions requests using a token bucket. Burst limits allow handles of rapid events before restricting to steady-state limits.
          </p>

          <div className="space-y-3">
            <div>
              <label className="block text-[10px] text-aegis-muted mb-1 uppercase tracking-wider">Steady Tokens Per Second</label>
              <input
                type="number"
                value={tokensPerSec}
                onChange={(e) => setTokensPerSec(e.target.value)}
                className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
              />
            </div>
            <div>
              <label className="block text-[10px] text-aegis-muted mb-1 uppercase tracking-wider">Burst Capacity</label>
              <input
                type="number"
                value={burstCapacity}
                onChange={(e) => setBurstCapacity(e.target.value)}
                className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
              />
            </div>
          </div>

          <button
            onClick={handleApplyConfig}
            disabled={loading}
            className="w-full py-2 bg-aegis-accent text-white rounded-lg hover:opacity-90 font-semibold text-sm transition-opacity"
          >
            {loading ? "Applying..." : "Apply Rate Limits"}
          </button>
        </div>

        {/* Stress Test Card */}
        <div className="lg:col-span-2 bg-aegis-card border border-aegis-border rounded-xl p-6 space-y-4">
          <h4 className="text-lg font-bold text-aegis-text">Rate Limiter Stress Testing</h4>
          <p className="text-xs text-aegis-muted">
            Send a rapid block of checks sequentially in a single route context to verify token exhaustion and rate limiting.
          </p>

          <div className="grid grid-cols-1 md:grid-cols-4 gap-4 items-end">
            <div>
              <label className="block text-[10px] text-aegis-muted mb-1 uppercase tracking-wider">Request Count</label>
              <input
                type="number"
                value={requestCount}
                onChange={(e) => setRequestCount(e.target.value)}
                className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
              />
            </div>
            <div>
              <label className="block text-[10px] text-aegis-muted mb-1 uppercase tracking-wider">Subject</label>
              <select
                value={testSubject}
                onChange={(e) => setTestSubject(e.target.value)}
                className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
              >
                {subjects.map(s => <option key={s} value={s}>{s}</option>)}
              </select>
            </div>
            <div>
              <label className="block text-[10px] text-aegis-muted mb-1 uppercase tracking-wider">Permission</label>
              <select
                value={testPermission}
                onChange={(e) => setTestPermission(e.target.value)}
                className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
              >
                {permissions.map(p => <option key={p} value={p}>{p}</option>)}
              </select>
            </div>
            <div>
              <label className="block text-[10px] text-aegis-muted mb-1 uppercase tracking-wider">Resource</label>
              <select
                value={testResource}
                onChange={(e) => setTestResource(e.target.value)}
                className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
              >
                {objects.map(r => <option key={r} value={r}>{r}</option>)}
              </select>
            </div>
          </div>

          <button
            onClick={handleRunStressTest}
            disabled={stressLoading}
            className="px-6 py-2.5 bg-aegis-accent text-white rounded-lg hover:opacity-90 font-semibold text-sm transition-opacity"
          >
            {stressLoading ? "Running Stress Test..." : "Run Stress Test"}
          </button>

          {stressResult && (
            <div className="space-y-4 pt-4 border-t border-aegis-border animate-fade-in">
              <div className="grid grid-cols-3 gap-4 text-center">
                <div className="p-3 bg-aegis-green/5 border border-aegis-green/20 rounded-lg">
                  <span className="text-xl font-bold text-aegis-green">{stressResult.ok}</span>
                  <p className="text-[10px] text-aegis-muted uppercase mt-0.5">Successful (200 OK)</p>
                </div>
                <div className="p-3 bg-aegis-red/5 border border-aegis-red/20 rounded-lg">
                  <span className="text-xl font-bold text-aegis-red">{stressResult.rateLimited}</span>
                  <p className="text-[10px] text-aegis-muted uppercase mt-0.5">Rate Limited</p>
                </div>
                <div className="p-3 bg-aegis-border/10 border border-aegis-border rounded-lg">
                  <span className="text-xl font-bold text-aegis-text">{stressResult.otherErrors}</span>
                  <p className="text-[10px] text-aegis-muted uppercase mt-0.5">Other Errors</p>
                </div>
              </div>

              {/* Stress Timeline Bar Chart */}
              <div>
                <span className="text-[10px] text-aegis-muted uppercase tracking-wider block font-semibold mb-2">
                  Request Timeline Distribution
                </span>
                <div className="flex flex-wrap gap-1 p-3 bg-aegis-border/10 rounded-lg max-h-[100px] overflow-y-auto">
                  {stressResult.timeline.map((item) => (
                    <div
                      key={item.id}
                      title={`Req #${item.id}: ${item.ok ? "Success" : item.error}`}
                      className={`w-3.5 h-3.5 rounded transition-transform hover:scale-125 cursor-pointer ${
                        item.ok ? "bg-aegis-green" : item.error === "RateLimitExceeded" ? "bg-aegis-red" : "bg-aegis-muted"
                      }`}
                    />
                  ))}
                </div>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
