"use client";

import { useEffect, useState, useCallback } from "react";

interface Draft {
  id: string;
  name: string;
  status: string;
  description?: string;
  createdAt?: string;
  schema?: string;
}

interface Version {
  version: number;
  name?: string;
  publishedAt?: string;
}

export default function PoliciesPage() {
  const [drafts, setDrafts] = useState<Draft[]>([]);
  const [versions, setVersions] = useState<Version[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [newName, setNewName] = useState("");
  const [newDesc, setNewDesc] = useState("");
  const [editingDraftId, setEditingDraftId] = useState<string | null>(null);
  const [editSchema, setEditSchema] = useState("");
  const [rollbackTarget, setRollbackTarget] = useState<number | null>(null);

  const fetchData = useCallback(async () => {
    setLoading(true); setError("");
    try {
      const res = await fetch("/api/policies");
      const data = await res.json();
      if (data.error) setError(data.error); else {
        setDrafts(data.drafts || []);
        setVersions(data.versions || []);
      }
    } catch { setError("Failed to load policies"); }
    setLoading(false);
  }, []);

  useEffect(() => { fetchData(); }, [fetchData]);

  async function handleAction(action: string, extra: Record<string, string | number> = {}) {
    try {
      await fetch("/api/policies", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action, ...extra }),
      });
      fetchData();
    } catch { setError("Action failed"); }
  }

  const statusBadge = (status: string) => {
    const styles: Record<string, string> = {
      draft: "bg-aegis-amber/20 text-aegis-amber border-aegis-amber/30",
      under_review: "bg-aegis-blue/20 text-aegis-blue border-aegis-blue/30",
      approved: "bg-aegis-green/20 text-aegis-green border-aegis-green/30",
      published: "bg-aegis-green/20 text-aegis-green border-aegis-green/30",
      rejected: "bg-aegis-red/20 text-aegis-red border-aegis-red/30",
      archived: "bg-aegis-muted/20 text-aegis-muted border-aegis-muted/30",
    };
    return styles[status] || "bg-aegis-card border-aegis-border text-aegis-muted";
  };

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold text-aegis-text">Policy Lifecycle</h2>
        <p className="text-aegis-muted text-sm mt-1">V7 draft lifecycle &mdash; create, review, approve, publish, rollback</p>
      </div>

      <div className="bg-aegis-card border border-aegis-border rounded-xl p-6">
        <p className="text-sm font-medium text-aegis-text mb-4">Create New Draft</p>
        <div className="flex flex-col md:flex-row gap-3">
          <input type="text" placeholder="Draft name (e.g., v2-schema)" value={newName} onChange={(e) => setNewName(e.target.value)}
            className="flex-1 bg-aegis-bg border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text placeholder-aegis-muted focus:outline-none focus:border-aegis-accent" />
          <input type="text" placeholder="Description" value={newDesc} onChange={(e) => setNewDesc(e.target.value)}
            className="flex-1 bg-aegis-bg border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text placeholder-aegis-muted focus:outline-none focus:border-aegis-accent" />
          <button onClick={() => { if (newName) { handleAction("create", { name: newName, description: newDesc || "No description" }); setNewName(""); setNewDesc(""); } }}
            disabled={!newName}
            className="px-6 py-2 bg-aegis-accent text-white rounded-lg hover:opacity-90 transition-opacity disabled:opacity-50 text-sm font-medium whitespace-nowrap">+ Create</button>
        </div>
      </div>

      {error && <div className="p-4 bg-aegis-red/10 border border-aegis-red/30 rounded-lg text-sm text-aegis-red">{error}</div>}

      {loading ? (
        <div className="flex items-center justify-center h-32 text-aegis-muted">Loading...</div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
          <div className="bg-aegis-card border border-aegis-border rounded-xl p-6">
            <p className="text-sm font-medium text-aegis-text mb-4">Drafts ({drafts.length})</p>
            {drafts.length === 0 ? (
              <p className="text-xs text-aegis-muted">No drafts yet. Create one above.</p>
            ) : (
              <div className="space-y-3">
                {drafts.map((draft) => (
                  <div key={draft.id} className="p-4 bg-aegis-bg rounded-lg border border-aegis-border">
                    <div className="flex items-center justify-between mb-2">
                      <p className="text-sm font-mono font-medium text-aegis-text">{draft.name}</p>
                      <span className={`text-xs px-2 py-0.5 rounded-full border ${statusBadge(draft.status)}`}>
                        {draft.status.replace("_", " ")}
                      </span>
                    </div>
                    {draft.description && <p className="text-xs text-aegis-muted mb-2">{draft.description}</p>}
                    <p className="text-xs text-aegis-muted mb-3 font-mono">id: {draft.id}</p>
                    <div className="flex flex-wrap gap-2">
                      {draft.status === "draft" && (
                        <>
                          <ActionButton label="Validate" onClick={() => handleAction("validate", { draftId: draft.id })} color="accent" />
                          <ActionButton label="Submit" onClick={() => handleAction("submit", { draftId: draft.id })} color="blue" />
                          <ActionButton label="Edit Schema" onClick={() => { setEditingDraftId(draft.id); setEditSchema(draft.schema || ""); }} color="muted" />
                        </>
                      )}
                      {draft.status === "under_review" && (
                        <>
                          <ActionButton label="✅ Approve" onClick={() => handleAction("approve", { draftId: draft.id })} color="green" />
                          <ActionButton label="❌ Reject" onClick={() => { const reason = prompt("Rejection reason:"); if (reason) handleAction("reject", { draftId: draft.id, reason }); }} color="red" />
                        </>
                      )}
                      {draft.status === "approved" && (
                        <ActionButton label="🚀 Publish" onClick={() => handleAction("publish", { draftId: draft.id })} color="green" />
                      )}
                      {(draft.status === "rejected" || draft.status === "published") && (
                        <ActionButton label="Archive" onClick={() => handleAction("archive", { draftId: draft.id })} color="muted" />
                      )}
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>

          <div className="bg-aegis-card border border-aegis-border rounded-xl p-6">
            <p className="text-sm font-medium text-aegis-text mb-4">Published Versions ({versions.length})</p>
            {versions.length === 0 ? (
              <p className="text-xs text-aegis-muted">No published versions yet. Publish a draft to see it here.</p>
            ) : (
              <div className="space-y-3">
                {[...versions].reverse().map((v) => (
                  <div key={v.version} className="p-4 bg-aegis-bg rounded-lg border border-aegis-border flex items-center justify-between">
                    <div>
                      <p className="text-sm font-mono font-medium text-aegis-text">{v.name || `Version ${v.version}`}</p>
                      <p className="text-xs text-aegis-muted mt-1">version={v.version}</p>
                    </div>
                    <div className="flex items-center gap-2">
                      <span className="text-xs px-2 py-0.5 rounded-full border bg-aegis-green/20 text-aegis-green border-aegis-green/30">Published</span>
                      <button onClick={() => { if (confirm(`Rollback to version ${v.version}?`)) handleAction("rollback", { version: v.version }); }}
                        className="text-xs px-2 py-1 bg-aegis-amber/20 text-aegis-amber rounded hover:bg-aegis-amber/30 transition-colors">Rollback</button>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      )}

      {editingDraftId && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50" onClick={() => setEditingDraftId(null)}>
          <div className="bg-aegis-card border border-aegis-border rounded-xl p-6 w-full max-w-2xl mx-4 max-h-[80vh] overflow-y-auto" onClick={(e) => e.stopPropagation()}>
            <p className="text-sm font-medium text-aegis-text mb-4">Edit Draft Schema</p>
            <textarea value={editSchema} onChange={(e) => setEditSchema(e.target.value)} rows={15}
              className="w-full bg-aegis-bg border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text font-mono focus:outline-none focus:border-aegis-accent mb-4" />
            <div className="flex gap-3">
              <button onClick={async () => {
                await handleAction("update", { draftId: editingDraftId, schemaJson: editSchema });
                setEditingDraftId(null);
              }} className="px-6 py-2 bg-aegis-accent text-white rounded-lg text-sm font-medium">Save</button>
              <button onClick={() => setEditingDraftId(null)}
                className="px-6 py-2 bg-aegis-card border border-aegis-border text-aegis-muted rounded-lg text-sm font-medium">Cancel</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function ActionButton({ label, onClick, color }: { label: string; onClick: () => void; color: string }) {
  const colors: Record<string, string> = {
    accent: "bg-aegis-accent/20 text-aegis-accent border-aegis-accent/30 hover:bg-aegis-accent/30",
    blue: "bg-aegis-blue/20 text-aegis-blue border-aegis-blue/30 hover:bg-aegis-blue/30",
    green: "bg-aegis-green/20 text-aegis-green border-aegis-green/30 hover:bg-aegis-green/30",
    red: "bg-aegis-red/20 text-aegis-red border-aegis-red/30 hover:bg-aegis-red/30",
    muted: "bg-aegis-muted/20 text-aegis-muted border-aegis-muted/30 hover:bg-aegis-muted/30",
  };
  return <button onClick={onClick}
    className={`px-3 py-1 text-xs rounded-lg border transition-colors ${colors[color] || colors.accent}`}>{label}</button>;
}
