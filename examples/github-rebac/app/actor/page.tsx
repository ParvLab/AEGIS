"use client";

import { useState, useEffect } from "react";

export default function ActorPage() {
  const [activeActor, setActiveActor] = useState<string | null>(null);
  const [newActor, setNewActor] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [success, setSuccess] = useState("");

  async function fetchActiveActor() {
    setLoading(true);
    try {
      const res = await fetch("/api/actor");
      if (res.ok) {
        const data = await res.json();
        setActiveActor(data.actor);
      }
    } catch {
      setError("Failed to fetch active actor");
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    fetchActiveActor();
  }, []);

  async function handleSetActor(actorVal: string | null) {
    setError("");
    setSuccess("");
    try {
      const res = await fetch("/api/actor", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ actor: actorVal }),
      });
      const data = await res.json();
      if (data.error) {
        setError(data.error);
      } else {
        setActiveActor(data.actor);
        setNewActor("");
        setSuccess(actorVal ? `Actor identity set to "${actorVal}"` : "Actor identity cleared.");
      }
    } catch {
      setError("Failed to update actor identity");
    }
  }

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold text-aegis-text">Actor Identity Configuration</h2>
        <p className="text-aegis-muted text-sm mt-1">
          Set the active actor identity context. Subsequent write operations will log this identity in the audit trail.
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

      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        {/* Active Actor Banner */}
        <div className="bg-aegis-card border border-aegis-border rounded-xl p-6 space-y-6">
          <div>
            <span className="text-xs uppercase tracking-wider text-aegis-muted block">Current Active Identity</span>
            <div className="text-xl font-mono font-semibold text-aegis-text mt-1 bg-aegis-border/10 p-3 rounded-lg border border-aegis-border/40 inline-block min-w-[200px]">
              {activeActor || "anonymous"}
            </div>
          </div>

          {activeActor && (
            <button
              onClick={() => handleSetActor(null)}
              className="px-4 py-2 bg-aegis-red/10 hover:bg-aegis-red text-aegis-red hover:text-white rounded-lg font-semibold text-xs transition-all"
            >
              Clear Actor Identity
            </button>
          )}
        </div>

        {/* Configure Actor Form */}
        <div className="bg-aegis-card border border-aegis-border rounded-xl p-6 space-y-4">
          <h4 className="text-lg font-bold text-aegis-text">Configure Active Actor</h4>
          <p className="text-xs text-aegis-muted leading-relaxed">
            Enter a unique user handle, service principal, or system account name to bind to the current session context.
          </p>

          <div className="space-y-3">
            <div>
              <label className="block text-[10px] text-aegis-muted mb-1 uppercase tracking-wider">Actor Identity Name</label>
              <input
                type="text"
                placeholder="e.g. user:alice, deploy-agent@acme.com"
                value={newActor}
                onChange={(e) => setNewActor(e.target.value)}
                className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
              />
            </div>

            <button
              onClick={() => handleSetActor(newActor.trim())}
              disabled={!newActor.trim()}
              className="px-4 py-2 bg-aegis-accent text-white rounded-lg hover:opacity-90 font-semibold text-xs transition-opacity disabled:opacity-50"
            >
              Bind Identity
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
