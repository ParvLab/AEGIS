"use client";

import { useState } from "react";

interface ErrorResponse {
  error: string;
  code: string;
}

export default function ErrorsPage() {
  const [result, setResult] = useState<ErrorResponse | null>(null);
  const [loadingTrigger, setLoadingTrigger] = useState<string | null>(null);

  async function triggerError(triggerName: string) {
    setLoadingTrigger(triggerName);
    setResult(null);
    try {
      const res = await fetch("/api/errors", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ trigger: triggerName }),
      });
      const data = await res.json();
      setResult(data);
    } catch {
      setResult({ error: "Failed to perform request", code: "RequestError" });
    } finally {
      setLoadingTrigger(null);
    }
  }

  const triggers = [
    {
      id: "invalid-subject",
      name: "Invalid Subject Format",
      description: "Write relationship tuple with a subject containing invalid spaces, expecting a validation error.",
      expectedCode: "ValidationError",
    },
    {
      id: "schema-error",
      name: "Schema Parsing Error",
      description: "Reload with syntactically malformed YAML content, expecting a schema parse error.",
      expectedCode: "SchemaParseError / SchemaError",
    },
    {
      id: "rate-limit",
      name: "Rate Limit Exceeded",
      description: "Temporarily set checks/sec = 1 and fire rapid checks, expecting a rate limiter exhaustion error.",
      expectedCode: "RateLimitExceeded",
    },
    {
      id: "future-revision",
      name: "Future Revision Check",
      description: "Perform a check query at revision 999999, expecting a revision not found error.",
      expectedCode: "RevisionNotFound",
    },
    {
      id: "engine-closed",
      name: "Engine Closed Query",
      description: "Initialize a temporary memory engine, close it, and then query it, expecting a closed error.",
      expectedCode: "EngineClosedError",
    },
  ];

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold text-aegis-text">Error Playground</h2>
        <p className="text-aegis-muted text-sm mt-1">
          Trigger and test different failure paths in the AEGIS Engine to verify structured error codes and exception messages.
        </p>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* Triggers list */}
        <div className="lg:col-span-2 space-y-4">
          <div className="bg-aegis-card border border-aegis-border rounded-xl p-6 space-y-4">
            <h3 className="text-lg font-bold text-aegis-text">Error Scenarios</h3>
            <div className="space-y-3">
              {triggers.map((t) => (
                <div
                  key={t.id}
                  className="p-4 bg-aegis-card/40 border border-aegis-border rounded-lg flex flex-col md:flex-row md:items-center justify-between gap-4"
                >
                  <div className="space-y-1">
                    <span className="font-semibold text-aegis-text text-sm">{t.name}</span>
                    <p className="text-xs text-aegis-muted leading-relaxed">{t.description}</p>
                    <span className="text-[10px] text-aegis-accent font-bold uppercase tracking-wider block pt-1">
                      Expected Code: {t.expectedCode}
                    </span>
                  </div>
                  <button
                    onClick={() => triggerError(t.id)}
                    disabled={loadingTrigger !== null}
                    className="px-4 py-2 bg-aegis-accent hover:opacity-90 text-white text-xs font-semibold rounded-lg transition-opacity disabled:opacity-50 self-end md:self-center"
                  >
                    {loadingTrigger === t.id ? "Triggering..." : "Trigger Failure"}
                  </button>
                </div>
              ))}
            </div>
          </div>
        </div>

        {/* Structured Output */}
        <div className="bg-aegis-card border border-aegis-border rounded-xl p-6 h-fit min-h-[250px] space-y-4">
          <h3 className="text-lg font-bold text-aegis-text">Structured Response</h3>
          {result ? (
            <div className="space-y-4 animate-fade-in text-xs">
              <div className="p-3 bg-aegis-red/5 border border-aegis-red/20 rounded-lg">
                <span className="text-[10px] text-aegis-red uppercase font-bold tracking-wider">Error Code</span>
                <div className="font-mono text-base font-bold text-aegis-red mt-1">{result.code}</div>
              </div>

              <div className="p-3 bg-aegis-border/10 border border-aegis-border rounded-lg font-mono">
                <span className="text-[10px] text-aegis-muted uppercase tracking-wider font-bold">Error Message</span>
                <div className="text-aegis-text break-all leading-relaxed whitespace-pre-wrap mt-2">{result.error}</div>
              </div>
            </div>
          ) : (
            <div className="h-full flex items-center justify-center text-aegis-muted text-xs text-center py-12">
              Select and trigger a failure scenario to view the parsed exception response from the AEGIS GraphEngine.
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
