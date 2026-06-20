"use client";

import { useEffect, useState, useCallback } from "react";

export default function EnforcementPage() {
  const [config, setConfig] = useState<any>(null);
  const [trends, setTrends] = useState<any>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [editConfig, setEditConfig] = useState("");

  const fetchData = useCallback(async () => {
    setLoading(true); setError("");
    try {
      const res = await fetch("/api/enforcement");
      const data = await res.json();
      if (data.error) setError(data.error);
      else {
        setConfig(data.config);
        setTrends(data.trends);
        setEditConfig(JSON.stringify(data.config, null, 2));
      }
    } catch { setError("Failed to load"); }
    setLoading(false);
  }, []);

  useEffect(() => { fetchData(); }, [fetchData]);

  async function handleSaveConfig() {
    try {
      const parsed = JSON.parse(editConfig);
      await fetch("/api/enforcement", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action: "config", configJson: JSON.stringify(parsed) }),
      });
      fetchData();
    } catch { setError("Invalid JSON or save failed"); }
  }

  const trendsData = trends?.recentEvents?.length > 0
    ? { totalEvents: trends.totalEvents, deniedCount: trends.deniedCount, allowedCount: trends.allowedCount, recentEvents: trends.recentEvents, byResource: trends.byResource }
    : null;

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold text-aegis-text">V7 Enforcement History</h2>
        <p className="text-aegis-muted text-sm mt-1">Configure and view enforcement event tracking</p>
      </div>

      {loading ? (
        <div className="flex items-center justify-center h-32 text-aegis-muted">Loading...</div>
      ) : (
        <>
          {error && <div className="p-4 bg-aegis-red/10 border border-aegis-red/30 rounded text-sm text-aegis-red">{error}</div>}

          <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
            <div className="bg-aegis-card border border-aegis-border rounded-xl p-6">
              <p className="text-sm font-medium text-aegis-text mb-4">Configuration</p>
              <textarea value={editConfig} onChange={(e) => setEditConfig(e.target.value)} rows={8}
                className="w-full bg-aegis-bg border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text font-mono focus:outline-none focus:border-aegis-accent mb-4" />
              <button onClick={handleSaveConfig}
                className="px-4 py-2 bg-aegis-accent text-white rounded-lg hover:opacity-90 text-sm font-medium">
                Save Config
              </button>
            </div>

            <div className="space-y-4">
              {trendsData ? (
                <>
                  <div className="grid grid-cols-3 gap-3">
                    <div className="bg-aegis-card border border-aegis-border rounded-xl p-4">
                      <p className="text-xs text-aegis-muted uppercase">Total Events</p>
                      <p className="text-xl font-bold text-aegis-text mt-1">{trendsData.totalEvents}</p>
                    </div>
                    <div className="bg-aegis-card border border-aegis-border rounded-xl p-4">
                      <p className="text-xs text-aegis-muted uppercase">Allowed</p>
                      <p className="text-xl font-bold text-aegis-green mt-1">{trendsData.allowedCount}</p>
                    </div>
                    <div className="bg-aegis-card border border-aegis-border rounded-xl p-4">
                      <p className="text-xs text-aegis-muted uppercase">Denied</p>
                      <p className="text-xl font-bold text-aegis-red mt-1">{trendsData.deniedCount}</p>
                    </div>
                  </div>

                  {trendsData.recentEvents.length > 0 && (
                    <div className="bg-aegis-card border border-aegis-border rounded-xl p-6">
                      <p className="text-sm font-medium text-aegis-text mb-3">Recent Events</p>
                      <div className="space-y-2">
                        {trendsData.recentEvents.slice(0, 10).map((e: any, i: number) => (
                          <div key={i} className="flex items-center justify-between p-3 bg-aegis-bg rounded-lg border border-aegis-border">
                            <div className="flex items-center gap-2 text-sm font-mono">
                              <span className={e.allowed ? "text-aegis-green" : "text-aegis-red"}>{e.allowed ? "✅" : "❌"}</span>
                              <span className="text-aegis-text">{e.subject}</span>
                              <span className="text-aegis-muted">→</span>
                              <span className="text-aegis-text">{e.permission}</span>
                              <span className="text-aegis-muted">→</span>
                              <span className="text-aegis-text">{e.resource}</span>
                            </div>
                          </div>
                        ))}
                      </div>
                    </div>
                  )}
                </>
              ) : (
                <div className="bg-aegis-card border border-aegis-border rounded-xl p-6">
                  <p className="text-sm text-aegis-muted">No enforcement data yet. Enable enforcement tracking and perform some permission checks to see trends.</p>
                </div>
              )}
            </div>
          </div>
        </>
      )}
    </div>
  );
}
