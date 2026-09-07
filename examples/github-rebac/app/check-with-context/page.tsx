"use client";

import { useState, useEffect } from "react";
import { useDiscovery } from "@/lib/useDiscovery";
import { ALL_USERS, ALL_RESOURCES, PERMISSIONS } from "@/lib/seed";

export default function CheckWithContextPage() {
  const discovery = useDiscovery();

  // Dynamic lists with seed fallback
  const subjectsList = discovery.subjects.length > 0 ? discovery.subjects : ALL_USERS;
  const permissionsList = discovery.permissions.length > 0 ? discovery.permissions : PERMISSIONS;
  const objectsList = discovery.objects.length > 0 ? discovery.objects : ALL_RESOURCES;

  const [subject, setSubject] = useState("user:alice");
  const [permission, setPermission] = useState("push");
  const [resource, setResource] = useState("repo:payment-api");
  const [dryRun, setDryRun] = useState(false);
  const [consistencyMode, setConsistencyMode] = useState("default");
  const [targetRevision, setTargetRevision] = useState("1");

  const [subjectMeta, setSubjectMeta] = useState<Record<string, string>>({});
  const [resourceMeta, setResourceMeta] = useState<Record<string, string>>({});
  const [env, setEnv] = useState<Record<string, string>>({});
  const [result, setResult] = useState<any>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  // Sync state with dynamic lists when loaded
  useEffect(() => {
    if (discovery.subjects.length > 0) setSubject(discovery.subjects[0]);
    if (discovery.permissions.length > 0) setPermission(discovery.permissions[0]);
    if (discovery.objects.length > 0) {
      const repos = discovery.objects.filter(o => o.startsWith("repo:"));
      if (repos.length > 0) setResource(repos[0]);
      else setResource(discovery.objects[0]);
    }
  }, [discovery.loading]);

  async function handleCheck() {
    setLoading(true); setError(""); setResult(null);
    try {
      const modeString = consistencyMode === "at_revision" ? `at_revision:${targetRevision}` : consistencyMode;
      const res = await fetch("/api/check-with-context", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          subject, permission, resource, dryRun, consistency: modeString,
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
              className="text-aegis-red text-xs hover:underline font-bold">✕</button>
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
        <p className="text-aegis-muted text-sm mt-1">Permission check with condition context (subject meta, resource meta, environment) under a specific consistency profile.</p>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-5 gap-4">
        <div>
          <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Subject</label>
          <select value={subject} onChange={(e) => setSubject(e.target.value)}
            className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent">
            {subjectsList.map((u) => <option key={u} value={u}>{u}</option>)}
          </select>
        </div>
        <div>
          <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Permission</label>
          <select value={permission} onChange={(e) => setPermission(e.target.value)}
            className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent">
            {permissionsList.map((p) => <option key={p} value={p}>{p}</option>)}
          </select>
        </div>
        <div>
          <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Resource</label>
          <select value={resource} onChange={(e) => setResource(e.target.value)}
            className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent">
            {objectsList.filter(o => o.startsWith("repo:")).map((r) => <option key={r} value={r}>{r}</option>)}
            {objectsList.filter(o => !o.startsWith("repo:")).map((r) => <option key={r} value={r}>{r}</option>)}
          </select>
        </div>
        <div>
          <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Consistency Mode</label>
          <select
            value={consistencyMode}
            onChange={(e) => setConsistencyMode(e.target.value)}
            className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
          >
            <option value="default">Default</option>
            <option value="minimize_latency">Minimize Latency</option>
            <option value="fully_consistent">Fully Consistent</option>
            <option value="at_revision">At Revision</option>
          </select>
        </div>
        <div className="flex items-end">
          <label className="flex items-center gap-2 cursor-pointer mb-2">
            <input type="checkbox" checked={dryRun} onChange={(e) => setDryRun(e.target.checked)}
              className="w-4 h-4 rounded border-aegis-border bg-aegis-card accent-aegis-accent" />
            <span className="text-sm text-aegis-text">Dry Run</span>
          </label>
        </div>
      </div>

      {consistencyMode === "at_revision" && (
        <div className="max-w-xs animate-fade-in">
          <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Target Revision Number</label>
          <input
            type="number"
            value={targetRevision}
            onChange={(e) => setTargetRevision(e.target.value)}
            className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
          />
        </div>
      )}

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
