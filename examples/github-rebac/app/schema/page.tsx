"use client";

import { useEffect, useState } from "react";
import dynamic from "next/dynamic";

const MonacoEditor = dynamic(() => import("@monaco-editor/react"), { ssr: false });

export default function SchemaPage() {
  const [schema, setSchema] = useState("");
  const [modifiedSchema, setModifiedSchema] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [result, setResult] = useState<Record<string, unknown> | null>(null);

  useEffect(() => {
    fetch("/api/schema")
      .then((r) => r.json())
      .then((d) => { setSchema(d.schema); setModifiedSchema(d.schema); })
      .catch(() => setError("Failed to load schema"))
      .finally(() => setLoading(false));
  }, []);

  async function handleValidate() {
    setError(""); setResult(null);
    try {
      const res = await fetch("/api/schema", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action: "validate", schema: modifiedSchema }),
      });
      const data = await res.json();
      setResult(data.report ?? data);
      if (data.error) setError(data.error);
    } catch { setError("Validation failed"); }
  }

  async function handleApply() {
    setError(""); setResult(null);
    try {
      const res = await fetch("/api/schema", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action: "apply", schema: modifiedSchema }),
      });
      const data = await res.json();
      setResult(data);
      if (data.error) { setError(data.error); }
      else { setSchema(modifiedSchema); }
    } catch { setError("Apply failed"); }
  }

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold text-aegis-text">Schema Editor</h2>
        <p className="text-aegis-muted text-sm mt-1 font-medium">Edit, validate, and apply ReBAC YAML schema policies live.</p>
      </div>

      {/* Guide Panel */}
      <div className="bg-aegis-card border border-aegis-border rounded-xl p-5 grid grid-cols-1 md:grid-cols-2 gap-4 text-xs">
        <div>
          <h4 className="font-bold text-aegis-text mb-2 flex items-center gap-1.5">
            <span>📖</span> Schema Structure Guide
          </h4>
          <pre className="bg-aegis-bg p-3 border border-aegis-border/60 rounded font-mono text-[11px] leading-relaxed text-aegis-text overflow-auto">
{`types:
  <type-name>: {}          # defines a resource/subject type
    relations:
      <relation>: {}       # who can be related
      <relation>:
        inherit_from:
          - <other>        # role inheritance
    permissions:
      <permission>:
        union_of:
          - <relation>     # union of relation checks
    deny:
      - relations:
          - <banned-rel>   # deny override`}
          </pre>
        </div>
        <div className="flex flex-col justify-between space-y-4">
          <div className="space-y-2">
            <h5 className="font-semibold text-aegis-text">File System Mapping</h5>
            <p className="text-aegis-muted leading-relaxed">
              Applying changes dynamically compiles the policy inside the active GraphEngine memory pool and persists the model to disk.
            </p>
            <div className="text-[11px] font-mono text-aegis-accent bg-aegis-border/20 px-2.5 py-1.5 rounded border border-aegis-border/40 inline-block break-all">
              File Path: .aegis-data/schema.yaml
            </div>
          </div>
          <div className="text-[11px] text-aegis-muted bg-aegis-accent/5 p-3 rounded border border-aegis-accent/15 leading-relaxed">
            <span className="font-bold text-aegis-accent uppercase tracking-wider block mb-1">PRO-TIP</span>
            Validate changes before applying them to catch syntactical errors, inheritance cycles, and breaking schema variations.
          </div>
        </div>
      </div>

      {loading ? (
        <div className="flex items-center justify-center h-64 text-aegis-muted">Loading schema...</div>
      ) : (
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
          <div className="bg-aegis-card border border-aegis-border rounded-xl overflow-hidden shadow-sm">
            <div className="p-3 border-b border-aegis-border flex items-center justify-between">
              <p className="text-xs text-aegis-muted uppercase tracking-wider font-semibold">Schema YAML Editor</p>
              <div className="flex gap-2">
                <button onClick={handleValidate} className="px-3 py-1 text-xs bg-aegis-blue/20 text-aegis-blue border border-aegis-blue/30 rounded hover:bg-aegis-blue/30 transition-colors font-bold">
                  Validate
                </button>
                <button onClick={handleApply} className="px-3 py-1 text-xs bg-aegis-accent/20 text-aegis-accent border border-aegis-accent/30 rounded hover:bg-aegis-accent/30 transition-colors font-bold">
                  Apply & Save
                </button>
              </div>
            </div>
            <MonacoEditor
              height="450px"
              language="yaml"
              theme="vs-dark"
              value={modifiedSchema}
              onChange={(val) => setModifiedSchema(val ?? "")}
              options={{
                minimap: { enabled: false },
                fontSize: 13,
                lineNumbers: "on",
                scrollBeyondLastLine: false,
                automaticLayout: true,
              }}
            />
          </div>

          <div className="space-y-4">
            <div className="bg-aegis-card border border-aegis-border rounded-xl p-6 shadow-sm min-h-[200px]">
              <p className="text-sm font-bold text-aegis-text mb-2">Result Output</p>
              {error && (
                <div className="p-3 bg-aegis-red/10 border border-aegis-red/30 rounded text-xs text-aegis-red font-mono break-all">{error}</div>
              )}
              {result && !error && (
                <pre className="text-xs text-aegis-text font-mono whitespace-pre-wrap overflow-auto max-h-[360px] bg-aegis-bg rounded p-3 border border-aegis-border leading-relaxed">
                  {JSON.stringify(result, null, 2)}
                </pre>
              )}
              {!result && !error && (
                <p className="text-xs text-aegis-muted">Click Validate or Apply to inspect diagnostic results.</p>
              )}
            </div>

            <div className="bg-aegis-card border border-aegis-border rounded-xl p-6 shadow-sm">
              <p className="text-sm font-bold text-aegis-text mb-2">Engine Status</p>
              <p className="text-xs text-aegis-muted">
                {schema === modifiedSchema ? "✓ Schema matches engine and disk file" : "⚠ Editor schema has unsaved changes"}
              </p>
              <p className="text-[11px] text-aegis-muted mt-2 leading-relaxed">
                Changes are <span className="font-bold text-aegis-accent">persistent</span> &mdash; applying a new schema updates the running engine and saves to the local storage folder instantly.
              </p>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
