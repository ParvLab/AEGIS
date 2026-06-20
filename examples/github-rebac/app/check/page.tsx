"use client";

import { useState } from "react";
import { ALL_USERS, ALL_RESOURCES, PERMISSIONS } from "@/lib/seed";

interface CheckResult {
  allowed: boolean;
  revision: number;
  durationMs: number;
}

export default function CheckPage() {
  const [subject, setSubject] = useState("user:alice");
  const [permission, setPermission] = useState("push");
  const [resource, setResource] = useState("repo:payment-api");
  const [dryRun, setDryRun] = useState(false);
  const [result, setResult] = useState<CheckResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  async function handleCheck() {
    setLoading(true);
    setError("");
    setResult(null);
    try {
      const res = await fetch("/api/check", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ subject, permission, resource, dryRun }),
      });
      const data = await res.json();
      if (data.error) { setError(data.error); } else { setResult(data); }
    } catch { setError("Request failed"); }
    setLoading(false);
  }

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold text-aegis-text">Permission Check</h2>
        <p className="text-aegis-muted text-sm mt-1">
          Test whether a subject has a permission on a resource
        </p>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <div>
          <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Subject</label>
          <select
            value={subject}
            onChange={(e) => setSubject(e.target.value)}
            className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
          >
            {ALL_USERS.map((u) => <option key={u} value={u}>{u}</option>)}
            <option value="user:mallory">user:mallory</option>
          </select>
        </div>
        <div>
          <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Permission</label>
          <select
            value={permission}
            onChange={(e) => setPermission(e.target.value)}
            className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
          >
            {PERMISSIONS.map((p) => <option key={p} value={p}>{p}</option>)}
          </select>
        </div>
        <div>
          <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Resource</label>
          <select
            value={resource}
            onChange={(e) => setResource(e.target.value)}
            className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
          >
            {ALL_RESOURCES.filter((r) => r.startsWith("repo:")).map((r) => <option key={r} value={r}>{r}</option>)}
          </select>
        </div>
      </div>

      <div className="flex items-center gap-4">
        <button
          onClick={handleCheck}
          disabled={loading}
          className="px-6 py-2.5 bg-aegis-accent text-white rounded-lg hover:opacity-90 transition-opacity disabled:opacity-50 text-sm font-medium"
        >
          {loading ? "Checking..." : dryRun ? "🧪 Dry Run" : "🔍 Check"}
        </button>
        <label className="flex items-center gap-2 text-sm text-aegis-muted cursor-pointer">
          <input
            type="checkbox"
            checked={dryRun}
            onChange={(e) => setDryRun(e.target.checked)}
            className="rounded border-aegis-border"
          />
          Dry run (no side effects)
        </label>
      </div>

      {error && (
        <div className="p-4 bg-aegis-red/10 border border-aegis-red/30 rounded-lg text-sm text-aegis-red animate-fade-in">
          {error}
        </div>
      )}

      {result && (
        <div className={`p-6 rounded-xl border animate-fade-in ${result.allowed ? "bg-aegis-green/10 border-aegis-green/30" : "bg-aegis-red/10 border-aegis-red/30"}`}>
          <div className="flex items-center gap-3">
            <span className={`text-2xl ${result.allowed ? "text-aegis-green" : "text-aegis-red"}`}>
              {result.allowed ? "✅" : "❌"}
            </span>
            <div>
              <p className={`text-lg font-bold ${result.allowed ? "text-aegis-green" : "text-aegis-red"}`}>
                {result.allowed ? "ALLOWED" : "DENIED"}
              </p>
              <p className="text-xs text-aegis-muted mt-1">
                revision={result.revision} &middot; {result.durationMs.toFixed(2)}ms
              </p>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
