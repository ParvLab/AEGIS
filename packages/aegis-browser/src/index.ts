import type { AegisConfig, Tuple, WorkerRequest, WorkerResponse } from "./types";

export type { AegisConfig, Tuple, CheckResult, AuditEntry, ExplainV2Response, WhoCanAccessResponse, AccessDiffResponse, PolicyVersion } from "./types";

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
