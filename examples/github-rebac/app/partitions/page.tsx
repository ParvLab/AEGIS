"use client";

import { useState, useEffect } from "react";
import { TENANTS, TenantSeed } from "@/lib/tenants";

interface PartitionInfo {
  id: string;
  isActive: boolean;
  tupleCount: number;
}

export default function PartitionsPage() {
  const [partitions, setPartitions] = useState<PartitionInfo[]>([]);
  const [activePartition, setActivePartition] = useState<string>("default");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [success, setSuccess] = useState("");

  // Isolation Test State
  const [testPartA, setTestPartA] = useState("default");
  const [testPartB, setTestPartB] = useState("");
  const [testSubject, setTestSubject] = useState("user:alice");
  const [testPermission, setTestPermission] = useState("push");
  const [testResource, setTestResource] = useState("repo:api-gateway");
  const [testResult, setTestResult] = useState<{
    allowedA: boolean;
    allowedB: boolean;
    revisionA: number;
    revisionB: number;
    tested: boolean;
  } | null>(null);

  // New Partition State
  const [newPartId, setNewPartId] = useState("");

  async function fetchPartitions() {
    setLoading(true);
    try {
      const res = await fetch("/api/partitions");
      if (res.ok) {
        const data = await res.json();
        setPartitions(data.partitions ?? []);
        setActivePartition(data.active ?? "default");
        if (data.partitions && data.partitions.length > 0) {
          if (!testPartA) setTestPartA(data.active);
          const firstNonActive = data.partitions.find((p: any) => p.id !== data.active);
          if (firstNonActive && !testPartB) setTestPartB(firstNonActive.id);
        }
      }
    } catch {
      setError("Failed to fetch partitions");
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    fetchPartitions();
  }, []);

  async function handleCreatePartition() {
    if (!newPartId.trim()) return;
    setError("");
    setSuccess("");
    try {
      const res = await fetch("/api/partitions", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action: "create", id: newPartId.trim() }),
      });
      const data = await res.json();
      if (data.error) {
        setError(data.error);
      } else {
        setSuccess(`Partition "${newPartId}" created successfully!`);
        setNewPartId("");
        fetchPartitions();
      }
    } catch {
      setError("Failed to create partition");
    }
  }

  async function handleDeletePartition(id: string) {
    if (!confirm(`Are you sure you want to delete partition "${id}" and all its data?`)) return;
    setError("");
    setSuccess("");
    try {
      const res = await fetch("/api/partitions", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action: "delete", id }),
      });
      const data = await res.json();
      if (data.error) {
        setError(data.error);
      } else {
        setSuccess(`Partition "${id}" deleted.`);
        fetchPartitions();
      }
    } catch {
      setError("Failed to delete partition");
    }
  }

  async function handleSwitchPartition(id: string) {
    setError("");
    setSuccess("");
    try {
      const res = await fetch("/api/partitions", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action: "switch", id }),
      });
      const data = await res.json();
      if (data.error) {
        setError(data.error);
      } else {
        setSuccess(`Switched active partition to "${id}"`);
        fetchPartitions();
      }
    } catch {
      setError("Failed to switch partition");
    }
  }

  async function handleSeedPartition(id: string) {
    setError("");
    setSuccess("");
    try {
      const res = await fetch("/api/partitions", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action: "seed", id }),
      });
      const data = await res.json();
      if (data.error) {
        setError(data.error);
      } else {
        setSuccess(`Successfully seeded partition "${id}" with tenant data.`);
        fetchPartitions();
      }
    } catch {
      setError("Failed to seed partition");
    }
  }

  async function handleRunIsolationTest() {
    setError("");
    setTestResult(null);
    try {
      const res = await fetch("/api/partitions", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          action: "isolation-test",
          partitionA: testPartA,
          partitionB: testPartB,
          subject: testSubject,
          permission: testPermission,
          resource: testResource,
        }),
      });
      const data = await res.json();
      if (data.error) {
        setError(data.error);
      } else {
        setTestResult({
          allowedA: data.allowedA,
          allowedB: data.allowedB,
          revisionA: data.revisionA,
          revisionB: data.revisionB,
          tested: true,
        });
      }
    } catch {
      setError("Failed to execute isolation test");
    }
  }

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold text-aegis-text">Partitions & Tenants</h2>
        <p className="text-aegis-muted text-sm mt-1">
          Manage multi-tenant partitions, switch tenant workspaces, and test security isolation boundaries.
        </p>
      </div>

      {error && (
        <div className="p-4 bg-aegis-red/10 border border-aegis-red/30 rounded-lg text-sm text-aegis-red">
          {error}
        </div>
      )}

      {success && (
        <div className="p-4 bg-aegis-green/10 border border-aegis-green/30 rounded-lg text-sm text-aegis-green">
          {success}
        </div>
      )}

      {/* Active Partition Banner */}
      <div className="p-6 bg-gradient-to-r from-aegis-accent/10 to-transparent border border-aegis-border rounded-xl flex items-center justify-between">
        <div>
          <span className="text-xs uppercase tracking-wider text-aegis-accent font-semibold">Active Tenant Workspace</span>
          <h3 className="text-2xl font-bold text-aegis-text flex items-center gap-2 mt-1">
            <span className="w-2.5 h-2.5 rounded-full bg-aegis-green animate-pulse" />
            {activePartition}
          </h3>
        </div>
        <button
          onClick={() => {
            const el = document.getElementById("isolation-test-card");
            el?.scrollIntoView({ behavior: "smooth" });
          }}
          className="px-4 py-2 border border-aegis-accent/50 text-aegis-accent hover:bg-aegis-accent/10 rounded-lg text-xs font-semibold uppercase tracking-wider transition-colors"
        >
          Verify Isolation
        </button>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* Partitions List */}
        <div className="lg:col-span-2 space-y-4">
          <div className="bg-aegis-card border border-aegis-border rounded-xl p-6">
            <h4 className="text-lg font-bold text-aegis-text mb-4">Workspace Partitions</h4>
            <div className="space-y-3">
              {partitions.map((p) => {
                const tenantSeed = TENANTS.find(t => t.id === p.id);
                return (
                  <div
                    key={p.id}
                    className={`p-4 rounded-lg border transition-all ${
                      p.isActive
                        ? "border-aegis-accent bg-aegis-accent/5 shadow-[0_0_15px_rgba(var(--aegis-accent-rgb),0.1)]"
                        : "border-aegis-border bg-aegis-card/50 hover:border-aegis-accent/40"
                    } flex flex-col md:flex-row md:items-center justify-between gap-4`}
                  >
                    <div>
                      <div className="flex items-center gap-2">
                        <span className="font-semibold text-aegis-text">{p.id}</span>
                        {p.isActive && (
                          <span className="px-2 py-0.5 bg-aegis-accent text-white text-[10px] uppercase font-bold rounded">
                            Active
                          </span>
                        )}
                        {tenantSeed && (
                          <span className="px-2 py-0.5 bg-aegis-border text-aegis-muted text-[10px] rounded">
                            Tenant Seed
                          </span>
                        )}
                      </div>
                      <p className="text-xs text-aegis-muted mt-1 max-w-md">
                        {tenantSeed ? tenantSeed.description : "Custom user-defined partition workspace."}
                      </p>
                      <div className="text-xs text-aegis-muted mt-2">
                        Tuples Indexed: <span className="font-semibold text-aegis-text">{p.tupleCount}</span>
                      </div>
                    </div>

                    <div className="flex items-center gap-2 self-end md:self-center">
                      {!p.isActive && (
                        <button
                          onClick={() => handleSwitchPartition(p.id)}
                          className="px-3 py-1.5 bg-aegis-accent/10 hover:bg-aegis-accent text-aegis-accent hover:text-white text-xs font-semibold rounded-lg transition-all"
                        >
                          Switch To
                        </button>
                      )}
                      {p.tupleCount === 0 && tenantSeed && (
                        <button
                          onClick={() => handleSeedPartition(p.id)}
                          className="px-3 py-1.5 bg-aegis-green/10 hover:bg-aegis-green text-aegis-green hover:text-white text-xs font-semibold rounded-lg transition-all"
                        >
                          Seed Dataset
                        </button>
                      )}
                      {p.id !== "default" && !p.isActive && (
                        <button
                          onClick={() => handleDeletePartition(p.id)}
                          className="px-3 py-1.5 bg-aegis-red/10 hover:bg-aegis-red text-aegis-red hover:text-white text-xs font-semibold rounded-lg transition-all"
                        >
                          Delete
                        </button>
                      )}
                    </div>
                  </div>
                );
              })}
            </div>

            {/* Create Partition Form */}
            <div className="mt-6 pt-6 border-t border-aegis-border flex items-center gap-3">
              <input
                type="text"
                placeholder="New partition ID (e.g. tenant-spacex)"
                value={newPartId}
                onChange={(e) => setNewPartId(e.target.value.toLowerCase().replace(/[^a-z0-9-_]/g, ""))}
                className="flex-1 bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
              />
              <button
                onClick={handleCreatePartition}
                className="px-4 py-2 bg-aegis-accent text-white rounded-lg hover:opacity-90 transition-opacity text-sm font-semibold"
              >
                + Create Partition
              </button>
            </div>
          </div>
        </div>

        {/* Tenant Template Reference */}
        <div className="space-y-4">
          <div className="bg-aegis-card border border-aegis-border rounded-xl p-6">
            <h4 className="text-md font-bold text-aegis-text mb-2">Available Tenant Seeds</h4>
            <p className="text-xs text-aegis-muted mb-4">
              A list of template tenants available to seed into your engine partitions.
            </p>
            <div className="space-y-3 max-h-[380px] overflow-y-auto pr-1">
              {TENANTS.map((t) => {
                const isLoaded = partitions.some(p => p.id === t.id && p.tupleCount > 0);
                return (
                  <div key={t.id} className="p-3 bg-aegis-card/40 border border-aegis-border rounded-lg space-y-1">
                    <div className="flex items-center justify-between">
                      <span className="font-semibold text-xs text-aegis-text">{t.name}</span>
                      <span className={`text-[10px] px-1.5 py-0.5 rounded font-medium ${isLoaded ? "bg-aegis-green/20 text-aegis-green" : "bg-aegis-muted/20 text-aegis-muted"}`}>
                        {isLoaded ? "Seeded" : "Unseeded"}
                      </span>
                    </div>
                    <p className="text-[11px] text-aegis-muted leading-relaxed">{t.description}</p>
                    <span className="text-[10px] text-aegis-accent block pt-1">
                      {t.tuples.length} relationship tuples
                    </span>
                  </div>
                );
              })}
            </div>
          </div>
        </div>
      </div>

      {/* Isolation Test Section */}
      <div id="isolation-test-card" className="bg-aegis-card border border-aegis-border rounded-xl p-6">
        <h4 className="text-lg font-bold text-aegis-text mb-2">🔒 Partition Isolation Verification</h4>
        <p className="text-xs text-aegis-muted mb-6">
          Prove that tenant partitions are completely isolated. Select two partitions and verify that checking a permission for a subject returns allowed in its own partition but denied in the other partition.
        </p>

        <div className="grid grid-cols-1 md:grid-cols-5 gap-4 items-end">
          <div>
            <label className="block text-[10px] text-aegis-muted mb-1 uppercase tracking-wider">Partition A</label>
            <select
              value={testPartA}
              onChange={(e) => setTestPartA(e.target.value)}
              className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
            >
              {partitions.map(p => <option key={p.id} value={p.id}>{p.id}</option>)}
            </select>
          </div>
          <div>
            <label className="block text-[10px] text-aegis-muted mb-1 uppercase tracking-wider">Partition B</label>
            <select
              value={testPartB}
              onChange={(e) => setTestPartB(e.target.value)}
              className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
            >
              {partitions.map(p => <option key={p.id} value={p.id}>{p.id}</option>)}
            </select>
          </div>
          <div>
            <label className="block text-[10px] text-aegis-muted mb-1 uppercase tracking-wider">Subject</label>
            <input
              type="text"
              value={testSubject}
              onChange={(e) => setTestSubject(e.target.value)}
              className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
            />
          </div>
          <div>
            <label className="block text-[10px] text-aegis-muted mb-1 uppercase tracking-wider">Permission / Resource</label>
            <div className="flex gap-2">
              <input
                type="text"
                value={testPermission}
                onChange={(e) => setTestPermission(e.target.value)}
                className="w-1/3 bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
              />
              <input
                type="text"
                value={testResource}
                onChange={(e) => setTestResource(e.target.value)}
                className="w-2/3 bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
              />
            </div>
          </div>
          <button
            onClick={handleRunIsolationTest}
            className="w-full py-2 bg-aegis-accent text-white rounded-lg hover:opacity-90 font-semibold text-sm transition-opacity"
          >
            Run Isolation Test
          </button>
        </div>

        {testResult && (
          <div className="grid grid-cols-1 md:grid-cols-2 gap-6 mt-6 pt-6 border-t border-aegis-border">
            <div className={`p-4 rounded-xl border ${testResult.allowedA ? "bg-aegis-green/5 border-aegis-green/20" : "bg-aegis-red/5 border-aegis-red/20"}`}>
              <span className="text-[10px] uppercase font-bold text-aegis-muted">Partition: {testPartA}</span>
              <div className="flex items-center gap-3 mt-2">
                <span className="text-3xl">{testResult.allowedA ? "✅" : "❌"}</span>
                <div>
                  <h5 className={`font-bold text-lg ${testResult.allowedA ? "text-aegis-green" : "text-aegis-red"}`}>
                    {testResult.allowedA ? "ALLOWED" : "DENIED"}
                  </h5>
                  <p className="text-xs text-aegis-muted">revision={testResult.revisionA}</p>
                </div>
              </div>
            </div>

            <div className={`p-4 rounded-xl border ${testResult.allowedB ? "bg-aegis-green/5 border-aegis-green/20" : "bg-aegis-red/5 border-aegis-red/20"}`}>
              <span className="text-[10px] uppercase font-bold text-aegis-muted">Partition: {testPartB}</span>
              <div className="flex items-center gap-3 mt-2">
                <span className="text-3xl">{testResult.allowedB ? "✅" : "❌"}</span>
                <div>
                  <h5 className={`font-bold text-lg ${testResult.allowedB ? "text-aegis-green" : "text-aegis-red"}`}>
                    {testResult.allowedB ? "ALLOWED" : "DENIED"}
                  </h5>
                  <p className="text-xs text-aegis-muted">revision={testResult.revisionB}</p>
                </div>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
