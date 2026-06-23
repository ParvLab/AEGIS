"use client";

import { useState, useEffect } from "react";

interface BackupRecord {
  filename: string;
  sizeBytes: number;
  createdAt: string;
}

export default function BackupPage() {
  const [backups, setBackups] = useState<BackupRecord[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [success, setSuccess] = useState("");

  // JSON Import State
  const [jsonText, setJsonText] = useState("");
  const [importLoading, setImportLoading] = useState(false);

  async function fetchBackups() {
    setLoading(true);
    try {
      const res = await fetch("/api/backup");
      if (res.ok) {
        const data = await res.json();
        setBackups(data.backups ?? []);
      }
    } catch {
      setError("Failed to fetch backups list");
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    fetchBackups();
  }, []);

  async function handleCreateBackup() {
    setLoading(true);
    setError("");
    setSuccess("");
    try {
      const res = await fetch("/api/backup", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action: "backup" }),
      });
      const data = await res.json();
      if (data.error) {
        setError(data.error);
      } else {
        setSuccess(`WAL-safe hot backup created: "${data.filename}"`);
        fetchBackups();
      }
    } catch {
      setError("Failed to create backup");
    } finally {
      setLoading(false);
    }
  }

  async function handleExportJson() {
    setError("");
    setSuccess("");
    try {
      const res = await fetch("/api/backup", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action: "export-json" }),
      });
      const data = await res.json();
      if (data.error) {
        setError(data.error);
      } else {
        // Trigger browser download of JSON text
        const blob = new Blob([data.json], { type: "application/json" });
        const url = URL.createObjectURL(blob);
        const a = document.createElement("a");
        a.href = url;
        a.download = `aegis-export-${new Date().toISOString().slice(0, 10)}.json`;
        a.click();
        URL.revokeObjectURL(url);
        setSuccess("JSON tuple export downloaded.");
      }
    } catch {
      setError("Failed to export JSON data");
    }
  }

  async function handleImportJson() {
    if (!jsonText.trim()) return;
    setImportLoading(true);
    setError("");
    setSuccess("");
    try {
      const res = await fetch("/api/backup", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action: "import-json", json: jsonText.trim() }),
      });
      const data = await res.json();
      if (data.error) {
        setError(data.error);
      } else {
        setSuccess(`Successfully imported tuples batch! Active revision: ${data.revision}`);
        setJsonText("");
      }
    } catch {
      setError("Failed to import JSON tuples");
    } finally {
      setImportLoading(false);
    }
  }

  async function handleRestore(filename: string) {
    if (!confirm(`WARNING: Restoring will overwrite all current data. Are you sure you want to restore from "${filename}"?`)) {
      return;
    }
    setLoading(true);
    setError("");
    setSuccess("");
    try {
      const res = await fetch("/api/backup", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action: "restore", filename }),
      });
      const data = await res.json();
      if (data.error) {
        setError(data.error);
      } else {
        setSuccess(`Database successfully restored to backup: "${filename}". Engine reset complete.`);
      }
    } catch {
      setError("Failed to restore backup");
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold text-aegis-text">Backup & Restore</h2>
        <p className="text-aegis-muted text-sm mt-1">
          Perform hot database backups, export/import relationship tuples as JSON, and restore snapshot states.
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

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* Actions & Backup Files */}
        <div className="lg:col-span-2 space-y-6">
          {/* Main Controls Card */}
          <div className="bg-aegis-card border border-aegis-border rounded-xl p-6 space-y-4">
            <h4 className="text-lg font-bold text-aegis-text">Snapshot Database Backups</h4>
            <p className="text-xs text-aegis-muted leading-relaxed">
              Create point-in-time WAL-safe hot copies of the active SQLite database store. Backup files are saved locally on the server filesystem and can be downloaded or restored instantly.
            </p>

            <div className="flex gap-3">
              <button
                onClick={handleCreateBackup}
                disabled={loading}
                className="px-4 py-2 bg-aegis-accent text-white rounded-lg hover:opacity-90 font-semibold text-xs transition-opacity disabled:opacity-50"
              >
                {loading ? "Creating..." : "Create Hot Backup"}
              </button>
              <button
                onClick={handleExportJson}
                className="px-4 py-2 border border-aegis-border hover:bg-aegis-border/20 text-aegis-text rounded-lg font-semibold text-xs transition-colors"
              >
                Export Tuples JSON
              </button>
            </div>
          </div>

          {/* Backup Files Table Card */}
          <div className="bg-aegis-card border border-aegis-border rounded-xl p-6">
            <h4 className="text-md font-bold text-aegis-text mb-4">Saved Backup Files</h4>
            {backups.length === 0 ? (
              <p className="text-xs text-aegis-muted">No database backups found on disk.</p>
            ) : (
              <div className="border border-aegis-border rounded-lg overflow-hidden text-xs">
                <table className="w-full text-left border-collapse text-aegis-text">
                  <thead>
                    <tr className="bg-aegis-border/20 border-b border-aegis-border font-bold text-aegis-muted uppercase tracking-wider text-[10px]">
                      <th className="p-3">Filename</th>
                      <th className="p-3">Size</th>
                      <th className="p-3">Created</th>
                      <th className="p-3 text-right">Actions</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-aegis-border/50">
                    {backups.map((b) => (
                      <tr key={b.filename} className="hover:bg-aegis-border/5">
                        <td className="p-3 font-semibold font-mono text-[11px]">{b.filename}</td>
                        <td className="p-3 text-aegis-muted">{(b.sizeBytes / 1024).toFixed(1)} KB</td>
                        <td className="p-3 text-aegis-muted">{new Date(b.createdAt).toLocaleString()}</td>
                        <td className="p-3 text-right space-x-2">
                          <a
                            href={`/api/backup?file=${b.filename}`}
                            className="px-2.5 py-1 bg-aegis-accent/10 hover:bg-aegis-accent text-aegis-accent hover:text-white rounded font-bold text-[10px] transition-all inline-block"
                          >
                            Download
                          </a>
                          <button
                            onClick={() => handleRestore(b.filename)}
                            className="px-2.5 py-1 bg-aegis-green/10 hover:bg-aegis-green text-aegis-green hover:text-white rounded font-bold text-[10px] transition-all"
                          >
                            Restore
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </div>
        </div>

        {/* JSON Import Card */}
        <div className="bg-aegis-card border border-aegis-border rounded-xl p-6 space-y-4">
          <h4 className="text-lg font-bold text-aegis-text">Import Relationships JSON</h4>
          <p className="text-xs text-aegis-muted leading-relaxed">
            Paste a JSON array of relationship tuples to batch import them directly into the current active partition workspace.
          </p>

          <textarea
            placeholder='[\n  { "subject": "user:alice", "relation": "admin", "object": "org:acme" }\n]'
            value={jsonText}
            onChange={(e) => setJsonText(e.target.value)}
            rows={12}
            className="w-full bg-aegis-card border border-aegis-border rounded-lg p-3 text-xs text-aegis-text font-mono focus:outline-none focus:border-aegis-accent"
          />

          <button
            onClick={handleImportJson}
            disabled={importLoading || !jsonText.trim()}
            className="w-full py-2 bg-aegis-accent text-white rounded-lg hover:opacity-90 font-semibold text-sm transition-opacity disabled:opacity-50"
          >
            {importLoading ? "Importing..." : "Import Relationship Tuples"}
          </button>
        </div>
      </div>
    </div>
  );
}
