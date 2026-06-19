import type { AegisConfig, Tuple, WorkerRequest, WorkerResponse } from "./types";

export type { AegisConfig, Tuple, CheckResult, AuditEntry, ExplainV2Response, WhoCanAccessResponse, AccessDiffResponse, PolicyVersion, PolicyDraft, ValidationReport, PublishResult, AnalysisSchedule, AnalysisRun, EnforcementHistoryConfig, EnforcementTrends, EnforcementEvent } from "./types";

type PendingRequest = {
  resolve: (value: any) => void;
  reject: (reason: any) => void;
  timer?: ReturnType<typeof setTimeout>;
};

export class AegisEngine {
  private worker: Worker | null = null;
  private pending = new Map<string, PendingRequest>();
  private counter = 0;
  private useWorker: boolean;
  private _partitionId: string;

  static async create(schema: string, config: AegisConfig = {}): Promise<AegisEngine> {
    const engine = new AegisEngine(config);
    await engine.init(schema);
    return engine;
  }

  constructor(config: AegisConfig = {}) {
    this.useWorker = config.useWorker !== false;
    this._partitionId = config.partitionId || "default";

    if (this.useWorker) {
      const workerUrl = config.workerUrl || new URL("./worker.ts", import.meta.url).href;
      this.worker = new Worker(workerUrl, { type: "module" });

      this.worker.onmessage = (e: MessageEvent<WorkerResponse>) => {
        const msg = e.data;
        const pending = this.pending.get(msg.id);
        if (!pending) return;
        clearTimeout(pending.timer);
        this.pending.delete(msg.id);

        if ("error" in msg && msg.error) {
          pending.reject(new Error(msg.error));
        } else {
          pending.resolve(msg);
        }
      };

      this.worker.onerror = (err) => {
        console.error("Aegis worker error:", err);
      };
    }
  }

  async init(schemaJson: string, _dbName?: string): Promise<void> {
    if (this.useWorker && this.worker) {
      return this.send({ type: "init", id: this.nextId(), schema: schemaJson, dbName: _dbName });
    }
    const wasm = await import("../rust/pkg/aegis_browser.js");
    await wasm.default();
    wasm.init_async(schemaJson);
  }

  async check(subject: string, permission: string, resource: string): Promise<boolean> {
    if (this.useWorker && this.worker) {
      const res = await this.send({ type: "check", id: this.nextId(), subject, permission, resource });
      return (res as any).allowed;
    }
    const wasm = await import("../rust/pkg/aegis_browser.js");
    return wasm.check(subject, permission, resource);
  }

  async write(subject: string, relation: string, resource: string): Promise<string> {
    if (this.useWorker && this.worker) {
      const res = await this.send({ type: "write", id: this.nextId(), subject, relation, resource });
      return (res as any).revision;
    }
    const wasm = await import("../rust/pkg/aegis_browser.js");
    return wasm.write_relation(subject, relation, resource);
  }

  async delete(subject: string, relation: string, resource: string): Promise<string> {
    if (this.useWorker && this.worker) {
      const res = await this.send({ type: "delete", id: this.nextId(), subject, relation, resource });
      return (res as any).revision;
    }
    const wasm = await import("../rust/pkg/aegis_browser.js");
    return wasm.delete_relation(subject, relation, resource);
  }

  async listByObject(resource: string): Promise<Tuple[]> {
    if (this.useWorker && this.worker) {
      const res = await this.send({ type: "listByObject", id: this.nextId(), resource });
      return (res as any).tuples;
    }
    const wasm = await import("../rust/pkg/aegis_browser.js");
    return JSON.parse(wasm.list_by_object(resource));
  }

  async listBySubject(subject: string): Promise<Tuple[]> {
    if (this.useWorker && this.worker) {
      const res = await this.send({ type: "listBySubject", id: this.nextId(), subject });
      return (res as any).tuples;
    }
    const wasm = await import("../rust/pkg/aegis_browser.js");
    return JSON.parse(wasm.list_by_subject(subject));
  }

  async exportToJson(): Promise<string> {
    if (this.useWorker && this.worker) {
      const res = await this.send({ type: "export", id: this.nextId() });
      return (res as any).json;
    }
    const wasm = await import("../rust/pkg/aegis_browser.js");
    return wasm.export_json();
  }

  async importFromJson(json: string): Promise<void> {
    if (this.useWorker && this.worker) {
      await this.send({ type: "import", id: this.nextId(), json });
      return;
    }
    const wasm = await import("../rust/pkg/aegis_browser.js");
    wasm.import_json(json);
  }

  async partition(partitionId: string): Promise<void> {
    this._partitionId = partitionId;
    if (this.useWorker && this.worker) {
      await this.send({ type: "setPartition", id: this.nextId(), partitionId });
      return;
    }
    const wasm = await import("../rust/pkg/aegis_browser.js");
    wasm.set_partition(partitionId);
  }

  async activePartition(): Promise<string> {
    if (this.useWorker && this.worker) {
      const res = await this.send({ type: "activePartition", id: this.nextId() });
      return (res as any).partitionId;
    }
    const wasm = await import("../rust/pkg/aegis_browser.js");
    return wasm.active_partition();
  }

  async explainV2(subject: string, permission: string, resource: string, consistency?: string): Promise<string> {
    if (this.useWorker && this.worker) {
      const res = await this.send({ type: "explainV2", id: this.nextId(), subject, permission, resource, consistency });
      return (res as any).result;
    }
    // wasm.pkg type stubs are stale; methods exist at runtime
    const m = await import("../rust/pkg/aegis_browser.js") as any;
    return m.explain_v2(subject, permission, resource, consistency || null);
  }

  async whoCanAccess(permission: string, resource: string, pageOffset?: number, pageLimit?: number, includePaths?: boolean): Promise<string> {
    if (this.useWorker && this.worker) {
      const res = await this.send({ type: "whoCanAccess", id: this.nextId(), permission, resource, pageOffset, pageLimit, includePaths });
      return (res as any).result;
    }
    const m = await import("../rust/pkg/aegis_browser.js") as any;
    return m.who_can_access(permission, resource, pageOffset || 0, pageLimit || 100, includePaths || false);
  }

  async accessDiff(schemaBefore: string, schemaAfter: string, maxChecks?: number): Promise<string> {
    if (this.useWorker && this.worker) {
      const res = await this.send({ type: "accessDiff", id: this.nextId(), schemaBefore, schemaAfter, maxChecks });
      return (res as any).result;
    }
    const m = await import("../rust/pkg/aegis_browser.js") as any;
    return m.access_diff(schemaBefore, schemaAfter, maxChecks ?? null);
  }

  async listPolicyVersions(): Promise<string> {
    if (this.useWorker && this.worker) {
      const res = await this.send({ type: "listPolicyVersions", id: this.nextId() });
      return (res as any).result;
    }
    const m = await import("../rust/pkg/aegis_browser.js") as any;
    return m.list_policy_versions();
  }

  async rollbackPolicy(version: number): Promise<void> {
    if (this.useWorker && this.worker) {
      await this.send({ type: "rollbackPolicy", id: this.nextId(), version });
      return;
    }
    const m = await import("../rust/pkg/aegis_browser.js") as any;
    m.rollback_policy(version);
  }

  // ── V7 Policy Lifecycle ──

  async createPolicyDraft(name: string, description: string): Promise<string> {
    if (this.useWorker && this.worker) {
      const res = await this.send({ type: "createPolicyDraft", id: this.nextId(), name, description });
      return (res as any).result;
    }
    const m = await import("../rust/pkg/aegis_browser.js") as any;
    return m.create_policy_draft?.(name, description) ?? JSON.stringify({ error: "not available" });
  }

  async updatePolicyDraft(draftId: string, schemaJson: string): Promise<string> {
    if (this.useWorker && this.worker) {
      const res = await this.send({ type: "updatePolicyDraft", id: this.nextId(), draftId, schemaJson });
      return (res as any).result;
    }
    const m = await import("../rust/pkg/aegis_browser.js") as any;
    return m.update_policy_draft?.(draftId, schemaJson) ?? JSON.stringify({ error: "not available" });
  }

  async validatePolicyDraft(draftId: string): Promise<string> {
    if (this.useWorker && this.worker) {
      const res = await this.send({ type: "validatePolicyDraft", id: this.nextId(), draftId });
      return (res as any).result;
    }
    const m = await import("../rust/pkg/aegis_browser.js") as any;
    return m.validate_policy_draft?.(draftId) ?? JSON.stringify({ error: "not available" });
  }

  async submitPolicyDraftForReview(draftId: string): Promise<string> {
    if (this.useWorker && this.worker) {
      const res = await this.send({ type: "submitPolicyDraftForReview", id: this.nextId(), draftId });
      return (res as any).result;
    }
    const m = await import("../rust/pkg/aegis_browser.js") as any;
    return m.submit_policy_draft_for_review?.(draftId) ?? JSON.stringify({ error: "not available" });
  }

  async approvePolicyDraft(draftId: string): Promise<string> {
    if (this.useWorker && this.worker) {
      const res = await this.send({ type: "approvePolicyDraft", id: this.nextId(), draftId });
      return (res as any).result;
    }
    const m = await import("../rust/pkg/aegis_browser.js") as any;
    return m.approve_policy_draft?.(draftId) ?? JSON.stringify({ error: "not available" });
  }

  async rejectPolicyDraft(draftId: string, rejectionReason: string): Promise<string> {
    if (this.useWorker && this.worker) {
      const res = await this.send({ type: "rejectPolicyDraft", id: this.nextId(), draftId, rejectionReason });
      return (res as any).result;
    }
    const m = await import("../rust/pkg/aegis_browser.js") as any;
    return m.reject_policy_draft?.(draftId, rejectionReason) ?? JSON.stringify({ error: "not available" });
  }

  async publishPolicyDraft(draftId: string): Promise<string> {
    if (this.useWorker && this.worker) {
      const res = await this.send({ type: "publishPolicyDraft", id: this.nextId(), draftId });
      return (res as any).result;
    }
    const m = await import("../rust/pkg/aegis_browser.js") as any;
    return m.publish_policy_draft?.(draftId) ?? JSON.stringify({ error: "not available" });
  }

  async archivePolicyDraft(draftId: string): Promise<string> {
    if (this.useWorker && this.worker) {
      const res = await this.send({ type: "archivePolicyDraft", id: this.nextId(), draftId });
      return (res as any).result;
    }
    const m = await import("../rust/pkg/aegis_browser.js") as any;
    return m.archive_policy_draft?.(draftId) ?? JSON.stringify({ error: "not available" });
  }

  async listPolicyDrafts(filterStatus?: string): Promise<string> {
    if (this.useWorker && this.worker) {
      const res = await this.send({ type: "listPolicyDrafts", id: this.nextId(), filterStatus });
      return (res as any).result;
    }
    const m = await import("../rust/pkg/aegis_browser.js") as any;
    return m.list_policy_drafts?.(filterStatus ?? null) ?? JSON.stringify({ error: "not available" });
  }

  // ── V7 Scheduler ──

  async createAnalysisSchedule(name: string, intervalSeconds: number, queriesJson: string, compareSchemaJson?: string): Promise<string> {
    if (this.useWorker && this.worker) {
      const configJson = JSON.stringify({ name, intervalSeconds, queriesJson, compareSchemaJson });
      const res = await this.send({ type: "createAnalysisSchedule", id: this.nextId(), configJson });
      return (res as any).result;
    }
    const m = await import("../rust/pkg/aegis_browser.js") as any;
    return m.create_analysis_schedule?.(name, intervalSeconds, queriesJson, compareSchemaJson ?? null) ?? JSON.stringify({ error: "not available" });
  }

  async listAnalysisSchedules(): Promise<string> {
    if (this.useWorker && this.worker) {
      const res = await this.send({ type: "listAnalysisSchedules", id: this.nextId() });
      return (res as any).result;
    }
    const m = await import("../rust/pkg/aegis_browser.js") as any;
    return m.list_analysis_schedules?.() ?? JSON.stringify({ error: "not available" });
  }

  async deleteAnalysisSchedule(scheduleId: string): Promise<boolean> {
    if (this.useWorker && this.worker) {
      const res = await this.send({ type: "deleteAnalysisSchedule", id: this.nextId(), scheduleId });
      return (res as any).ok;
    }
    const m = await import("../rust/pkg/aegis_browser.js") as any;
    return m.delete_analysis_schedule?.(scheduleId) ?? false;
  }

  async runAnalysisNow(scheduleId?: string): Promise<string> {
    if (this.useWorker && this.worker) {
      const res = await this.send({ type: "runAnalysisNow", id: this.nextId(), scheduleId });
      return (res as any).result;
    }
    const m = await import("../rust/pkg/aegis_browser.js") as any;
    return m.run_analysis_now?.(scheduleId ?? null) ?? JSON.stringify({ error: "not available" });
  }

  async getAnalysisRuns(limit?: number): Promise<string> {
    if (this.useWorker && this.worker) {
      const res = await this.send({ type: "getAnalysisRuns", id: this.nextId(), limit });
      return (res as any).result;
    }
    const m = await import("../rust/pkg/aegis_browser.js") as any;
    return m.get_analysis_runs?.(limit ?? 100) ?? JSON.stringify({ error: "not available" });
  }

  // ── V7 Enforcement History ──

  async setEnforcementHistoryConfig(configJson: string): Promise<void> {
    if (this.useWorker && this.worker) {
      await this.send({ type: "setEnforcementHistoryConfig", id: this.nextId(), configJson });
      return;
    }
    const m = await import("../rust/pkg/aegis_browser.js") as any;
    m.set_enforcement_history_config?.(configJson);
  }

  async getEnforcementHistoryConfig(): Promise<string> {
    if (this.useWorker && this.worker) {
      const res = await this.send({ type: "getEnforcementHistoryConfig", id: this.nextId() });
      return (res as any).result;
    }
    const m = await import("../rust/pkg/aegis_browser.js") as any;
    return m.get_enforcement_history_config?.() ?? JSON.stringify({ error: "not available" });
  }

  async enforcementTrends(limit?: number): Promise<string> {
    if (this.useWorker && this.worker) {
      const res = await this.send({ type: "enforcementTrends", id: this.nextId(), limit });
      return (res as any).result;
    }
    const m = await import("../rust/pkg/aegis_browser.js") as any;
    return m.enforcement_trends?.(limit ?? 100) ?? JSON.stringify({ error: "not available" });
  }

  destroy(): void {
    if (this.worker) {
      this.worker.terminate();
      this.worker = null;
    }
  }

  private nextId(): string {
    return `q${++this.counter}`;
  }

  private send(req: WorkerRequest): Promise<any> {
    return new Promise((resolve, reject) => {
      const id = req.id || this.nextId();
      const timedOut = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`Aegis worker request timed out: ${req.type}`));
      }, 30_000);

      this.pending.set(id, { resolve, reject, timer: timedOut });
      this.worker!.postMessage({ ...req, id });
    });
  }
}
