"use client";

import { useState } from "react";
import { ALL_RESOURCES, PERMISSIONS } from "@/lib/seed";

interface SubjectEntry {
  subject: string;
  path?: string[];
}

export default function WhoCanAccessPage() {
  const [permission, setPermission] = useState("push");
  const [resource, setResource] = useState("repo:payment-api");
  const [pageOffset, setPageOffset] = useState(0);
  const [pageLimit, setPageLimit] = useState(10);
  const [includePaths, setIncludePaths] = useState(true);
  const [data, setData] = useState<{ subjects: SubjectEntry[]; subjectNames: string[]; totalCount: number; nextOffset?: number } | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  async function handleQuery(offset?: number) {
    setLoading(true); setError(""); setData(null);
    try {
      const res = await fetch("/api/who-can-access", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ permission, resource, pageOffset: offset ?? pageOffset, pageLimit, includePaths }),
      });
      const result = await res.json();
      if (result.error) setError(result.error); else setData(result);
    } catch { setError("Request failed"); }
    setLoading(false);
  }

  const totalStr = data ? `${data.subjects.length} shown${data.totalCount ? ` of ${data.totalCount} total` : ""}` : "";

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold text-aegis-text">Who Can Access?</h2>
        <p className="text-aegis-muted text-sm mt-1">Reverse permission lookup with pagination</p>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-4 gap-4">
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
        <div>
          <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Page Limit</label>
          <input type="number" value={pageLimit} onChange={(e) => setPageLimit(Number(e.target.value))}
            className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent" />
        </div>
        <div className="flex items-end">
          <label className="flex items-center gap-2 cursor-pointer">
            <input type="checkbox" checked={includePaths} onChange={(e) => setIncludePaths(e.target.checked)}
              className="w-4 h-4 rounded border-aegis-border bg-aegis-card accent-aegis-accent" />
            <span className="text-sm text-aegis-text">Show paths</span>
          </label>
        </div>
      </div>

      <button onClick={() => handleQuery()} disabled={loading}
        className="px-6 py-2.5 bg-aegis-accent text-white rounded-lg hover:opacity-90 transition-opacity disabled:opacity-50 text-sm font-medium">
        {loading ? "Searching..." : "🔍 Who Can Access?"}
      </button>

      {error && <div className="p-4 bg-aegis-red/10 border border-aegis-red/30 rounded-lg text-sm text-aegis-red">{error}</div>}

      {data && (
        <div className="bg-aegis-green/10 border border-aegis-green/30 rounded-xl p-6 animate-fade-in">
          <div className="flex items-center justify-between mb-3">
            <p className="text-sm font-medium text-aegis-green">✅ {totalStr}</p>
            {pageOffset > 0 && (
              <button onClick={() => handleQuery(0)}
                className="text-xs px-3 py-1 bg-aegis-accent/20 text-aegis-accent rounded hover:bg-aegis-accent/30">First</button>
            )}
          </div>

          {data.subjects.length === 0 ? (
            <p className="text-sm text-aegis-muted">No subjects found</p>
          ) : (
            <div className="space-y-2">
              {data.subjects.map((entry, i) => (
                <div key={i} className="p-3 bg-aegis-card rounded-lg border border-aegis-border">
                  <p className="text-sm font-mono text-aegis-text">{entry.subject}</p>
                  {entry.path && entry.path.length > 0 && includePaths && (
                    <p className="text-xs text-aegis-muted mt-1 font-mono">{entry.path.join(" → ")}</p>
                  )}
                </div>
              ))}
            </div>
          )}

          {data.nextOffset != null && (
            <div className="flex items-center justify-center gap-4 mt-4">
              {pageOffset > 0 && (
                <button onClick={() => { setPageOffset(Math.max(0, pageOffset - pageLimit)); handleQuery(Math.max(0, pageOffset - pageLimit)); }}
                  className="px-4 py-2 bg-aegis-card border border-aegis-border text-aegis-text rounded-lg text-sm hover:bg-white/5">← Prev</button>
              )}
              {data.nextOffset > 0 && (
                <button onClick={() => { setPageOffset(pageOffset + pageLimit); handleQuery(pageOffset + pageLimit); }}
                  className="px-4 py-2 bg-aegis-card border border-aegis-border text-aegis-text rounded-lg text-sm hover:bg-white/5">Next →</button>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
