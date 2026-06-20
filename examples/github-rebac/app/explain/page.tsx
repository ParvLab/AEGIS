"use client";

import { useState } from "react";
import { ALL_USERS, ALL_RESOURCES, PERMISSIONS } from "@/lib/seed";

interface TraceStep { subject: string; relation: string; object: string; result?: string; }
interface V2TraceStep { subject: string; relation: string; object: string; result: boolean; depth: number; }
interface ExplainResult { allowed: boolean; resolvedVia: string; durationMs: number; trace: TraceStep[]; }
interface V2ExplainResult { allowed: boolean; resolvedVia: string; durationMs: number; trace: V2TraceStep[]; cacheHit: boolean; }

export default function ExplainPage() {
  const [subject, setSubject] = useState("user:mallory");
  const [permission, setPermission] = useState("pull");
  const [resource, setResource] = useState("repo:docs");
  const [useV2, setUseV2] = useState(false);
  const [result, setResult] = useState<ExplainResult | V2ExplainResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  async function handleExplain() {
    setLoading(true); setError(""); setResult(null);
    try {
      const res = await fetch(useV2 ? "/api/explain-v2" : "/api/explain", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ subject, permission, resource }),
      });
      const data = await res.json();
      if (data.error) setError(data.error); else setResult(data);
    } catch { setError("Request failed"); }
    setLoading(false);
  }

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold text-aegis-text">Explain Trace</h2>
        <p className="text-aegis-muted text-sm mt-1">See the full resolution path &mdash; V1 basic or V2 with depth-per-step</p>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-4 gap-4">
        <div>
          <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Subject</label>
          <select value={subject} onChange={(e) => setSubject(e.target.value)}
            className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent">
            {ALL_USERS.map((u) => <option key={u} value={u}>{u}</option>)}
          </select>
        </div>
        <div>
          <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Permission</label>
          <select value={permission} onChange={(e) => setPermission(e.target.value)}
            className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent">
            {PERMISSIONS.map((p) => <option key={p} value={p}>{p}</option>)}
          </select>
        </div>
        <div>
          <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Resource</label>
          <select value={resource} onChange={(e) => setResource(e.target.value)}
            className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent">
            {ALL_RESOURCES.filter((r) => r.startsWith("repo:")).map((r) => <option key={r} value={r}>{r}</option>)}
          </select>
        </div>
        <div className="flex items-end">
          <label className="flex items-center gap-2 cursor-pointer">
            <input type="checkbox" checked={useV2} onChange={(e) => setUseV2(e.target.checked)}
              className="w-4 h-4 rounded border-aegis-border bg-aegis-card accent-aegis-accent" />
            <span className="text-sm text-aegis-text">V2 (depth trace)</span>
          </label>
        </div>
      </div>

      <button onClick={handleExplain} disabled={loading}
        className="px-6 py-2.5 bg-aegis-accent text-white rounded-lg hover:opacity-90 transition-opacity disabled:opacity-50 text-sm font-medium">
        {loading ? "Tracing..." : "🧬 Explain"}
      </button>

      {error && <div className="p-4 bg-aegis-red/10 border border-aegis-red/30 rounded-lg text-sm text-aegis-red">{error}</div>}

      {result && (
        <div className="space-y-4 animate-fade-in">
          <div className={`p-4 rounded-lg border ${result.allowed ? "bg-aegis-green/10 border-aegis-green/30" : "bg-aegis-red/10 border-aegis-red/30"}`}>
            <div className="flex items-center gap-2">
              <span className={`text-lg ${result.allowed ? "text-aegis-green" : "text-aegis-red"}`}>{result.allowed ? "✅" : "❌"}</span>
              <span className={`font-bold ${result.allowed ? "text-aegis-green" : "text-aegis-red"}`}>{result.allowed ? "ALLOWED" : "DENIED"}</span>
              <span className="text-xs text-aegis-muted">resolvedVia: {result.resolvedVia} &middot; {result.durationMs.toFixed(2)}ms{(result as V2ExplainResult).cacheHit != null ? ` · cache:${(result as V2ExplainResult).cacheHit}` : ""}</span>
            </div>
          </div>

          {"trace" in result && result.trace && result.trace.length > 0 && (
            <div className="bg-aegis-card border border-aegis-border rounded-xl p-6">
              <p className="text-xs text-aegis-muted uppercase tracking-wider mb-4">Resolution Trace {useV2 ? "(V2)" : "(V1)"}</p>
              <div className="space-y-3">
                {result.trace.map((step: any, i: number) => {
                  const isDeny = step.subject === subject && step.relation === "banned";
                  return <TraceStepRow key={i} step={step} index={i} isLast={i === result.trace.length - 1} isDeny={isDeny} useV2={useV2} />;
                })}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function TraceStepRow({ step, index, isLast, isDeny, useV2 }: { step: any; index: number; isLast: boolean; isDeny: boolean; useV2: boolean }) {
  return (
    <div className="flex items-start gap-3">
      <div className="flex flex-col items-center">
        <div className={`w-2.5 h-2.5 rounded-full mt-1.5 ${isDeny ? "bg-aegis-red" : step.result === false ? "bg-aegis-red" : isLast ? "bg-aegis-green" : "bg-aegis-blue"}`} />
        {!isLast && <div className={`w-px h-6 ${isDeny ? "bg-aegis-red/50" : "bg-aegis-border"}`} />}
      </div>
      <div className={`flex-1 pb-3 ${isDeny ? "opacity-80" : ""}`}>
        <div className="flex items-center gap-2 flex-wrap">
          <span className={`text-sm font-mono ${isDeny ? "text-aegis-red" : step.result === false ? "text-aegis-red" : "text-aegis-text"}`}>
            {step.subject}
          </span>
          {index > 0 && (
            <>
              <span className="text-xs text-aegis-muted">↓</span>
              <span className="text-xs text-aegis-muted">{step.relation}</span>
            </>
          )}
          {useV2 && step.depth != null && (
            <span className="text-xs text-aegis-muted bg-aegis-bg px-1.5 py-0.5 rounded">depth={step.depth}</span>
          )}
          {useV2 && step.result != null && (
            <span className={`text-xs font-mono ${step.result ? "text-aegis-green" : "text-aegis-red"}`}>{step.result ? "✓" : "✗"}</span>
          )}
        </div>
        {isLast && <p className={`text-xs mt-1 font-medium ${isDeny ? "text-aegis-red" : "text-aegis-green"}`}>{isDeny ? "❌ DENY OVERRIDES ALL" : "✓ Granting path"}</p>}
      </div>
    </div>
  );
}
