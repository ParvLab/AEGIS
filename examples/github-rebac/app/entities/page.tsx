"use client";

import { useState, useEffect } from "react";
import { useDiscovery } from "@/lib/useDiscovery";

interface EntityItem {
  name: string;
  count: number;
}

export default function EntitiesPage() {
  const { subjectTypes, objectTypes, relations, refresh: refreshDiscovery } = useDiscovery();

  // List states
  const [subjectsList, setSubjectsList] = useState<EntityItem[]>([]);
  const [objectsList, setObjectsList] = useState<EntityItem[]>([]);
  const [search, setSearch] = useState("");
  const [loadingList, setLoadingList] = useState(false);

  // Form states
  const [subType, setSubType] = useState("user");
  const [subName, setSubName] = useState("");
  const [rel, setRel] = useState("member");
  const [objType, setObjType] = useState("repo");
  const [objName, setObjName] = useState("");
  const [condition, setCondition] = useState("");
  const [metaKey, setMetaKey] = useState("");
  const [metaVal, setMetaVal] = useState("");
  const [validUntil, setValidUntil] = useState("");

  // UI state
  const [activeSideEntity, setActiveSideEntity] = useState<string | null>(null);
  const [entityTuples, setEntityTuples] = useState<any[]>([]);
  const [loadingTuples, setLoadingTuples] = useState(false);
  const [error, setError] = useState("");
  const [success, setSuccess] = useState("");

  async function fetchEntities() {
    setLoadingList(true);
    try {
      const res = await fetch("/api/entities");
      if (res.ok) {
        const data = await res.json();
        setSubjectsList(data.subjects ?? []);
        setObjectsList(data.objects ?? []);
      }
    } catch {
      setError("Failed to load entities list");
    } finally {
      setLoadingList(false);
    }
  }

  useEffect(() => {
    fetchEntities();
  }, []);

  async function handleAddTuple() {
    setError("");
    setSuccess("");
    const subject = `${subType}:${subName.trim()}`;
    const object = `${objType}:${objName.trim()}`;

    if (!subName.trim() || !objName.trim()) {
      setError("Please fill in both Subject and Object names");
      return;
    }

    try {
      const metadata = metaKey.trim() && metaVal.trim() ? { [metaKey.trim()]: metaVal.trim() } : undefined;
      const res = await fetch("/api/entities", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          action: "add",
          subject,
          relation: rel,
          object,
          condition: condition.trim() || undefined,
          metadata,
          validUntil: validUntil || undefined,
        }),
      });
      const data = await res.json();
      if (data.error) {
        setError(data.error);
      } else {
        setSuccess(`Tuple added successfully: ${subject} → ${rel} → ${object}`);
        setSubName("");
        setObjName("");
        setCondition("");
        setMetaKey("");
        setMetaVal("");
        setValidUntil("");
        fetchEntities();
        refreshDiscovery();
      }
    } catch {
      setError("Failed to create relationship tuple");
    }
  }

  async function handleRemoveTuple(tSub: string, tRel: string, tObj: string) {
    setError("");
    setSuccess("");
    try {
      const res = await fetch("/api/entities", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action: "remove", subject: tSub, relation: tRel, object: tObj }),
      });
      const data = await res.json();
      if (data.error) {
        setError(data.error);
      } else {
        setSuccess(`Tuple removed.`);
        if (activeSideEntity) {
          handleViewEntity(activeSideEntity);
        }
        fetchEntities();
        refreshDiscovery();
      }
    } catch {
      setError("Failed to delete tuple");
    }
  }

  async function handleCascadeDelete(object: string) {
    if (!confirm(`WARNING: This will delete ALL relationship tuples referencing resource "${object}". Proceed?`)) {
      return;
    }
    setError("");
    setSuccess("");
    try {
      const res = await fetch("/api/entities", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action: "remove-all", object }),
      });
      const data = await res.json();
      if (data.error) {
        setError(data.error);
      } else {
        setSuccess(`Cascade deleted all tuples for resource "${object}".`);
        setActiveSideEntity(null);
        fetchEntities();
        refreshDiscovery();
      }
    } catch {
      setError("Failed to cascade delete object");
    }
  }

  async function handleViewEntity(name: string) {
    setActiveSideEntity(name);
    setLoadingTuples(true);
    try {
      // Find all tuples where this entity is either subject or object
      const res = await fetch("/api/tuples");
      if (res.ok) {
        const data = await res.json();
        const allTuples = data.tuples ?? [];
        const filtered = allTuples.filter((t: any) => t.subject === name || t.object === name);
        setEntityTuples(filtered);
      }
    } catch {
      setError("Failed to load entity details");
    } finally {
      setLoadingTuples(false);
    }
  }

  const filteredSubjects = subjectsList.filter(s => s.name.toLowerCase().includes(search.toLowerCase()));
  const filteredObjects = objectsList.filter(o => o.name.toLowerCase().includes(search.toLowerCase()));

  const previewTuple = `${subType}:${subName || "___"}  →  ${rel}  →  ${objType}:${objName || "___"}`;

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold text-aegis-text">Entity & Relationship Manager</h2>
        <p className="text-aegis-muted text-sm mt-1">
          Form-based interface to build relationship rules, audit subjects/objects, and execute cascade deletes.
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

      {/* Tuple Creation Form */}
      <div className="bg-aegis-card border border-aegis-border rounded-xl p-6 space-y-4">
        <h3 className="text-lg font-bold text-aegis-text">Add Relationship Tuple</h3>
        <div className="grid grid-cols-1 md:grid-cols-3 gap-6 items-end">
          {/* Subject Form */}
          <div className="space-y-2 border border-aegis-border/40 p-4 rounded-lg bg-aegis-border/5">
            <span className="text-[10px] text-aegis-accent font-bold uppercase tracking-wider">Subject (Who?)</span>
            <div className="grid grid-cols-3 gap-2">
              <select
                value={subType}
                onChange={(e) => setSubType(e.target.value)}
                className="col-span-1 bg-aegis-card border border-aegis-border rounded-lg px-2 py-1.5 text-xs text-aegis-text focus:outline-none focus:border-aegis-accent"
              >
                {subjectTypes.map(t => <option key={t} value={t}>{t}</option>)}
                {!subjectTypes.includes("user") && <option value="user">user</option>}
                {!subjectTypes.includes("team") && <option value="team">team</option>}
              </select>
              <input
                type="text"
                placeholder="alice, admin-group"
                value={subName}
                onChange={(e) => setSubName(e.target.value)}
                className="col-span-2 bg-aegis-card border border-aegis-border rounded-lg px-3 py-1.5 text-xs text-aegis-text focus:outline-none focus:border-aegis-accent"
              />
            </div>
          </div>

          {/* Relation Form */}
          <div className="space-y-2 border border-aegis-border/40 p-4 rounded-lg bg-aegis-border/5">
            <span className="text-[10px] text-aegis-accent font-bold uppercase tracking-wider">Relation</span>
            <select
              value={rel}
              onChange={(e) => setRel(e.target.value)}
              className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-1.5 text-xs text-aegis-text focus:outline-none focus:border-aegis-accent"
            >
              {relations.map(r => <option key={r} value={r}>{r}</option>)}
              {!relations.includes("member") && <option value="member">member</option>}
              {!relations.includes("admin") && <option value="admin">admin</option>}
              {!relations.includes("viewer") && <option value="viewer">viewer</option>}
            </select>
          </div>

          {/* Object Form */}
          <div className="space-y-2 border border-aegis-border/40 p-4 rounded-lg bg-aegis-border/5">
            <span className="text-[10px] text-aegis-accent font-bold uppercase tracking-wider">Object (To What?)</span>
            <div className="grid grid-cols-3 gap-2">
              <select
                value={objType}
                onChange={(e) => setObjType(e.target.value)}
                className="col-span-1 bg-aegis-card border border-aegis-border rounded-lg px-2 py-1.5 text-xs text-aegis-text focus:outline-none focus:border-aegis-accent"
              >
                {objectTypes.map(t => <option key={t} value={t}>{t}</option>)}
                {!objectTypes.includes("repo") && <option value="repo">repo</option>}
                {!objectTypes.includes("org") && <option value="org">org</option>}
              </select>
              <input
                type="text"
                placeholder="payment-api, acme"
                value={objName}
                onChange={(e) => setObjName(e.target.value)}
                className="col-span-2 bg-aegis-card border border-aegis-border rounded-lg px-3 py-1.5 text-xs text-aegis-text focus:outline-none focus:border-aegis-accent"
              />
            </div>
          </div>
        </div>

        {/* Expandable Advanced Options */}
        <details className="text-xs text-aegis-muted border border-aegis-border/30 rounded-lg p-3 cursor-pointer">
          <summary className="font-semibold text-aegis-text">Advanced Tuple Parameters (ABAC Conditions, Meta, TTL)</summary>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-4 pt-3 cursor-default">
            <div>
              <label className="block text-[10px] mb-1 uppercase tracking-wider">ABAC Condition</label>
              <input
                type="text"
                placeholder="e.g. request.ip == '10.0.0.1'"
                value={condition}
                onChange={(e) => setCondition(e.target.value)}
                className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-1.5 text-xs text-aegis-text focus:outline-none focus:border-aegis-accent"
              />
            </div>
            <div>
              <label className="block text-[10px] mb-1 uppercase tracking-wider">Metadata (Key/Value)</label>
              <div className="flex gap-2">
                <input
                  type="text"
                  placeholder="Key"
                  value={metaKey}
                  onChange={(e) => setMetaKey(e.target.value)}
                  className="w-1/2 bg-aegis-card border border-aegis-border rounded-lg px-3 py-1.5 text-xs text-aegis-text focus:outline-none focus:border-aegis-accent"
                />
                <input
                  type="text"
                  placeholder="Value"
                  value={metaVal}
                  onChange={(e) => setMetaVal(e.target.value)}
                  className="w-1/2 bg-aegis-card border border-aegis-border rounded-lg px-3 py-1.5 text-xs text-aegis-text focus:outline-none focus:border-aegis-accent"
                />
              </div>
            </div>
            <div>
              <label className="block text-[10px] mb-1 uppercase tracking-wider">Expires (Valid Until)</label>
              <input
                type="datetime-local"
                value={validUntil}
                onChange={(e) => setValidUntil(e.target.value)}
                className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-1.5 text-xs text-aegis-text focus:outline-none focus:border-aegis-accent"
              />
            </div>
          </div>
        </details>

        {/* Preview and Submit */}
        <div className="pt-4 border-t border-aegis-border flex items-center justify-between gap-4">
          <div className="font-mono text-xs text-aegis-muted">
            Preview: <span className="font-bold text-aegis-text bg-aegis-border/10 px-2 py-1 rounded">{previewTuple}</span>
          </div>
          <button
            onClick={handleAddTuple}
            className="px-6 py-2.5 bg-aegis-accent text-white rounded-lg hover:opacity-90 font-semibold text-sm transition-opacity"
          >
            Add Tuple Relationship
          </button>
        </div>
      </div>

      {/* Main Browse Panel */}
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* Entities Browse */}
        <div className="lg:col-span-2 bg-aegis-card border border-aegis-border rounded-xl p-6 space-y-4">
          <div className="flex items-center justify-between gap-4 flex-wrap">
            <h3 className="text-lg font-bold text-aegis-text">Entity Directory</h3>
            <input
              type="text"
              placeholder="Search subjects or resources..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              className="bg-aegis-card border border-aegis-border rounded-lg px-3 py-1.5 text-xs text-aegis-text focus:outline-none focus:border-aegis-accent max-w-xs"
            />
          </div>

          <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
            {/* Subjects Column */}
            <div>
              <span className="text-[10px] text-aegis-muted uppercase tracking-wider font-semibold block mb-2">
                Active Subjects ({filteredSubjects.length})
              </span>
              <div className="space-y-1 max-h-[380px] overflow-y-auto pr-1">
                {filteredSubjects.map((s) => (
                  <div
                    key={s.name}
                    onClick={() => handleViewEntity(s.name)}
                    className="p-2.5 bg-aegis-card/50 border border-aegis-border rounded-lg flex items-center justify-between cursor-pointer hover:border-aegis-accent/40"
                  >
                    <span className="font-mono text-xs text-aegis-text">{s.name}</span>
                    <span className="text-[10px] text-aegis-muted bg-aegis-border/20 px-1.5 py-0.5 rounded">
                      {s.count} tuples
                    </span>
                  </div>
                ))}
              </div>
            </div>

            {/* Objects Column */}
            <div>
              <span className="text-[10px] text-aegis-muted uppercase tracking-wider font-semibold block mb-2">
                Active Resources ({filteredObjects.length})
              </span>
              <div className="space-y-1 max-h-[380px] overflow-y-auto pr-1">
                {filteredObjects.map((o) => (
                  <div
                    key={o.name}
                    onClick={() => handleViewEntity(o.name)}
                    className="p-2.5 bg-aegis-card/50 border border-aegis-border rounded-lg flex items-center justify-between cursor-pointer hover:border-aegis-accent/40"
                  >
                    <span className="font-mono text-xs text-aegis-text">{o.name}</span>
                    <span className="text-[10px] text-aegis-muted bg-aegis-border/20 px-1.5 py-0.5 rounded">
                      {o.count} tuples
                    </span>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </div>

        {/* Side Detail Panel */}
        <div className="bg-aegis-card border border-aegis-border rounded-xl p-6 h-fit min-h-[300px]">
          {activeSideEntity ? (
            <div className="space-y-4">
              <div className="border-b border-aegis-border pb-3 flex items-start justify-between gap-3">
                <div>
                  <span className="text-[10px] uppercase font-bold text-aegis-muted">Selected Entity</span>
                  <h4 className="font-mono text-sm font-semibold text-aegis-text break-all mt-1">{activeSideEntity}</h4>
                </div>
                {activeSideEntity.includes(":") && !activeSideEntity.startsWith("user:") && (
                  <button
                    onClick={() => handleCascadeDelete(activeSideEntity)}
                    className="px-2 py-1 bg-aegis-red/10 text-aegis-red hover:bg-aegis-red hover:text-white rounded text-[10px] font-bold transition-all"
                  >
                    Cascade Delete
                  </button>
                )}
              </div>

              <div>
                <span className="text-[10px] text-aegis-muted uppercase tracking-wider block font-semibold mb-2">
                  Direct Relationships
                </span>
                {loadingTuples ? (
                  <p className="text-xs text-aegis-muted">Loading relationship path...</p>
                ) : entityTuples.length === 0 ? (
                  <p className="text-xs text-aegis-muted">No direct tuples found.</p>
                ) : (
                  <div className="space-y-2 max-h-[320px] overflow-y-auto pr-1">
                    {entityTuples.map((t, idx) => (
                      <div key={idx} className="p-2 bg-aegis-border/10 border border-aegis-border/30 rounded text-[11px] space-y-1">
                        <div className="flex items-center justify-between">
                          <span className="font-semibold text-aegis-accent">{t.relation}</span>
                          <button
                            onClick={() => handleRemoveTuple(t.subject, t.relation, t.object)}
                            className="text-aegis-red hover:underline text-[10px]"
                          >
                            Delete
                          </button>
                        </div>
                        <div className="text-[10px] text-aegis-muted font-mono break-all">
                          {t.subject === activeSideEntity ? `To: ${t.object}` : `From: ${t.subject}`}
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            </div>
          ) : (
            <div className="h-full flex items-center justify-center text-aegis-muted text-xs text-center py-12">
              Select an entity from the directory to inspect its active relationship paths and perform modifications.
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
