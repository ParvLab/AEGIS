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
        <p className="text-aegis-muted text-sm mt-1">Edit, validate, and apply YAML schema changes live</p>
      </div>

      {loading ? (
        <div className="flex items-center justify-center h-64 text-aegis-muted">Loading schema...</div>
      ) : (
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
          <div className="bg-aegis-card border border-aegis-border rounded-xl overflow-hidden">
            <div className="p-3 border-b border-aegis-border flex items-center justify-between">
              <p className="text-xs text-aegis-muted uppercase tracking-wider">Schema YAML</p>
              <div className="flex gap-2">
                <button onClick={handleValidate} className="px-3 py-1 text-xs bg-aegis-blue/20 text-aegis-blue border border-aegis-blue/30 rounded hover:bg-aegis-blue/30 transition-colors">
                  Validate
                </button>
                <button onClick={handleApply} className="px-3 py-1 text-xs bg-aegis-accent/20 text-aegis-accent border border-aegis-accent/30 rounded hover:bg-aegis-accent/30 transition-colors">
                  Apply
                </button>
              </div>
            </div>
            <MonacoEditor
              height="500px"
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
            <div className="bg-aegis-card border border-aegis-border rounded-xl p-6">
              <p className="text-sm font-medium text-aegis-text mb-2">Result</p>
              {error && (
                <div className="p-3 bg-aegis-red/10 border border-aegis-red/30 rounded text-sm text-aegis-red">{error}</div>
              )}
              {result && !error && (
                <pre className="text-sm text-aegis-text font-mono whitespace-pre-wrap overflow-auto max-h-96 bg-aegis-bg rounded p-3 border border-aegis-border">
                  {JSON.stringify(result, null, 2)}
                </pre>
              )}
              {!result && !error && (
                <p className="text-xs text-aegis-muted">Click Validate or Apply to see results here.</p>
              )}
            </div>

            <div className="bg-aegis-card border border-aegis-border rounded-xl p-6">
              <p className="text-sm font-medium text-aegis-text mb-2">Engine Status</p>
              <p className="text-xs text-aegis-muted">
                {schema === modifiedSchema ? "✓ Schema matches engine" : "⚠ Schema has unsaved changes"}
              </p>
              <p className="text-xs text-aegis-muted mt-1">
                Changes are NOT persistent — applying a new schema updates the running engine only.
              </p>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
