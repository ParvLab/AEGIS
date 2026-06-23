"use client";

import { useState } from "react";
import Link from "next/link";
import { useDiscovery } from "@/lib/useDiscovery";
import { ALL_RESOURCES } from "@/lib/seed";

export default function AuditPage() {
  const discovery = useDiscovery();
  const objectsList = discovery.objects.length > 0 ? discovery.objects : ALL_RESOURCES;

  const [object, setObject] = useState("");
  const [fromRevision, setFromRevision] = useState("");
  const [toRevision, setToRevision] = useState("");
  const [limit, setLimit] = useState("50");
  const [all, setAll] = useState(true);
  const [entries, setEntries] = useState<any[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  async function handleQuery() {
    setLoading(true); setError(""); setEntries([]);
    try {
      const res = await fetch("/api/audit", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          object: all ? undefined : object || undefined,
          fromRevision: fromRevision ? Number(fromRevision) : undefined,
          toRevision: toRevision ? Number(toRevision) : undefined,
          limit: Number(limit) || 100,
          all,
        }),
      });
      const data = await res.json();
      if (data.error) setError(data.error); else setEntries(data.entries ?? []);
    } catch { setError("Failed to load audit"); }
    setLoading(false);
  }

  const actionColor = (action: string) => {
    const colors: Record<string, string> = {
      write: "text-aegis-green",
      delete: "text-aegis-red",
      check: "text-aegis-blue",
    };
    return colors[action] || "text-aegis-text";
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between flex-wrap gap-4">
        <div>
          <h2 className="text-2xl font-bold text-aegis-text">Audit Trail</h2>
          <p className="text-aegis-muted text-sm mt-1">View the full history of tuple changes and permission checks</p>
        </div>
        <Link
          href="/analysis"
          className="px-4 py-2 border border-aegis-accent text-aegis-accent hover:bg-aegis-accent/10 rounded-lg text-xs font-semibold uppercase tracking-wider transition-colors"
        >
          🔒 Verify Chain Integrity
        </Link>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-5 gap-4">
        <div className="flex items-end">
          <label className="flex items-center gap-2 cursor-pointer mb-2">
            <input type="checkbox" checked={all} onChange={(e) => setAll(e.target.checked)}
              className="w-4 h-4 rounded border-aegis-border bg-aegis-card accent-aegis-accent" />
            <span className="text-sm text-aegis-text font-medium">All events</span>
          </label>
        </div>
        <div>
          <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Object (filter)</label>
          <select value={object} onChange={(e) => setObject(e.target.value)} disabled={all}
            className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent disabled:opacity-50">
            <option value="">All objects</option>
            {objectsList.map((r) => <option key={r} value={r}>{r}</option>)}
          </select>
        </div>
        <div>
          <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">From Revision</label>
          <input type="number" value={fromRevision} onChange={(e) => setFromRevision(e.target.value)}
            className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent" placeholder="0" />
        </div>
        <div>
          <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">To Revision</label>
          <input type="number" value={toRevision} onChange={(e) => setToRevision(e.target.value)}
            className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent" placeholder="latest" />
        </div>
        <div>
          <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Limit</label>
          <input type="number" value={limit} onChange={(e) => setLimit(e.target.value)}
            className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent" />
        </div>
      </div>

      <button onClick={handleQuery} disabled={loading}
        className="px-6 py-2.5 bg-aegis-accent text-white rounded-lg hover:opacity-90 transition-opacity disabled:opacity-50 text-sm font-medium">
        {loading ? "Loading..." : "📋 Query Audit"}
      </button>

      {error && <div className="p-4 bg-aegis-red/10 border border-aegis-red/30 rounded-lg text-sm text-aegis-red">{error}</div>}

      {entries.length === 0 && !loading && (
        <div className="bg-aegis-card border border-aegis-border rounded-xl p-6">
          <p className="text-sm text-aegis-muted">No audit entries found.</p>
        </div>
      )}

      {entries.length > 0 && (
        <div className="bg-aegis-card border border-aegis-border rounded-xl overflow-hidden shadow-sm">
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-aegis-border text-left">
                  <th className="px-4 py-3 text-aegis-muted text-xs uppercase tracking-wider">Revision</th>
                  <th className="px-4 py-3 text-aegis-muted text-xs uppercase tracking-wider">Action</th>
                  <th className="px-4 py-3 text-aegis-muted text-xs uppercase tracking-wider">Subject</th>
                  <th className="px-4 py-3 text-aegis-muted text-xs uppercase tracking-wider">Relation</th>
                  <th className="px-4 py-3 text-aegis-muted text-xs uppercase tracking-wider">Object</th>
                  <th className="px-4 py-3 text-aegis-muted text-xs uppercase tracking-wider">Actor Identity</th>
                  <th className="px-4 py-3 text-aegis-muted text-xs uppercase tracking-wider">Timestamp</th>
                </tr>
              </thead>
              <tbody>
                {entries.map((entry: any, i: number) => (
                  <tr key={i} className="border-b border-aegis-border/50 hover:bg-white/5">
                    <td className="px-4 py-3 font-mono text-aegis-text">{entry.revision}</td>
                    <td className={`px-4 py-3 font-medium ${actionColor(entry.action)}`}>{entry.action}</td>
                    <td className="px-4 py-3 font-mono text-aegis-text">{entry.subject}</td>
                    <td className="px-4 py-3 font-mono text-aegis-text">{entry.relation}</td>
                    <td className="px-4 py-3 font-mono text-aegis-text">{entry.object}</td>
                    <td className="px-4 py-3">
                      {entry.identity ? (
                        <span className="px-2 py-0.5 bg-aegis-amber/10 border border-aegis-amber/30 text-aegis-amber font-mono rounded text-xs">
                          {entry.identity}
                        </span>
                      ) : (
                        <span className="text-aegis-muted text-xs font-mono">anonymous</span>
                      )}
                    </td>
                    <td className="px-4 py-3 text-aegis-muted text-xs">{entry.timestamp}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div className="p-3 border-t border-aegis-border text-xs text-aegis-muted">
            {entries.length} entr{entries.length === 1 ? "y" : "ies"}
          </div>
        </div>
      )}
    </div>
  );
}
