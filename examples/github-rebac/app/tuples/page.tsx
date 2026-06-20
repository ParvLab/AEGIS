"use client";

import { useState, useEffect, useCallback } from "react";
import { ALL_USERS, ALL_RESOURCES, RELATIONS } from "@/lib/seed";

type Tab = "single" | "batch" | "query" | "delete-object" | "transaction";

export default function TuplesPage() {
  const [tab, setTab] = useState<Tab>("single");
  const [action, setAction] = useState<"write" | "delete" | "ban" | "unban" | "dry-run-write">("write");
  const [subject, setSubject] = useState("user:alice");
  const [relation, setRelation] = useState("admin");
  const [resource, setResource] = useState("repo:payment-api");
  const [condition, setCondition] = useState("");
  const [validUntil, setValidUntil] = useState("");
  const [metadata, setMetadata] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<Record<string, unknown> | null>(null);
  const [error, setError] = useState("");
  const [bans, setBans] = useState<Array<{ subject: string; relation: string; object: string }>>([]);

  // Batch
  const [batchJson, setBatchJson] = useState('[{"subject":"user:frank","relation":"member","object":"team:engineering"}]');

  // Query
  const [qObject, setQObject] = useState("");
  const [qSubject, setQSubject] = useState("");
  const [qRelation, setQRelation] = useState("");
  const [qMode, setQMode] = useState<"object" | "subject" | "relation">("object");
  const [qResult, setQResult] = useState<any[]>([]);
  const [qLoading, setQLoading] = useState(false);

  // Delete object
  const [delObject, setDelObject] = useState("repo:docs");

  // Transaction
  const [txId, setTxId] = useState<string | null>(null);
  const [txLog, setTxLog] = useState<string[]>([]);

  const fetchBans = useCallback(async () => {
    try {
      const res = await fetch("/api/list", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ mode: "relation", target: "repo:banned", relation: "banned" }),
      });
      if (res.ok) { const data = await res.json(); if (data.tuples) setBans(data.tuples); }
    } catch { /* ignore */ }
  }, []);

  useEffect(() => { fetchBans(); }, [fetchBans]);

  async function handleAction() {
    setLoading(true); setError(""); setResult(null);
    try {
      const body: Record<string, unknown> = { action, subject, relation: action === "ban" || action === "unban" ? "banned" : relation, resource };
      if (condition) body.condition = condition;
      if (validUntil) body.validUntil = validUntil;
      if (Object.keys(metadata).length > 0) body.metadata = metadata;
      const res = await fetch("/api/tuples", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      const data = await res.json();
      if (data.error) setError(data.error); else setResult(data);
    } catch { setError("Request failed"); }
    setLoading(false);
    fetchBans();
  }

  async function handleBatchWrite() {
    setLoading(true); setError(""); setResult(null);
    try {
      const tuples = JSON.parse(batchJson);
      const res = await fetch("/api/tuples", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action: "batch-write", tuples }),
      });
      const data = await res.json();
      if (data.error) setError(data.error); else setResult(data);
    } catch { setError("Invalid JSON or request failed"); }
    setLoading(false);
  }

  async function handleQuery() {
    setQLoading(true); setError("");
    try {
      const req: Record<string, unknown> = { action: `list-by-${qMode}` };
      if (qMode === "object") { req.object = qObject; req.relation = qRelation || undefined; }
      if (qMode === "subject") { req.subject = qSubject; req.relation = qRelation || undefined; }
      if (qMode === "relation") { req.object = qObject; req.relation = qRelation; }
      const res = await fetch("/api/tuples", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(req),
      });
      const data = await res.json();
      if (data.error) setError(data.error); else setQResult(data.tuples ?? []);
    } catch { setError("Query failed"); }
    setQLoading(false);
  }

  async function handleDeleteObject() {
    setLoading(true); setError(""); setResult(null);
    try {
      const res = await fetch("/api/tuples", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action: "delete-object", object: delObject }),
      });
      const data = await res.json();
      if (data.error) setError(data.error); else setResult(data);
    } catch { setError("Request failed"); }
    setLoading(false);
    fetchBans();
  }

  async function handleTx(action: string, extra: Record<string, unknown> = {}) {
    try {
      const res = await fetch("/api/transaction", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action, transactionId: txId, ...extra }),
      });
      const data = await res.json();
      if (data.transactionId) setTxId(data.transactionId);
      if (data.error) { setError(data.error); } else { setTxLog((p) => [...p, `${action}: ${JSON.stringify(data)}`]); }
      if (action === "commit" || action === "rollback") { setTxId(null); fetchBans(); }
    } catch { setError("TX action failed"); }
  }

  const actionColor = (a: string) => {
    if (a === "ban" || a === "unban") return "bg-aegis-red";
    return "bg-aegis-accent";
  };

  const activeAction = tab === "single" ? (["write", "delete", "ban", "unban", "dry-run-write"].includes(action) ? action : "write") as typeof action : "write";

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold text-aegis-text">Tuples</h2>
        <p className="text-aegis-muted text-sm mt-1">Manage authorization tuples</p>
      </div>

      <div className="flex flex-wrap gap-2">
        {(["single", "batch", "query", "delete-object", "transaction"] as const).map((t) => (
          <button key={t} onClick={() => setTab(t)}
            className={`px-4 py-2 rounded-lg text-sm font-medium transition-colors ${
              tab === t ? "bg-aegis-accent/20 text-aegis-accent border border-aegis-accent/40"
                : "bg-aegis-card border border-aegis-border text-aegis-muted hover:text-aegis-text"
            }`}>
            {t === "single" ? "Single" : t === "batch" ? "Batch" : t === "query" ? "Query" : t === "delete-object" ? "Delete Object" : "Transaction"}
          </button>
        ))}
      </div>

      {tab === "single" && (
        <>
          <div className="flex gap-2">
            {(["write", "delete", "ban", "unban", "dry-run-write"] as const).map((a) => (
              <button key={a} onClick={() => setAction(a)}
                className={`px-4 py-2 rounded-lg text-sm font-medium transition-colors ${
                  action === a
                    ? a === "ban" || a === "unban"
                      ? "bg-aegis-red/20 text-aegis-red border border-aegis-red/40"
                      : a === "dry-run-write"
                        ? "bg-aegis-amber/20 text-aegis-amber border border-aegis-amber/40"
                        : "bg-aegis-accent/20 text-aegis-accent border border-aegis-accent/40"
                    : "bg-aegis-card border border-aegis-border text-aegis-muted hover:text-aegis-text"
                }`}>
                {a === "ban" ? "🚫 Ban" : a === "unban" ? "✅ Unban" : a === "dry-run-write" ? "🧪 Dry-Run Write" : a.charAt(0).toUpperCase() + a.slice(1)}
              </button>
            ))}
          </div>

          <div className="grid grid-cols-1 md:grid-cols-4 gap-4">
            <div>
              <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Subject</label>
              <select value={subject} onChange={(e) => setSubject(e.target.value)}
                className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent">
                {ALL_USERS.map((u) => <option key={u} value={u}>{u}</option>)}
              </select>
            </div>
            <div>
              <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Relation</label>
              {action === "ban" || action === "unban" ? (
                <div className="w-full bg-aegis-card border border-aegis-red/30 rounded-lg px-3 py-2 text-sm text-aegis-red font-mono">banned</div>
              ) : (
                <select value={relation} onChange={(e) => setRelation(e.target.value)}
                  className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent">
                  {RELATIONS.map((r) => <option key={r} value={r}>{r}</option>)}
                </select>
              )}
            </div>
            <div>
              <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Resource</label>
              <select value={resource} onChange={(e) => setResource(e.target.value)}
                className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent">
                {ALL_RESOURCES.filter((r) => r.startsWith("repo:")).map((r) => <option key={r} value={r}>{r}</option>)}
              </select>
            </div>
            <div>
              <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Valid Until</label>
              <input type="datetime-local" value={validUntil} onChange={(e) => setValidUntil(e.target.value)}
                className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent" />
            </div>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div>
              <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Condition</label>
              <input type="text" value={condition} onChange={(e) => setCondition(e.target.value)} placeholder="e.g., time_between(9,17)"
                className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text placeholder-aegis-muted focus:outline-none focus:border-aegis-accent font-mono" />
            </div>
            <div>
              <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Metadata</label>
              <div className="flex gap-2">
                <input type="text" value={metadata["key"] ?? ""} onChange={(e) => setMetadata({ ...metadata, key: e.target.value })}
                  placeholder="key" className="flex-1 bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text placeholder-aegis-muted focus:outline-none focus:border-aegis-accent font-mono" />
                <input type="text" value={metadata["value"] ?? ""} onChange={(e) => setMetadata({ ...metadata, value: e.target.value })}
                  placeholder="value" className="flex-1 bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text placeholder-aegis-muted focus:outline-none focus:border-aegis-accent font-mono" />
              </div>
            </div>
          </div>

          <button onClick={handleAction} disabled={loading}
            className={`px-6 py-2.5 rounded-lg text-sm font-medium transition-opacity disabled:opacity-50 text-white ${actionColor(action)}`}>
            {loading ? "Processing..." : action === "write" ? "📝 Write" : action === "delete" ? "🗑️ Delete" : action === "ban" ? "🚫 Ban" : action === "dry-run-write" ? "🧪 Dry-Run" : "✅ Unban"}
          </button>
        </>
      )}

      {tab === "batch" && (
        <div className="bg-aegis-card border border-aegis-border rounded-xl p-6">
          <p className="text-sm font-medium text-aegis-text mb-2">Batch Write Tuples</p>
          <p className="text-xs text-aegis-muted mb-4">Paste a JSON array of tuples.</p>
          <textarea value={batchJson} onChange={(e) => setBatchJson(e.target.value)} rows={6}
            className="w-full bg-aegis-bg border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text font-mono focus:outline-none focus:border-aegis-accent mb-4" />
          <button onClick={handleBatchWrite} disabled={loading}
            className="px-6 py-2 bg-aegis-accent text-white rounded-lg hover:opacity-90 disabled:opacity-50 text-sm font-medium">
            {loading ? "Writing..." : "📦 Batch Write"}
          </button>
        </div>
      )}

      {tab === "query" && (
        <div className="space-y-4">
          <div className="flex gap-2">
            {(["object", "subject", "relation"] as const).map((m) => (
              <button key={m} onClick={() => setQMode(m)}
                className={`px-3 py-1.5 text-xs rounded-lg transition-colors ${
                  qMode === m ? "bg-aegis-accent/20 text-aegis-accent border border-aegis-accent/40"
                    : "bg-aegis-card border border-aegis-border text-aegis-muted"
                }`}>{`listBy${m.charAt(0).toUpperCase() + m.slice(1)}`}</button>
            ))}
          </div>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            {qMode !== "subject" && (
              <div>
                <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Object</label>
                <input type="text" value={qObject} onChange={(e) => setQObject(e.target.value)}
                  className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent font-mono" placeholder="repo:docs" />
              </div>
            )}
            {qMode !== "object" && (
              <div>
                <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Subject</label>
                <input type="text" value={qSubject} onChange={(e) => setQSubject(e.target.value)}
                  className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent font-mono" placeholder="team:engineering" />
              </div>
            )}
            <div>
              <label className="block text-xs text-aegis-muted mb-1 uppercase tracking-wider">Relation (optional)</label>
              <input type="text" value={qRelation} onChange={(e) => setQRelation(e.target.value)}
                className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent font-mono" placeholder="member" />
            </div>
          </div>
          <button onClick={handleQuery} disabled={qLoading}
            className="px-6 py-2 bg-aegis-accent text-white rounded-lg hover:opacity-90 disabled:opacity-50 text-sm font-medium">
            {qLoading ? "Querying..." : "🔍 Query"}
          </button>
          {qResult.length > 0 && (
            <div className="bg-aegis-card border border-aegis-border rounded-xl overflow-hidden">
              <div className="overflow-x-auto">
                <table className="w-full text-sm">
                  <thead><tr className="border-b border-aegis-border text-left">
                    <th className="px-4 py-3 text-aegis-muted text-xs uppercase">Subject</th>
                    <th className="px-4 py-3 text-aegis-muted text-xs uppercase">Relation</th>
                    <th className="px-4 py-3 text-aegis-muted text-xs uppercase">Object</th>
                  </tr></thead>
                  <tbody>{qResult.map((t: any, i: number) => (
                    <tr key={i} className="border-b border-aegis-border/50 hover:bg-white/5">
                      <td className="px-4 py-3 font-mono text-aegis-text text-xs">{t.subject}</td>
                      <td className="px-4 py-3 font-mono text-aegis-text text-xs">{t.relation}</td>
                      <td className="px-4 py-3 font-mono text-aegis-text text-xs">{t.object}</td>
                    </tr>
                  ))}</tbody>
                </table>
              </div>
              <div className="p-3 border-t border-aegis-border text-xs text-aegis-muted">{qResult.length} tuples</div>
            </div>
          )}
        </div>
      )}

      {tab === "delete-object" && (
        <div className="bg-aegis-card border border-aegis-border rounded-xl p-6">
          <p className="text-sm font-medium text-aegis-text mb-4">Delete All Tuples for an Object</p>
          <div className="flex gap-4">
            <select value={delObject} onChange={(e) => setDelObject(e.target.value)}
              className="flex-1 bg-aegis-bg border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent">
              {ALL_RESOURCES.map((r) => <option key={r} value={r}>{r}</option>)}
            </select>
            <button onClick={handleDeleteObject} disabled={loading}
              className="px-6 py-2 bg-aegis-red text-white rounded-lg hover:opacity-90 disabled:opacity-50 text-sm font-medium">
              {loading ? "Deleting..." : "🗑️ Delete Object"}
            </button>
          </div>
        </div>
      )}

      {tab === "transaction" && (
        <div className="bg-aegis-card border border-aegis-border rounded-xl p-6">
          <p className="text-sm font-medium text-aegis-text mb-4">Atomic Transaction</p>
          <div className="flex gap-2 flex-wrap mb-4">
            {!txId ? (
              <button onClick={() => handleTx("begin")}
                className="px-4 py-2 bg-aegis-accent text-white rounded-lg text-sm font-medium hover:opacity-90">🔓 Begin TX</button>
            ) : (
              <>
                <span className="text-xs text-aegis-green font-mono bg-aegis-green/10 px-3 py-2 rounded border border-aegis-green/30">TX: {txId}</span>
                <button onClick={() => handleTx("write", { subject: "user:frank", relation: "member", resource: "team:engineering" })}
                  className="px-3 py-2 bg-aegis-accent/20 text-aegis-accent rounded-lg text-xs font-medium">Write</button>
                <button onClick={() => handleTx("savepoint", { savepointName: "sp1" })}
                  className="px-3 py-2 bg-aegis-blue/20 text-aegis-blue rounded-lg text-xs font-medium">Savepoint</button>
                <button onClick={() => handleTx("rollbackToSavepoint", { savepointName: "sp1" })}
                  className="px-3 py-2 bg-aegis-amber/20 text-aegis-amber rounded-lg text-xs font-medium">Rollback</button>
                <button onClick={() => handleTx("commit")}
                  className="px-3 py-2 bg-aegis-green text-white rounded-lg text-xs font-medium">Commit</button>
                <button onClick={() => handleTx("rollback")}
                  className="px-3 py-2 bg-aegis-red text-white rounded-lg text-xs font-medium">Rollback All</button>
              </>
            )}
          </div>
          {txLog.length > 0 && (
            <div className="bg-aegis-bg rounded-lg p-3 max-h-32 overflow-y-auto border border-aegis-border">
              {txLog.map((l, i) => <p key={i} className="text-xs font-mono text-aegis-text">{l}</p>)}
            </div>
          )}
        </div>
      )}

      {error && <div className="p-4 bg-aegis-red/10 border border-aegis-red/30 rounded-lg text-sm text-aegis-red">{error}</div>}
      {result && (
        <div className="p-4 bg-aegis-green/10 border border-aegis-green/30 rounded-lg text-sm text-aegis-green animate-fade-in">
          ✓ {String(result.action)} (revision={String(result.revision)})
        </div>
      )}

      {bans.length > 0 && (
        <div className="bg-aegis-card border border-aegis-border rounded-xl p-6">
          <p className="text-xs text-aegis-muted uppercase tracking-wider mb-3 flex items-center gap-2"><span>🚫</span> Current Bans</p>
          <div className="space-y-2">
            {bans.map((b, i) => (
              <div key={i} className="flex items-center justify-between p-3 bg-aegis-red/5 border border-aegis-red/20 rounded-lg">
                <div className="flex items-center gap-2 text-sm">
                  <span className="text-aegis-text font-mono">{b.subject}</span>
                  <span className="text-aegis-muted">→</span>
                  <span className="text-aegis-red font-medium">{b.relation}</span>
                  <span className="text-aegis-muted">→</span>
                  <span className="text-aegis-text font-mono">{b.object}</span>
                </div>
                <button onClick={async () => {
                  await fetch("/api/tuples", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ action: "unban", subject: b.subject, relation: "banned", resource: b.object }) });
                  fetchBans();
                }} className="text-xs px-3 py-1 bg-aegis-green/20 text-aegis-green rounded hover:bg-aegis-green/30 transition-colors">Unban</button>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
