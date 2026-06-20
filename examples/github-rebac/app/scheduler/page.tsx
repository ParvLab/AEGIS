"use client";

import { useEffect, useState, useCallback } from "react";

export default function SchedulerPage() {
  const [schedules, setSchedules] = useState<any[]>([]);
  const [runs, setRuns] = useState<any[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [newName, setNewName] = useState("");
  const [newInterval, setNewInterval] = useState("60");
  const [newQueries, setNewQueries] = useState('[{"subject":"user:alice","permission":"push","resource":"repo:payment-api"}]');

  const fetchData = useCallback(async () => {
    setLoading(true); setError("");
    try {
      const res = await fetch("/api/scheduler");
      const data = await res.json();
      if (data.error) setError(data.error);
      else { setSchedules(data.schedules ?? []); setRuns(data.runs ?? []); }
    } catch { setError("Failed to load"); }
    setLoading(false);
  }, []);

  useEffect(() => { fetchData(); }, [fetchData]);

  async function handleAction(action: string, extra: Record<string, unknown> = {}) {
    try {
      await fetch("/api/scheduler", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action, ...extra }),
      });
      fetchData();
    } catch { setError("Action failed"); }
  }

  const statusColor = (status: string) => {
    const m: Record<string, string> = { completed: "text-aegis-green", running: "text-aegis-blue", failed: "text-aegis-red" };
    return m[status] || "text-aegis-muted";
  };

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold text-aegis-text">V7 Scheduler</h2>
        <p className="text-aegis-muted text-sm mt-1">Create and manage recurring permission analysis schedules</p>
      </div>

      <div className="bg-aegis-card border border-aegis-border rounded-xl p-6">
        <p className="text-sm font-medium text-aegis-text mb-4">Create New Schedule</p>
        <div className="grid grid-cols-1 md:grid-cols-3 gap-4 mb-4">
          <input type="text" value={newName} onChange={(e) => setNewName(e.target.value)}
            placeholder="Schedule name" className="bg-aegis-bg border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text placeholder-aegis-muted focus:outline-none focus:border-aegis-accent" />
          <input type="number" value={newInterval} onChange={(e) => setNewInterval(e.target.value)}
            placeholder="Interval (seconds)" className="bg-aegis-bg border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text placeholder-aegis-muted focus:outline-none focus:border-aegis-accent" />
        </div>
        <textarea value={newQueries} onChange={(e) => setNewQueries(e.target.value)} rows={3}
          className="w-full bg-aegis-bg border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text font-mono focus:outline-none focus:border-aegis-accent mb-4"
          placeholder='[{"subject":"user:alice","permission":"push","resource":"repo:payment-api"}]' />
        <button onClick={() => { if (newName) { handleAction("create", { name: newName, intervalSeconds: Number(newInterval), queriesJson: newQueries }); setNewName(""); } }}
          disabled={!newName}
          className="px-6 py-2 bg-aegis-accent text-white rounded-lg hover:opacity-90 transition-opacity disabled:opacity-50 text-sm font-medium">
          + Create Schedule
        </button>
      </div>

      {error && <div className="p-4 bg-aegis-red/10 border border-aegis-red/30 rounded text-sm text-aegis-red">{error}</div>}

      {loading ? (
        <div className="flex items-center justify-center h-32 text-aegis-muted">Loading...</div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
          <div className="bg-aegis-card border border-aegis-border rounded-xl p-6">
            <p className="text-sm font-medium text-aegis-text mb-4">Schedules ({schedules.length})</p>
            {schedules.length === 0 ? (
              <p className="text-xs text-aegis-muted">No schedules yet.</p>
            ) : (
              <div className="space-y-3">
                {schedules.map((s: any, i: number) => (
                  <div key={i} className="p-4 bg-aegis-bg rounded-lg border border-aegis-border">
                    <div className="flex items-center justify-between mb-2">
                      <p className="text-sm font-mono font-medium text-aegis-text">{s.name || s.id}</p>
                      <div className="flex gap-2">
                        <button onClick={() => handleAction("run", { scheduleId: s.id })}
                          className="px-2 py-1 text-xs bg-aegis-blue/20 text-aegis-blue rounded hover:bg-aegis-blue/30">Run Now</button>
                        <button onClick={() => handleAction("delete", { scheduleId: s.id })}
                          className="px-2 py-1 text-xs bg-aegis-red/20 text-aegis-red rounded hover:bg-aegis-red/30">Delete</button>
                      </div>
                    </div>
                    <p className="text-xs text-aegis-muted">Interval: {s.intervalSeconds}s · Enabled: {String(!!s.enabled)}</p>
                  </div>
                ))}
              </div>
            )}
          </div>

          <div className="bg-aegis-card border border-aegis-border rounded-xl p-6">
            <p className="text-sm font-medium text-aegis-text mb-4">Recent Runs ({runs.length})</p>
            {runs.length === 0 ? (
              <p className="text-xs text-aegis-muted">No runs yet.</p>
            ) : (
              <div className="space-y-3">
                {runs.slice(0, 20).map((r: any, i: number) => (
                  <div key={i} className="p-4 bg-aegis-bg rounded-lg border border-aegis-border">
                    <div className="flex items-center justify-between mb-1">
                      <p className="text-sm font-mono text-aegis-text">{r.id}</p>
                      <span className={`text-xs font-medium ${statusColor(r.status)}`}>{r.status}</span>
                    </div>
                    <p className="text-xs text-aegis-muted">Started: {r.startedAt ? String(r.startedAt).slice(0, 19) : "—"}</p>
                    {r.errorMessage && <p className="text-xs text-aegis-red mt-1">{r.errorMessage}</p>}
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
