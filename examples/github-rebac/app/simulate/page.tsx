"use client";

import { useState, useEffect } from "react";
import { ALL_USERS, ALL_RESOURCES, PERMISSIONS, RELATIONS } from "@/lib/seed";

export default function SimulatePage() {
  const [mode, setMode] = useState<"dry-run" | "dry-run-write" | "access-diff">("dry-run");
  const [subject, setSubject] = useState("user:bob");
  const [relation, setRelation] = useState("admin");
  const [permission, setPermission] = useState("push");
  const [resource, setResource] = useState("repo:vault");
  const [schemaBefore, setSchemaBefore] = useState("");
  const [schemaAfter, setSchemaAfter] = useState("");
  const [result, setResult] = useState<Record<string, unknown> | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    if (mode === "access-diff" && !schemaBefore) {
      fetch("/api/schema").then((r) => r.json()).then((d) => { setSchemaBefore(d.schema); setSchemaAfter(d.schema); });
    }
  }, [mode, schemaBefore]);

  async function handleSimulate() {
    setLoading(true); setError(""); setResult(null);
    try {
      const body: Record<string, unknown> = { mode, subject, permission, resource };
      if (mode === "access-diff") { body.schemaBefore = schemaBefore; body.schemaAfter = schemaAfter; }
      if (mode === "dry-run-write") { body.relation = relation; }
      const res = await fetch("/api/simulate", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      const data = await res.json();
      if (data.error) setError(data.error); else setResult(data);
    } catch { setError("Request failed"); }
    setLoading(false);
  }

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold text-aegis-text">Simulation</h2>
        <p className="text-aegis-muted text-sm mt-1">What-if analysis &mdash; preview changes without committing them</p>
      </div>

      <div className="flex flex-wrap gap-2">
        {(["dry-run", "dry-run-write", "access-diff"] as const).map((m) => (
          <button key={m} onClick={() => setMode(m)}
            className={`px-4 py-2 rounded-lg text-sm font-medium transition-colors ${
              mode === m ? "bg-aegis-accent/20 text-aegis-accent border border-aegis-accent/40"
                : "bg-aegis-card border border-aegis-border text-aegis-muted hover:text-aegis-text"
            }`}>
            {m === "dry-run" ? "🧪 Dry-Run Check" : m === "dry-run-write" ? "📝 Dry-Run Write" : "📊 Access Diff"}
          </button>
        ))}
      </div>

      {mode !== "access-diff" && (
        <div className="grid grid-cols-1 md:grid-cols-4 gap-4">
          <div>
            <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Subject</label>
            <select value={subject} onChange={(e) => setSubject(e.target.value)}
              className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent">
              {ALL_USERS.map((u) => <option key={u} value={u}>{u}</option>)}
            </select>
          </div>
          {mode === "dry-run-write" && (
            <div>
              <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">New Relation</label>
              <select value={relation} onChange={(e) => setRelation(e.target.value)}
                className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent">
                {RELATIONS.map((r) => <option key={r} value={r}>{r}</option>)}
              </select>
            </div>
          )}
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
        </div>
      )}

      {mode === "access-diff" && (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <div>
            <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Schema Before</label>
            <textarea value={schemaBefore} onChange={(e) => setSchemaBefore(e.target.value)} rows={8}
              className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text font-mono focus:outline-none focus:border-aegis-accent" />
          </div>
          <div>
            <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Schema After</label>
            <textarea value={schemaAfter} onChange={(e) => setSchemaAfter(e.target.value)} rows={8}
              className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text font-mono focus:outline-none focus:border-aegis-accent" />
          </div>
        </div>
      )}

      <button onClick={handleSimulate} disabled={loading}
        className="px-6 py-2.5 bg-aegis-accent text-white rounded-lg hover:opacity-90 transition-opacity disabled:opacity-50 text-sm font-medium">
        {loading ? "Simulating..." : mode === "dry-run" ? "🧪 Dry Run" : mode === "dry-run-write" ? "📝 Dry-Run Write" : "📊 Compute Diff"}
      </button>

      {error && <div className="p-4 bg-aegis-red/10 border border-aegis-red/30 rounded-lg text-sm text-aegis-red">{error}</div>}

      {result && (mode === "dry-run" || mode === "dry-run-write") && (
        <div className={`p-6 rounded-xl border animate-fade-in ${
          (result as any).dryRunResult?.allowed ? "bg-aegis-green/10 border-aegis-green/30" : "bg-aegis-red/10 border-aegis-red/30"
        }`}>
          <p className="text-xs text-aegis-muted uppercase tracking-wider mb-2">
            Simulated: {subject} → {mode === "dry-run-write" ? `${relation} → ` : ""}{resource} :: {permission}
          </p>
          <div className="flex items-center gap-2">
            <span>{(result as any).dryRunResult?.allowed ? "✅" : "❌"}</span>
            <span className={`font-bold ${(result as any).dryRunResult?.allowed ? "text-aegis-green" : "text-aegis-red"}`}>
              {(result as any).dryRunResult?.allowed ? "ALLOWED" : "DENIED"}
            </span>
          </div>
          <p className="text-xs text-aegis-muted mt-1">
            {(result as any).dryRunResult?.durationMs?.toFixed(2)}ms · revision={(result as any).dryRunResult?.revision}
          </p>
        </div>
      )}

      {result && mode === "access-diff" && (
        <div className="bg-aegis-card border border-aegis-border rounded-xl p-6 animate-fade-in">
          <pre className="text-sm text-aegis-text font-mono whitespace-pre-wrap overflow-auto max-h-96">
            {JSON.stringify(result, null, 2)}
          </pre>
        </div>
      )}
    </div>
  );
}
