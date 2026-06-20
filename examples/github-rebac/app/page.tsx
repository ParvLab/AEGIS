"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import TypeCards from "./components/TypeCards";

export default function DashboardPage() {
  const [health, setHealth] = useState<Record<string, unknown> | null>(null);
  const [seeding, setSeeding] = useState(false);
  const [message, setMessage] = useState("");
  const [cacheMsg, setCacheMsg] = useState("");
  const [migrateTarget, setMigrateTarget] = useState("1");

  useEffect(() => { fetchHealth(); }, []);

  async function fetchHealth() {
    try {
      const res = await fetch("/api/health");
      const data = await res.json();
      setHealth(data);
    } catch {
      setHealth({ healthy: false, error: "Failed to connect" });
    }
  }

  async function seed(level: string) {
    setSeeding(true); setMessage("");
    try {
      const res = await fetch("/api/seed", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ level }) });
      const data = await res.json();
      setMessage(`Seeded ${data.tuplesWritten} tuples (revision ${data.revision})`);
    } catch { setMessage("Failed to seed"); }
    setSeeding(false);
    fetchHealth();
  }

  async function reset() {
    setSeeding(true); setMessage("");
    try { await fetch("/api/reset", { method: "POST" }); setMessage("Engine reset and re-seeded"); }
    catch { setMessage("Failed to reset"); }
    setSeeding(false);
    fetchHealth();
  }

  async function invalidateCache() {
    setCacheMsg("");
    try {
      await fetch("/api/cache", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ action: "invalidate" }) });
      setCacheMsg("Cache invalidated"); fetchHealth();
    } catch { setCacheMsg("Failed"); }
  }

  async function handleMigrate() {
    setCacheMsg("");
    try {
      const res = await fetch("/api/cache", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ action: "migrate", targetVersion: Number(migrateTarget) }) });
      const data = await res.json();
      setCacheMsg(`Migrated to schema version ${data.schemaVersion}`);
      fetchHealth();
    } catch { setCacheMsg("Migration failed"); }
  }

  if (!health) {
    return <div className="flex items-center justify-center h-64"><div className="text-aegis-muted">Loading engine status...</div></div>;
  }

  return (
    <div className="space-y-8">
      <div>
        <h2 className="text-2xl font-bold text-aegis-text">Dashboard</h2>
        <p className="text-aegis-muted text-sm mt-1">AEGIS authorization engine &mdash; GitHub-style ReBAC demo</p>
      </div>

      <div className="grid grid-cols-2 md:grid-cols-5 gap-4">
        <StatCard label="Revision" value={String(health.revision ?? "—")} />
        <StatCard label="Status" value={health.healthy ? "Healthy" : "Error"} color={health.healthy ? "green" : "red"} />
        <StatCard label="Cache Hit Rate" value={health.cacheHitRate != null ? `${(Number(health.cacheHitRate) * 100).toFixed(0)}%` : "—"} />
        <StatCard label="Cache Entries" value={String(health.cacheEntries ?? health.cacheSize ?? "—")} />
        <StatCard label="Backend" value="SQLite WAL" />
      </div>

      <div className="grid grid-cols-1 md:grid-cols-4 gap-4">
        <button onClick={() => seed("minimal")} disabled={seeding}
          className="px-4 py-3 bg-aegis-accent/10 border border-aegis-accent/30 text-aegis-accent rounded-lg hover:bg-aegis-accent/20 transition-colors disabled:opacity-50 text-sm font-medium">
          Seed Minimal (11 tuples)
        </button>
        <button onClick={() => seed("full")} disabled={seeding}
          className="px-4 py-3 bg-aegis-blue/10 border border-aegis-blue/30 text-aegis-blue rounded-lg hover:bg-aegis-blue/20 transition-colors disabled:opacity-50 text-sm font-medium">
          Seed Full (21 tuples)
        </button>
        <button onClick={reset} disabled={seeding}
          className="px-4 py-3 bg-aegis-red/10 border border-aegis-red/30 text-aegis-red rounded-lg hover:bg-aegis-red/20 transition-colors disabled:opacity-50 text-sm font-medium">
          Reset Engine
        </button>
        <button onClick={invalidateCache}
          className="px-4 py-3 bg-aegis-amber/10 border border-aegis-amber/30 text-aegis-amber rounded-lg hover:bg-aegis-amber/20 transition-colors text-sm font-medium">
          Clear Cache
        </button>
      </div>

      {(message || cacheMsg) && (
        <div className="p-4 bg-aegis-card border border-aegis-border rounded-lg text-sm text-aegis-text animate-fade-in">{message || cacheMsg}</div>
      )}

      <div className="bg-aegis-card border border-aegis-border rounded-xl p-6">
        <p className="text-sm font-medium text-aegis-text mb-3">Schema Migration</p>
        <div className="flex gap-4">
          <input type="number" value={migrateTarget} onChange={(e) => setMigrateTarget(e.target.value)}
            className="w-32 bg-aegis-bg border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent" />
          <button onClick={handleMigrate}
            className="px-4 py-2 bg-aegis-accent text-white rounded-lg hover:opacity-90 text-sm font-medium">
            Migrate to Version
          </button>
        </div>
        <p className="text-xs text-aegis-muted mt-2">Current schema version: {String(health.schemaVersion ?? "?")}</p>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <QuickLink href="/check" label="Check" desc="Test a permission" />
        <QuickLink href="/explain" label="Explain" desc="See why access was granted or denied" />
        <QuickLink href="/graph" label="Graph Explorer" desc="Visualize the access graph" />
        <QuickLink href="/who-can-access" label="Who Can Access" desc="Reverse permission lookup" />
        <QuickLink href="/simulate" label="Simulation" desc="What-if analysis" />
        <QuickLink href="/policies" label="Policies" desc="V7 policy lifecycle" />
        <QuickLink href="/schema" label="Schema Editor" desc="Edit YAML schema live" />
        <QuickLink href="/audit" label="Audit" desc="View change history" />
        <QuickLink href="/scheduler" label="Scheduler" desc="V7 analysis schedules" />
      </div>

      <TypeCards />
    </div>
  );
}

function StatCard({ label, value, color }: { label: string; value: string; color?: string }) {
  const colorMap: Record<string, string> = { green: "text-aegis-green", red: "text-aegis-red" };
  return (
    <div className="bg-aegis-card border border-aegis-border rounded-xl p-4">
      <p className="text-xs text-aegis-muted uppercase tracking-wider">{label}</p>
      <p className={`text-2xl font-bold mt-1 ${color ? (colorMap[color] ?? "") : "text-aegis-text"}`}>{value}</p>
    </div>
  );
}

function QuickLink({ href, label, desc }: { href: string; label: string; desc: string }) {
  return <Link href={href}
    className="block p-4 bg-aegis-card border border-aegis-border rounded-xl hover:border-aegis-accent/50 transition-colors">
    <p className="text-sm font-medium text-aegis-text">{label}</p>
    <p className="text-xs text-aegis-muted mt-1">{desc}</p>
  </Link>;
}
