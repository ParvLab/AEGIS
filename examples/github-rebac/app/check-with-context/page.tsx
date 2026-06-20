"use client";

import { useState } from "react";
import { ALL_USERS, ALL_RESOURCES, PERMISSIONS } from "@/lib/seed";

export default function CheckWithContextPage() {
  const [subject, setSubject] = useState("user:alice");
  const [permission, setPermission] = useState("push");
  const [resource, setResource] = useState("repo:payment-api");
  const [dryRun, setDryRun] = useState(false);
  const [subjectMeta, setSubjectMeta] = useState<Record<string, string>>({});
  const [resourceMeta, setResourceMeta] = useState<Record<string, string>>({});
  const [env, setEnv] = useState<Record<string, string>>({});
  const [result, setResult] = useState<Record<string, unknown> | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  async function handleCheck() {
    setLoading(true); setError(""); setResult(null);
    try {
      const res = await fetch("/api/check-with-context", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          subject, permission, resource, dryRun,
          context: { subjectMeta, resourceMeta, env },
        }),
      });
      const data = await res.json();
      if (data.error) setError(data.error); else setResult(data);
    } catch { setError("Request failed"); }
    setLoading(false);
  }

  function KeyValEditor({ label, map, setMap }: { label: string; map: Record<string, string>; setMap: (m: Record<string, string>) => void }) {
    return (
      <div className="bg-aegis-card border border-aegis-border rounded-lg p-3">
        <p className="text-xs text-aegis-muted uppercase tracking-wider mb-2">{label}</p>
        {Object.entries(map).length === 0 && <p className="text-xs text-aegis-muted">No entries</p>}
        {Object.entries(map).map(([k, v], i) => (
          <div key={i} className="flex gap-2 mb-2">
            <input value={k} onChange={(e) => { const m = { ...map }; delete m[k]; m[e.target.value] = v; setMap(m); }}
              className="flex-1 bg-aegis-bg border border-aegis-border rounded px-2 py-1 text-xs text-aegis-text font-mono focus:outline-none focus:border-aegis-accent" placeholder="key" />
            <input value={v} onChange={(e) => setMap({ ...map, [k]: e.target.value })}
              className="flex-1 bg-aegis-bg border border-aegis-border rounded px-2 py-1 text-xs text-aegis-text font-mono focus:outline-none focus:border-aegis-accent" placeholder="value" />
            <button onClick={() => { const m = { ...map }; delete m[k]; setMap(m); }}
              className="text-aegis-red text-xs hover:underline">✕</button>
          </div>
        ))}
        <button onClick={() => setMap({ ...map, ["new"]: "" })}
          className="text-xs text-aegis-accent hover:underline mt-1">+ Add</button>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold text-aegis-text">Check with Context</h2>
        <p className="text-aegis-muted text-sm mt-1">Permission check with condition context (subject meta, resource meta, environment)</p>
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
            <input type="checkbox" checked={dryRun} onChange={(e) => setDryRun(e.target.checked)}
              className="w-4 h-4 rounded border-aegis-border bg-aegis-card accent-aegis-accent" />
            <span className="text-sm text-aegis-text">Dry Run</span>
          </label>
        </div>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <KeyValEditor label="Subject Meta" map={subjectMeta} setMap={setSubjectMeta} />
        <KeyValEditor label="Resource Meta" map={resourceMeta} setMap={setResourceMeta} />
        <KeyValEditor label="Environment" map={env} setMap={setEnv} />
      </div>

      <button onClick={handleCheck} disabled={loading}
        className="px-6 py-2.5 bg-aegis-accent text-white rounded-lg hover:opacity-90 transition-opacity disabled:opacity-50 text-sm font-medium">
        {loading ? "Checking..." : dryRun ? "🧪 Dry Run Check" : "✓ Check Permission"}
      </button>

      {error && <div className="p-4 bg-aegis-red/10 border border-aegis-red/30 rounded-lg text-sm text-aegis-red">{error}</div>}

      {result && (
        <div className={`p-6 rounded-xl border animate-fade-in ${result.allowed ? "bg-aegis-green/10 border-aegis-green/30" : "bg-aegis-red/10 border-aegis-red/30"}`}>
          <div className="flex items-center gap-2 mb-2">
            <span>{result.allowed ? "✅" : "❌"}</span>
            <span className={`font-bold ${result.allowed ? "text-aegis-green" : "text-aegis-red"}`}>
              {result.allowed ? "ALLOWED" : "DENIED"}
            </span>
          </div>
          <p className="text-xs text-aegis-muted">revision={String(result.revision)} · {String(result.durationMs)}ms</p>
        </div>
      )}
    </div>
  );
}
