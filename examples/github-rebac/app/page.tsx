"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import TypeCards from "./components/TypeCards";

export default function DashboardPage() {
  const [health, setHealth] = useState<any>(null);
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
    try {
      await fetch("/api/reset", { method: "POST" });
      setMessage("Engine reset and re-seeded");
    } catch { setMessage("Failed to reset"); }
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
      <div className="flex items-center justify-between flex-wrap gap-4">
        <div>
          <h2 className="text-2xl font-bold text-aegis-text">Dashboard</h2>
          <p className="text-aegis-muted text-sm mt-1">AEGIS authorization engine &mdash; GitHub-style ReBAC demo</p>
        </div>

        {/* Tenant and Actor Badges */}
        <div className="flex items-center gap-3">
          <div className="px-3 py-1.5 bg-aegis-accent/15 border border-aegis-accent/20 rounded-lg text-xs flex items-center gap-2">
            <span className="text-aegis-muted">Workspace:</span>
            <Link href="/partitions" className="font-bold text-aegis-accent hover:underline">
              {health.activePartition ?? "default"}
            </Link>
          </div>
          <div className="px-3 py-1.5 bg-aegis-border/30 border border-aegis-border rounded-lg text-xs flex items-center gap-2">
            <span className="text-aegis-muted">Actor:</span>
            <Link href="/actor" className="font-bold text-aegis-text hover:underline font-mono">
              {health.activeActor ?? "anonymous"}
            </Link>
          </div>
        </div>
      </div>

      {/* 8-Card Stat Grid */}
      <div className="grid grid-cols-2 sm:grid-cols-4 lg:grid-cols-8 gap-4">
        <StatCard label="Revision" value={String(health.revision ?? "—")} />
        <StatCard label="Status" value={health.healthy ? "Healthy" : "Error"} color={health.healthy ? "green" : "red"} />
        <StatCard label="Cache Hit Rate" value={health.cacheHitRate != null ? `${(Number(health.cacheHitRate) * 100).toFixed(0)}%` : "—"} />
        <StatCard label="Cache Entries" value={String(health.cacheEntries ?? "—")} />
        <StatCard label="Total Checks" value={String(health.totalChecks ?? "0")} />
        <StatCard label="Allowed Checks" value={String(health.allowedChecks ?? "0")} color="green" />
        <StatCard label="Denied Checks" value={String(health.deniedChecks ?? "0")} color="red" />
        <StatCard label="WAL size" value={`${(health.walSizeMb ?? 0).toFixed(2)} MB`} />
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

      <div className="bg-aegis-card border border-aegis-border rounded-xl p-6 shadow-sm">
        <p className="text-sm font-bold text-aegis-text mb-3">Schema Migration</p>
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

      <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 gap-4">
        <QuickLink href="/check" label="🔍 Check" desc="Test whether a subject has a permission" />
        <QuickLink href="/explain" label="🧬 Explain" desc="See why access was granted or denied" />
        <QuickLink href="/graph" label="📊 Graph Explorer" desc="Visualize relationships and paths" />
        <QuickLink href="/entities" label="👥 Entities Manager" desc="Form-based relationship manager" />
        <QuickLink href="/tuples" label="📝 Tuples Editor" desc="Add, list, or ban tuples directly" />
        <QuickLink href="/partitions" label="🏢 Tenants & Partitions" desc="Configure multi-tenant data boundaries" />
        <QuickLink href="/analysis" label="🔐 Analysis & Integrity" desc="Verify audit chain and review actor access" />
        <QuickLink href="/events" label="📡 Events Stream" desc="Watch write activities in real-time" />
        <QuickLink href="/rate-limiter" label="⏳ Rate Limiter" desc="Manage token bucket traffic and stress test" />
        <QuickLink href="/backup" label="💾 Backup & Restore" desc="Export JSON and snapshot SQLite backups" />
        <QuickLink href="/actor" label="👤 Actor Identity" desc="Bind current session user handle" />
        <QuickLink href="/errors" label="⚠️ Error Playground" desc="Audit fail paths and exception responses" />
      </div>

      <TypeCards />
    </div>
  );
}

function StatCard({ label, value, color }: { label: string; value: string; color?: string }) {
  const colorMap: Record<string, string> = { green: "text-aegis-green", red: "text-aegis-red" };
  return (
    <div className="bg-aegis-card border border-aegis-border rounded-xl p-4 shadow-sm">
      <p className="text-[10px] text-aegis-muted uppercase tracking-wider font-semibold">{label}</p>
      <p className={`text-xl font-bold mt-1.5 ${color ? (colorMap[color] ?? "") : "text-aegis-text"}`}>{value}</p>
    </div>
  );
}

function QuickLink({ href, label, desc }: { href: string; label: string; desc: string }) {
  return <Link href={href}
    className="block p-4 bg-aegis-card border border-aegis-border rounded-xl hover:border-aegis-accent/50 transition-colors shadow-sm">
    <p className="text-sm font-semibold text-aegis-text">{label}</p>
    <p className="text-xs text-aegis-muted mt-1 leading-relaxed">{desc}</p>
  </Link>;
}
