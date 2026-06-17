import type { WorkerRequest, WorkerResponse, Tuple } from "./types";

let engine: AegisWasm | null = null;

interface AegisWasm {
  init_async(schema: string): string;
  init_sync(schema: string, inMemory: boolean): string;
  check(subject: string, permission: string, resource: string): boolean;
  write_relation(subject: string, relation: string, resource: string): string;
  delete_relation(subject: string, relation: string, resource: string): string;
  list_by_object(resource: string): string;
  list_by_subject(subject: string): string;
  export_json(): string;
  import_json(json: string): string;
  set_partition(partitionId: string): string;
  active_partition(): string;
  explain_v2(subject: string, permission: string, resource: string, consistency: string | null): string;
  who_can_access(permission: string, resource: string, pageOffset: number, pageLimit: number, includePaths: boolean): string;
  access_diff(schemaBefore: string, schemaAfter: string, maxChecks: number | null): string;
  list_policy_versions(): string;
  rollback_policy(version: number): string;
}

let counter = 0;

self.onmessage = async (e: MessageEvent<WorkerRequest>) => {
  const msg = e.data;
  const id = msg.id || `r${++counter}`;

  try {
    switch (msg.type) {
      case "init": {
        const wasm = await import("../rust/pkg/aegis_browser.js");
        await wasm.default();
        wasm.init_async(msg.schema);
        engine = wasm as unknown as AegisWasm;
        postMessage({ type: "init", id, ok: true } satisfies WorkerResponse);
        break;
      }

      case "check": {
        if (!engine) throw new Error("not initialized");
        const allowed = engine.check(msg.subject, msg.permission, msg.resource);
        postMessage({ type: "check", id, allowed } satisfies WorkerResponse);
        break;
      }

      case "write": {
        if (!engine) throw new Error("not initialized");
        const revision = engine.write_relation(msg.subject, msg.relation, msg.resource);
        postMessage({ type: "write", id, revision } satisfies WorkerResponse);
        break;
      }

      case "delete": {
        if (!engine) throw new Error("not initialized");
        const revision = engine.delete_relation(msg.subject, msg.relation, msg.resource);
        postMessage({ type: "delete", id, revision } satisfies WorkerResponse);
        break;
      }

      case "listByObject": {
        if (!engine) throw new Error("not initialized");
        const json = engine.list_by_object(msg.resource);
        const tuples: Tuple[] = JSON.parse(json);
        postMessage({ type: "listByObject", id, tuples } satisfies WorkerResponse);
        break;
      }

      case "listBySubject": {
        if (!engine) throw new Error("not initialized");
        const json = engine.list_by_subject(msg.subject);
        const tuples: Tuple[] = JSON.parse(json);
        postMessage({ type: "listBySubject", id, tuples } satisfies WorkerResponse);
        break;
      }

      case "export": {
        if (!engine) throw new Error("not initialized");
        const json = engine.export_json();
        postMessage({ type: "export", id, json } satisfies WorkerResponse);
        break;
      }

      case "import": {
        if (!engine) throw new Error("not initialized");
        engine.import_json(msg.json);
        postMessage({ type: "import", id, ok: true } satisfies WorkerResponse);
        break;
      }

      case "setPartition": {
        if (!engine) throw new Error("not initialized");
        engine.set_partition(msg.partitionId);
        postMessage({ type: "setPartition", id, ok: true } satisfies WorkerResponse);
        break;
      }

      case "activePartition": {
        if (!engine) throw new Error("not initialized");
        const partitionId = engine.active_partition();
        postMessage({ type: "activePartition", id, partitionId } satisfies WorkerResponse);
        break;
      }

      case "explainV2": {
        if (!engine) throw new Error("not initialized");
        const expResult = engine.explain_v2(msg.subject, msg.permission, msg.resource, msg.consistency ?? null);
        postMessage({ type: "explainV2", id, result: expResult } satisfies WorkerResponse);
        break;
      }

      case "whoCanAccess": {
        if (!engine) throw new Error("not initialized");
        const wcaResult = engine.who_can_access(msg.permission, msg.resource, msg.pageOffset ?? 0, msg.pageLimit ?? 100, msg.includePaths ?? false);
        postMessage({ type: "whoCanAccess", id, result: wcaResult } satisfies WorkerResponse);
        break;
      }

      case "accessDiff": {
        if (!engine) throw new Error("not initialized");
        const adResult = engine.access_diff(msg.schemaBefore, msg.schemaAfter, msg.maxChecks ?? null);
        postMessage({ type: "accessDiff", id, result: adResult } satisfies WorkerResponse);
        break;
      }

      case "listPolicyVersions": {
        if (!engine) throw new Error("not initialized");
        const lpvResult = engine.list_policy_versions();
        postMessage({ type: "listPolicyVersions", id, result: lpvResult } satisfies WorkerResponse);
        break;
      }

      case "rollbackPolicy": {
        if (!engine) throw new Error("not initialized");
        engine.rollback_policy(msg.version);
        postMessage({ type: "rollbackPolicy", id, ok: true } satisfies WorkerResponse);
        break;
      }
    }
  } catch (err: any) {
    postMessage({ type: msg.type, id, error: err.message ?? String(err) } as WorkerResponse);
  }
};
