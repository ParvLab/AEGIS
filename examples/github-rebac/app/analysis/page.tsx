"use client";

import { useState } from "react";
import { useDiscovery } from "@/lib/useDiscovery";

interface IntegrityResult {
  ok: boolean;
  brokenChainAt?: number;
  details?: string;
}

interface AnalysisReport {
  orphanedTupleCount: number;
  orphanedTuples: string[];
  highAccessSubjects: string[];
  integrityOk: boolean;
  summary: string;
}

interface AccessReviewItem {
  subject: string;
  relation: string;
  object: string;
  allowed: boolean;
  resolvedVia: string;
  durationMs: number;
}

export default function AnalysisPage() {
  const [activeTab, setActiveTab] = useState<"integrity" | "review" | "consistency">("integrity");
  const { subjects, objects, permissions } = useDiscovery();

  // Tab 1: Integrity state
  const [integrityLoading, setIntegrityLoading] = useState(false);
  const [integrityResult, setIntegrityResult] = useState<IntegrityResult | null>(null);
  const [reportResult, setReportResult] = useState<AnalysisReport | null>(null);
  const [reportLoading, setReportLoading] = useState(false);
  const [error, setError] = useState("");

  // Tab 2: Access Review state
  const [reviewType, setReviewType] = useState<"subject" | "resource">("subject");
  const [selectedSubject, setSelectedSubject] = useState("");
  const [selectedResource, setSelectedResource] = useState("");
  const [reviewLoading, setReviewLoading] = useState(false);
  const [reviewResults, setReviewResults] = useState<AccessReviewItem[]>([]);

  // Tab 3: Consistency state
  const [testSubject, setTestSubject] = useState("");
  const [testPermission, setTestPermission] = useState("");
  const [testResource, setTestResource] = useState("");
  const [consistencyMode, setConsistencyMode] = useState("default");
  const [targetRevision, setTargetRevision] = useState("1");
  const [consistencyLoading, setConsistencyLoading] = useState(false);
  const [consistencyResult, setConsistencyResult] = useState<any>(null);

  async function handleVerifyChain() {
    setIntegrityLoading(true);
    setError("");
    try {
      const res = await fetch("/api/analysis", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action: "verify-audit-chain" }),
      });
      const data = await res.json();
      if (data.error) {
        setError(data.error);
      } else {
        setIntegrityResult(data.integrity);
      }
    } catch {
      setError("Verify chain request failed");
    } finally {
      setIntegrityLoading(false);
    }
  }

  async function handleAnalysisReport() {
    setReportLoading(true);
    setError("");
    try {
      const res = await fetch("/api/analysis", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action: "analysis-report" }),
      });
      const data = await res.json();
      if (data.error) {
        setError(data.error);
      } else {
        setReportResult(data.report);
      }
    } catch {
      setError("Generate analysis report request failed");
    } finally {
      setReportLoading(false);
    }
  }

  async function handleRunReview() {
    setReviewLoading(true);
    setError("");
    setReviewResults([]);
    try {
      const body: any = {};
      if (reviewType === "subject") {
        body.action = "access-review-subject";
        body.subject = selectedSubject || subjects[0];
      } else {
        body.action = "access-review-resource";
        body.resource = selectedResource || objects[0];
      }

      const res = await fetch("/api/analysis", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      const data = await res.json();
      if (data.error) {
        setError(data.error);
      } else {
        // The endpoint parses the NAPI JSON array
        setReviewResults(data.review ?? []);
      }
    } catch {
      setError("Run access review failed");
    } finally {
      setReviewLoading(false);
    }
  }

  async function handleRunConsistencyCheck() {
    consistencyLoading && null;
    setConsistencyLoading(true);
    setError("");
    setConsistencyResult(null);
    try {
      const modeString = consistencyMode === "at_revision" ? `at_revision:${targetRevision}` : consistencyMode;
      const res = await fetch("/api/check", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          subject: testSubject || subjects[0],
          permission: testPermission || permissions[0],
          resource: testResource || objects[0],
          consistency: modeString,
        }),
      });
      const data = await res.json();
      if (data.error) {
        setError(data.error);
      } else {
        setConsistencyResult(data);
      }
    } catch {
      setError("Consistency check request failed");
    } finally {
      setConsistencyLoading(false);
    }
  }

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold text-aegis-text">Analysis & Integrity Suite</h2>
        <p className="text-aegis-muted text-sm mt-1">
          Perform cryptographic audit chain verification, subject access reviews, and run consistency explorer tests.
        </p>
      </div>

      {error && (
        <div className="p-4 bg-aegis-red/10 border border-aegis-red/30 rounded-lg text-sm text-aegis-red">
          {error}
        </div>
      )}

      {/* Tabs Menu */}
      <div className="flex border-b border-aegis-border gap-4">
        <button
          onClick={() => setActiveTab("integrity")}
          className={`pb-3 text-sm font-semibold transition-all border-b-2 ${
            activeTab === "integrity"
              ? "border-aegis-accent text-aegis-text"
              : "border-transparent text-aegis-muted hover:text-aegis-text"
          }`}
        >
          🔐 Cryptographic Integrity
        </button>
        <button
          onClick={() => setActiveTab("review")}
          className={`pb-3 text-sm font-semibold transition-all border-b-2 ${
            activeTab === "review"
              ? "border-aegis-accent text-aegis-text"
              : "border-transparent text-aegis-muted hover:text-aegis-text"
          }`}
        >
          🔍 Access Review
        </button>
        <button
          onClick={() => setActiveTab("consistency")}
          className={`pb-3 text-sm font-semibold transition-all border-b-2 ${
            activeTab === "consistency"
              ? "border-aegis-accent text-aegis-text"
              : "border-transparent text-aegis-muted hover:text-aegis-text"
          }`}
        >
          🧭 Consistency Explorer
        </button>
      </div>

      {/* Tab Contents */}
      {activeTab === "integrity" && (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-6 animate-fade-in">
          {/* Cryptographic Chain */}
          <div className="bg-aegis-card border border-aegis-border rounded-xl p-6 space-y-4">
            <div>
              <h4 className="text-lg font-bold text-aegis-text">Audit Chain Verification</h4>
              <p className="text-xs text-aegis-muted mt-0.5">
                Cryptographically verify that the revision history log has not been altered or tampered with.
              </p>
            </div>

            <button
              onClick={handleVerifyChain}
              disabled={integrityLoading}
              className="px-4 py-2 bg-aegis-accent text-white rounded-lg hover:opacity-90 font-semibold text-xs transition-opacity"
            >
              {integrityLoading ? "Verifying..." : "Verify Audit Chain Integrity"}
            </button>

            {integrityResult && (
              <div
                className={`p-4 rounded-lg border flex items-start gap-3 ${
                  integrityResult.ok ? "bg-aegis-green/10 border-aegis-green/30" : "bg-aegis-red/10 border-aegis-red/30"
                }`}
              >
                <span className="text-2xl">{integrityResult.ok ? "✅" : "⚠️"}</span>
                <div>
                  <h5 className={`font-bold text-sm ${integrityResult.ok ? "text-aegis-green" : "text-aegis-red"}`}>
                    {integrityResult.ok ? "LOG CHAIN SECURE" : "INTEGRITY FAULT DETECTED"}
                  </h5>
                  <p className="text-xs text-aegis-muted mt-1 leading-relaxed">
                    {integrityResult.ok
                      ? "All database hashes verify successfully. No tamper attempts found."
                      : `Chain verification failed at revision ${integrityResult.brokenChainAt}. Detail: ${integrityResult.details || "Unspecified chain break."}`}
                  </p>
                </div>
              </div>
            )}
          </div>

          {/* Engine Report */}
          <div className="bg-aegis-card border border-aegis-border rounded-xl p-6 space-y-4">
            <div>
              <h4 className="text-lg font-bold text-aegis-text">Orphaned & High-Access Report</h4>
              <p className="text-xs text-aegis-muted mt-0.5">
                Scan for relationship tuples with subjects not matching any defined schema type, and list high-privilege actors.
              </p>
            </div>

            <button
              onClick={handleAnalysisReport}
              disabled={reportLoading}
              className="px-4 py-2 bg-aegis-accent text-white rounded-lg hover:opacity-90 font-semibold text-xs transition-opacity"
            >
              {reportLoading ? "Analyzing..." : "Generate Analysis Report"}
            </button>

            {reportResult && (
              <div className="space-y-3 pt-2 text-xs text-aegis-text">
                <div className="p-3 bg-aegis-border/20 rounded-lg flex items-center justify-between">
                  <span>Orphaned Relationship Tuples</span>
                  <span className={`font-bold ${reportResult.orphanedTupleCount > 0 ? "text-aegis-red" : "text-aegis-green"}`}>
                    {reportResult.orphanedTupleCount}
                  </span>
                </div>

                {reportResult.orphanedTuples && reportResult.orphanedTuples.length > 0 && (
                  <div className="bg-aegis-border/10 border border-aegis-border rounded-lg p-3 max-h-[120px] overflow-y-auto font-mono text-[10px] space-y-1">
                    {reportResult.orphanedTuples.map((o, idx) => (
                      <div key={idx}>{o}</div>
                    ))}
                  </div>
                )}

                <div className="space-y-1">
                  <span className="text-[10px] text-aegis-muted uppercase tracking-wider block font-semibold mb-1">
                    High Privilege Subjects
                  </span>
                  <div className="space-y-1">
                    {reportResult.highAccessSubjects && reportResult.highAccessSubjects.map((s, idx) => {
                      try {
                        const parsed = JSON.parse(s);
                        return (
                          <div key={idx} className="p-2 bg-aegis-border/15 rounded flex items-center justify-between">
                            <span className="font-mono text-[11px] font-semibold text-aegis-accent">{parsed.subject}</span>
                            <span className="text-[10px] text-aegis-muted">{parsed.score} permissions</span>
                          </div>
                        );
                      } catch {
                        return <div key={idx} className="p-2 bg-aegis-border/15 rounded font-mono">{s}</div>;
                      }
                    })}
                  </div>
                </div>
              </div>
            )}
          </div>
        </div>
      )}

      {activeTab === "review" && (
        <div className="bg-aegis-card border border-aegis-border rounded-xl p-6 space-y-6 animate-fade-in">
          <div>
            <h4 className="text-lg font-bold text-aegis-text">Access Review Matrix</h4>
            <p className="text-xs text-aegis-muted mt-0.5">
              Query every permission relationship path that a subject holds or that is declared on a resource.
            </p>
          </div>

          <div className="flex bg-aegis-border/20 p-1 rounded-lg max-w-xs">
            <button
              onClick={() => setReviewType("subject")}
              className={`flex-1 py-1.5 text-xs font-semibold rounded-md transition-all ${reviewType === "subject" ? "bg-aegis-accent text-white" : "text-aegis-muted hover:text-aegis-text"}`}
            >
              By Subject
            </button>
            <button
              onClick={() => setReviewType("resource")}
              className={`flex-1 py-1.5 text-xs font-semibold rounded-md transition-all ${reviewType === "resource" ? "bg-aegis-accent text-white" : "text-aegis-muted hover:text-aegis-text"}`}
            >
              By Resource
            </button>
          </div>

          <div className="flex flex-col sm:flex-row items-end gap-4">
            {reviewType === "subject" ? (
              <div className="flex-1">
                <label className="block text-[10px] text-aegis-muted mb-1 uppercase tracking-wider">Select Subject</label>
                <select
                  value={selectedSubject}
                  onChange={(e) => setSelectedSubject(e.target.value)}
                  className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
                >
                  {subjects.length === 0 && <option value="">No subjects indexed</option>}
                  {subjects.map(s => <option key={s} value={s}>{s}</option>)}
                </select>
              </div>
            ) : (
              <div className="flex-1">
                <label className="block text-[10px] text-aegis-muted mb-1 uppercase tracking-wider">Select Resource</label>
                <select
                  value={selectedResource}
                  onChange={(e) => setSelectedResource(e.target.value)}
                  className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
                >
                  {objects.length === 0 && <option value="">No resources indexed</option>}
                  {objects.map(r => <option key={r} value={r}>{r}</option>)}
                </select>
              </div>
            )}

            <button
              onClick={handleRunReview}
              disabled={reviewLoading}
              className="px-6 py-2.5 bg-aegis-accent text-white rounded-lg hover:opacity-90 font-semibold text-sm transition-opacity"
            >
              {reviewLoading ? "Reviewing..." : "Generate Review"}
            </button>
          </div>

          {reviewResults.length > 0 && (
            <div className="border border-aegis-border rounded-lg overflow-hidden">
              <table className="w-full text-left border-collapse text-xs text-aegis-text">
                <thead>
                  <tr className="bg-aegis-border/20 border-b border-aegis-border font-bold text-aegis-muted uppercase tracking-wider text-[10px]">
                    <th className="p-3">Subject</th>
                    <th className="p-3">Relation/Permission</th>
                    <th className="p-3">Resource</th>
                    <th className="p-3">Result</th>
                    <th className="p-3">Resolution Path</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-aegis-border/50">
                  {reviewResults.map((item, idx) => (
                    <tr key={idx} className="hover:bg-aegis-border/5">
                      <td className="p-3 font-mono">{item.subject}</td>
                      <td className="p-3 font-semibold">{item.relation}</td>
                      <td className="p-3 font-mono">{item.object}</td>
                      <td className="p-3">
                        <span className={`px-2 py-0.5 rounded font-bold uppercase text-[10px] ${item.allowed ? "bg-aegis-green/20 text-aegis-green" : "bg-aegis-red/20 text-aegis-red"}`}>
                          {item.allowed ? "Allowed" : "Denied"}
                        </span>
                      </td>
                      <td className="p-3 text-aegis-muted font-mono text-[11px]">{item.resolvedVia}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}

      {activeTab === "consistency" && (
        <div className="bg-aegis-card border border-aegis-border rounded-xl p-6 space-y-6 animate-fade-in">
          <div>
            <h4 className="text-lg font-bold text-aegis-text">Consistency Mode Tester</h4>
            <p className="text-xs text-aegis-muted mt-0.5">
              Demonstrate the behavior of different consistency modes: Default, Minimize Latency, Fully Consistent, or specific At Revision checks.
            </p>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-4 gap-4 items-end">
            <div>
              <label className="block text-[10px] text-aegis-muted mb-1 uppercase tracking-wider">Subject</label>
              <select
                value={testSubject}
                onChange={(e) => setTestSubject(e.target.value)}
                className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
              >
                {subjects.map(s => <option key={s} value={s}>{s}</option>)}
              </select>
            </div>
            <div>
              <label className="block text-[10px] text-aegis-muted mb-1 uppercase tracking-wider">Permission</label>
              <select
                value={testPermission}
                onChange={(e) => setTestPermission(e.target.value)}
                className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
              >
                {permissions.map(p => <option key={p} value={p}>{p}</option>)}
              </select>
            </div>
            <div>
              <label className="block text-[10px] text-aegis-muted mb-1 uppercase tracking-wider">Resource</label>
              <select
                value={testResource}
                onChange={(e) => setTestResource(e.target.value)}
                className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
              >
                {objects.map(r => <option key={r} value={r}>{r}</option>)}
              </select>
            </div>
            <div>
              <label className="block text-[10px] text-aegis-muted mb-1 uppercase tracking-wider">Consistency Mode</label>
              <select
                value={consistencyMode}
                onChange={(e) => setConsistencyMode(e.target.value)}
                className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
              >
                <option value="default">Default</option>
                <option value="minimize_latency">Minimize Latency</option>
                <option value="fully_consistent">Fully Consistent</option>
                <option value="at_revision">At Revision</option>
              </select>
            </div>
          </div>

          {consistencyMode === "at_revision" && (
            <div className="max-w-xs animate-fade-in">
              <label className="block text-[10px] text-aegis-muted mb-1 uppercase tracking-wider">Target Revision Number</label>
              <input
                type="number"
                value={targetRevision}
                onChange={(e) => setTargetRevision(e.target.value)}
                className="w-full bg-aegis-card border border-aegis-border rounded-lg px-3 py-2 text-sm text-aegis-text focus:outline-none focus:border-aegis-accent"
              />
            </div>
          )}

          <button
            onClick={handleRunConsistencyCheck}
            disabled={consistencyLoading}
            className="px-6 py-2.5 bg-aegis-accent text-white rounded-lg hover:opacity-90 font-semibold text-sm transition-opacity"
          >
            {consistencyLoading ? "Running Check..." : "Run Consistency Check"}
          </button>

          {consistencyResult && (
            <div className={`p-4 rounded-xl border animate-fade-in ${consistencyResult.allowed ? "bg-aegis-green/10 border-aegis-green/20" : "bg-aegis-red/10 border-aegis-red/20"}`}>
              <div className="flex items-center gap-3">
                <span className="text-2xl">{consistencyResult.allowed ? "✅" : "❌"}</span>
                <div>
                  <h5 className={`font-bold text-base ${consistencyResult.allowed ? "text-aegis-green" : "text-aegis-red"}`}>
                    {consistencyResult.allowed ? "ALLOWED" : "DENIED"}
                  </h5>
                  <p className="text-xs text-aegis-muted mt-0.5">
                    Revision Returned: {consistencyResult.revision} &middot; Duration: {consistencyResult.durationMs?.toFixed(2)}ms
                  </p>
                </div>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
