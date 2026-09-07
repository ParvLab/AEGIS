"use client";

import { useState, useEffect, useRef } from "react";

interface Subscription {
  subscriptionId: string;
  ageSeconds: number;
}

interface WatchEvent {
  eventType: string;
  subject: string;
  relation: string;
  object: string;
  revision: number;
  timestamp: string;
}

export default function EventsPage() {
  const [mode, setMode] = useState<"watch" | "subscribe">("watch");

  // Watch filter inputs
  const [subType, setSubType] = useState("");
  const [relation, setRelation] = useState("");
  const [objType, setObjType] = useState("");

  // Subscribe inputs
  const [eventTypes, setEventTypes] = useState<string>("TupleWritten,TupleDeleted");

  const [activeSubs, setActiveSubs] = useState<Subscription[]>([]);
  const [selectedSubId, setSelectedSubId] = useState<string>("");
  const [eventsFeed, setEventsFeed] = useState<WatchEvent[]>([]);
  const [autoPoll, setAutoPoll] = useState(true);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  const pollIntervalRef = useRef<NodeJS.Timeout | null>(null);

  async function fetchSubscriptions() {
    try {
      const res = await fetch("/api/events");
      if (res.ok) {
        const data = await res.json();
        setActiveSubs(data.subscriptions ?? []);
        if (data.subscriptions && data.subscriptions.length > 0 && !selectedSubId) {
          setSelectedSubId(data.subscriptions[0].subscriptionId);
        }
      }
    } catch {
      setError("Failed to fetch subscriptions");
    }
  }

  useEffect(() => {
    fetchSubscriptions();
    return () => {
      if (pollIntervalRef.current) clearInterval(pollIntervalRef.current);
    };
  }, []);

  // Poll runner
  useEffect(() => {
    if (pollIntervalRef.current) {
      clearInterval(pollIntervalRef.current);
      pollIntervalRef.current = null;
    }

    if (autoPoll && selectedSubId) {
      pollIntervalRef.current = setInterval(async () => {
        try {
          const res = await fetch("/api/events", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ action: "poll", subscriptionId: selectedSubId }),
          });
          if (res.ok) {
            const data = await res.json();
            if (data.event) {
              setEventsFeed(prev => [data.event, ...prev].slice(0, 100)); // Cap at 100
            }
          }
        } catch {
          // Suppress errors during background poll
        }
      }, 500);
    }

    return () => {
      if (pollIntervalRef.current) clearInterval(pollIntervalRef.current);
    };
  }, [autoPoll, selectedSubId]);

  async function handleStartStream() {
    setLoading(true);
    setError("");
    try {
      const body: any = { action: mode };
      if (mode === "watch") {
        body.subjectType = subType.trim() || undefined;
        body.relation = relation.trim() || undefined;
        body.objectType = objType.trim() || undefined;
      } else {
        body.eventTypes = eventTypes.split(",").map(e => e.trim()).filter(Boolean);
      }

      const res = await fetch("/api/events", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      const data = await res.json();
      if (data.error) {
        setError(data.error);
      } else {
        setSelectedSubId(data.subscriptionId);
        fetchSubscriptions();
      }
    } catch {
      setError("Failed to create subscription");
    } finally {
      setLoading(false);
    }
  }

  async function handleStopStream(id: string) {
    try {
      await fetch("/api/events", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action: "unsubscribe", subscriptionId: id }),
      });
      if (selectedSubId === id) {
        setSelectedSubId("");
      }
      fetchSubscriptions();
    } catch {
      setError("Failed to stop subscription");
    }
  }

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold text-aegis-text">Live Events Stream</h2>
        <p className="text-aegis-muted text-sm mt-1">
          Subscribe to low-level authorization events, write updates, and changes across all partitions in real-time.
        </p>
      </div>

      {error && (
        <div className="p-4 bg-aegis-red/10 border border-aegis-red/30 rounded-lg text-sm text-aegis-red">
          {error}
        </div>
      )}

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* Subscription Panel */}
        <div className="space-y-6 lg:col-span-1">
          <div className="bg-aegis-card border border-aegis-border rounded-xl p-6 space-y-4">
            <h4 className="text-lg font-bold text-aegis-text">New Subscription</h4>
            <div className="flex bg-aegis-border/20 p-1 rounded-lg">
              <button
                onClick={() => setMode("watch")}
                className={`flex-1 py-1.5 text-xs font-semibold rounded-md transition-all ${mode === "watch" ? "bg-aegis-accent text-white shadow-sm" : "text-aegis-muted hover:text-aegis-text"}`}
              >
                Watch Filter
              </button>
              <button
                onClick={() => setMode("subscribe")}
                className={`flex-1 py-1.5 text-xs font-semibold rounded-md transition-all ${mode === "subscribe" ? "bg-aegis-accent text-white shadow-sm" : "text-aegis-muted hover:text-aegis-text"}`}
              >
                Event Subscribe
              </button>
            </div>

            {mode === "watch" ? (
              <div className="space-y-3">
                <div>
                  <label className="block text-[10px] text-aegis-muted mb-1 uppercase tracking-wider">Subject Type Filter</label>
                  <input
                    type="text"
                    placeholder="e.g. user"
                    value={subType}
                    onChange={(e) => setSubType(e.target.value)}
                    className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
                  />
                </div>
                <div>
                  <label className="block text-[10px] text-aegis-muted mb-1 uppercase tracking-wider">Relation Filter</label>
                  <input
                    type="text"
                    placeholder="e.g. admin"
                    value={relation}
                    onChange={(e) => setRelation(e.target.value)}
                    className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
                  />
                </div>
                <div>
                  <label className="block text-[10px] text-aegis-muted mb-1 uppercase tracking-wider">Object Type Filter</label>
                  <input
                    type="text"
                    placeholder="e.g. repo"
                    value={objType}
                    onChange={(e) => setObjType(e.target.value)}
                    className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
                  />
                </div>
              </div>
            ) : (
              <div className="space-y-3">
                <div>
                  <label className="block text-[10px] text-aegis-muted mb-1 uppercase tracking-wider">Event Types (comma separated)</label>
                  <input
                    type="text"
                    placeholder="TupleWritten, TupleDeleted"
                    value={eventTypes}
                    onChange={(e) => setEventTypes(e.target.value)}
                    className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
                  />
                  <p className="text-[10px] text-aegis-muted mt-1">Available: TupleWritten, TupleDeleted</p>
                </div>
              </div>
            )}

            <button
              onClick={handleStartStream}
              disabled={loading}
              className="w-full py-2 bg-aegis-accent text-white rounded-lg hover:opacity-90 font-semibold text-sm transition-opacity disabled:opacity-50"
            >
              {loading ? "Initializing..." : "Start Stream"}
            </button>
          </div>

          {/* Active Subscriptions */}
          <div className="bg-aegis-card border border-aegis-border rounded-xl p-6">
            <h4 className="text-md font-bold text-aegis-text mb-4">Active Subscriptions ({activeSubs.length})</h4>
            {activeSubs.length === 0 ? (
              <p className="text-xs text-aegis-muted">No active subscriptions. Create one above to start listening.</p>
            ) : (
              <div className="space-y-2">
                {activeSubs.map((sub) => (
                  <div
                    key={sub.subscriptionId}
                    onClick={() => setSelectedSubId(sub.subscriptionId)}
                    className={`p-3 rounded-lg border cursor-pointer transition-all flex items-center justify-between gap-2 ${selectedSubId === sub.subscriptionId ? "border-aegis-accent bg-aegis-accent/5" : "border-aegis-border hover:border-aegis-accent/30"}`}
                  >
                    <div>
                      <div className="font-semibold text-xs text-aegis-text">{sub.subscriptionId}</div>
                      <span className="text-[10px] text-aegis-muted">age: {sub.ageSeconds}s</span>
                    </div>
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        handleStopStream(sub.subscriptionId);
                      }}
                      className="px-2 py-1 bg-aegis-red/10 text-aegis-red hover:bg-aegis-red hover:text-white rounded text-[10px] font-bold transition-all"
                    >
                      Stop
                    </button>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>

        {/* Live Feed Terminal */}
        <div className="lg:col-span-2 space-y-4">
          <div className="bg-aegis-card border border-aegis-border rounded-xl p-6 flex flex-col h-[520px]">
            <div className="flex items-center justify-between mb-4 border-b border-aegis-border pb-3">
              <div>
                <h4 className="text-lg font-bold text-aegis-text">Live Event Feed</h4>
                <p className="text-xs text-aegis-muted mt-0.5">
                  Listening to: <span className="font-semibold text-aegis-text">{selectedSubId || "None"}</span>
                </p>
              </div>
              <div className="flex items-center gap-4">
                <button
                  onClick={() => setEventsFeed([])}
                  className="text-xs text-aegis-muted hover:text-aegis-text transition-colors"
                >
                  Clear Feed
                </button>
                <label className="flex items-center gap-2 text-xs text-aegis-text cursor-pointer">
                  <input
                    type="checkbox"
                    checked={autoPoll}
                    onChange={(e) => setAutoPoll(e.target.checked)}
                    className="rounded border-aegis-border text-aegis-accent focus:ring-aegis-accent"
                  />
                  Auto-Poll (500ms)
                </label>
              </div>
            </div>

            <div className="flex-1 overflow-y-auto font-mono text-xs space-y-2 pr-1">
              {eventsFeed.length === 0 ? (
                <div className="h-full flex items-center justify-center text-aegis-muted">
                  No events received yet. Make writes or edits to trigger updates.
                </div>
              ) : (
                eventsFeed.map((evt, idx) => (
                  <div
                    key={idx}
                    className={`p-2.5 rounded border border-aegis-border/40 font-mono text-[11px] leading-relaxed animate-fade-in ${
                      evt.eventType === "TupleWritten" ? "bg-aegis-green/5 border-l-2 border-l-aegis-green" : "bg-aegis-red/5 border-l-2 border-l-aegis-red"
                    }`}
                  >
                    <div className="flex items-center justify-between text-aegis-muted mb-1 text-[10px]">
                      <span className={`font-bold ${evt.eventType === "TupleWritten" ? "text-aegis-green" : "text-aegis-red"}`}>
                        [{evt.eventType}]
                      </span>
                      <span>{new Date(evt.timestamp).toLocaleTimeString()}</span>
                    </div>
                    <div className="text-aegis-text">
                      <span className="font-bold text-aegis-accent">{evt.subject}</span>
                      <span className="mx-1 text-aegis-muted">→</span>
                      <span className="underline">{evt.relation}</span>
                      <span className="mx-1 text-aegis-muted">→</span>
                      <span className="font-bold text-aegis-accent">{evt.object}</span>
                    </div>
                    <div className="text-[10px] text-aegis-muted mt-1">
                      revision={evt.revision}
                    </div>
                  </div>
                ))
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
