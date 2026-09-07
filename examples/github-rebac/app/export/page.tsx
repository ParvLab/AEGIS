"use client";

import { useState, useEffect } from "react";
import { useDiscovery } from "@/lib/useDiscovery";
import { ALL_USERS } from "@/lib/seed";

export default function ExportPage() {
  const discovery = useDiscovery();
  const subjectsList = discovery.subjects.length > 0 ? discovery.subjects : ALL_USERS;

  const [subject, setSubject] = useState("user:alice");
  const [result, setResult] = useState<Record<string, unknown> | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  // Sync state with dynamic lists when loaded
  useEffect(() => {
    if (discovery.subjects.length > 0) setSubject(discovery.subjects[0]);
  }, [discovery.loading]);

  async function handleExport() {
    setLoading(true); setError(""); setResult(null);
    try {
      const res = await fetch("/api/export", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ subject }),
      });
      const data = await res.json();
      if (data.error) setError(data.error); else setResult(data);
    } catch { setError("Export failed"); }
    setLoading(false);
  }

  function downloadJson() {
    if (!result) return;
    const blob = new Blob([JSON.stringify(result, null, 2)], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url; a.download = `${subject.replace(":", "-")}-export.json`; a.click();
    URL.revokeObjectURL(url);
  }

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold text-aegis-text">Subject Export</h2>
        <p className="text-aegis-muted text-sm mt-1">Export all active tuples for a subject (GDPR portability)</p>
      </div>

      <div className="flex flex-col md:flex-row gap-4">
        <div className="flex-1">
          <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Subject</label>
          <select value={subject} onChange={(e) => setSubject(e.target.value)}
            className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent">
            {subjectsList.map((u) => <option key={u} value={u}>{u}</option>)}
          </select>
        </div>
        <div className="flex items-end">
          <button onClick={handleExport} disabled={loading}
            className="px-6 py-2 bg-aegis-accent text-white rounded-lg hover:opacity-90 transition-opacity disabled:opacity-50 text-sm font-medium">
            {loading ? "Exporting..." : "📤 Export"}
          </button>
        </div>
      </div>

      {error && <div className="p-4 bg-aegis-red/10 border border-aegis-red/30 rounded-lg text-sm text-aegis-red">{error}</div>}

      {result && (
        <div className="space-y-4 animate-fade-in">
          <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
            <div className="bg-aegis-card border border-aegis-border rounded-xl p-4">
              <p className="text-xs text-aegis-muted uppercase tracking-wider">Subject</p>
              <p className="text-sm font-mono font-bold text-aegis-text mt-1">{result.subject as string}</p>
            </div>
            <div className="bg-aegis-card border border-aegis-border rounded-xl p-4">
              <p className="text-xs text-aegis-muted uppercase tracking-wider">Export Revision</p>
              <p className="text-sm font-bold text-aegis-text mt-1">{String(result.exportRevision ?? "—")}</p>
            </div>
            <div className="bg-aegis-card border border-aegis-border rounded-xl p-4">
              <p className="text-xs text-aegis-muted uppercase tracking-wider">Tuples</p>
              <p className="text-sm font-bold text-aegis-text mt-1">{String(Array.isArray(result.activeTuples) ? result.activeTuples.length : 0)}</p>
            </div>
            <div className="bg-aegis-card border border-aegis-border rounded-xl p-4">
              <p className="text-xs text-aegis-muted uppercase tracking-wider">Exported At</p>
              <p className="text-sm font-bold text-aegis-text mt-1">{result.exportedAt ? String(result.exportedAt).slice(0, 19) : "—"}</p>
            </div>
          </div>

          <div className="bg-aegis-card border border-aegis-border rounded-xl overflow-hidden">
            <div className="p-4 border-b border-aegis-border flex items-center justify-between">
              <p className="text-sm font-medium text-aegis-text">Active Tuples</p>
              <button onClick={downloadJson}
                className="px-3 py-1 text-xs bg-aegis-accent/20 text-aegis-accent border border-aegis-accent/30 rounded hover:bg-aegis-accent/30 transition-colors">
                Download JSON
              </button>
            </div>
            {Array.isArray(result.activeTuples) && result.activeTuples.length > 0 ? (
              <div className="overflow-x-auto">
                <table className="w-full text-sm">
                  <thead>
                    <tr className="border-b border-aegis-border text-left">
                      <th className="px-4 py-3 text-aegis-muted text-xs uppercase tracking-wider">Subject</th>
                      <th className="px-4 py-3 text-aegis-muted text-xs uppercase tracking-wider">Relation</th>
                      <th className="px-4 py-3 text-aegis-muted text-xs uppercase tracking-wider">Object</th>
                    </tr>
                  </thead>
                  <tbody>
                    {(result.activeTuples as any[]).map((t: any, i: number) => (
                      <tr key={i} className="border-b border-aegis-border/50 hover:bg-white/5">
                        <td className="px-4 py-3 font-mono text-aegis-text">{t.subject ?? t.subject}</td>
                        <td className="px-4 py-3 font-mono text-aegis-text">{t.relation}</td>
                        <td className="px-4 py-3 font-mono text-aegis-text">{t.object}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            ) : (
              <div className="p-6 text-center text-aegis-muted text-sm">No active tuples found for this subject.</div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
