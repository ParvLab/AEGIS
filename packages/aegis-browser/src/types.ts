export type CheckResult = {
  allowed: boolean;
  reason?: string;
};

export type Tuple = {
  subject: string;
  relation: string;
  object: string;
  condition?: string;
  metadata?: Record<string, string>;
};

export type AuditEntry = {
  revision: number;
  action: "add" | "remove";
  subject: string;
  relation: string;
  object: string;
  timestamp: string;
  identity?: string;
};

export type AegisConfig = {
  dbName?: string;
  useWorker?: boolean;
  workerUrl?: string;
  partitionId?: string;
};

export type ExplainV2Request = {
  subject: string;
  permission: string;
  resource: string;
  consistency?: string;
};

export type ExplainV2Response = {
  allowed: boolean;
  revision: number;
  trace: { subject: string; relation: string; object: string }[];
  resolvedVia: string;
  durationMs: number;
};

export type WhoCanAccessResponse = {
  subjects: { subject: string; path?: string[] }[];
  nextOffset: number;
  totalCount: number;
};

export type AccessDiffRequest = {
  schemaBefore: string;
  schemaAfter: string;
  maxChecks?: number;
};

export type AccessDiffResponse = {
  changed: boolean;
  added: { subject: string; permission: string; resource: string }[];
  removed: { subject: string; permission: string; resource: string }[];
  summary: string;
};

export type PolicyVersion = {
  version: number;
  schema: string;
  created_at: string;
  description?: string;
};

export type WorkerRequest =
  | { type: "init"; id: string; schema: string; dbName?: string }
  | { type: "check"; id: string; subject: string; permission: string; resource: string }
  | { type: "write"; id: string; subject: string; relation: string; resource: string }
  | { type: "delete"; id: string; subject: string; relation: string; resource: string }
  | { type: "listByObject"; id: string; resource: string }
  | { type: "listBySubject"; id: string; subject: string }
  | { type: "export"; id: string }
  | { type: "import"; id: string; json: string }
  | { type: "setPartition"; id: string; partitionId: string }
  | { type: "activePartition"; id: string }
  | { type: "explainV2"; id: string; subject: string; permission: string; resource: string; consistency?: string }
  | { type: "whoCanAccess"; id: string; permission: string; resource: string; pageOffset?: number; pageLimit?: number; includePaths?: boolean }
  | { type: "accessDiff"; id: string; schemaBefore: string; schemaAfter: string; maxChecks?: number }
  | { type: "listPolicyVersions"; id: string }
  | { type: "rollbackPolicy"; id: string; version: number };

export type WorkerResponse =
  | { type: "init"; id: string; ok: boolean; error?: string }
  | { type: "check"; id: string; allowed: boolean; error?: string }
  | { type: "write"; id: string; revision: string; error?: string }
  | { type: "delete"; id: string; revision: string; error?: string }
  | { type: "listByObject"; id: string; tuples: Tuple[]; error?: string }
  | { type: "listBySubject"; id: string; tuples: Tuple[]; error?: string }
  | { type: "export"; id: string; json: string; error?: string }
  | { type: "import"; id: string; ok: boolean; error?: string }
  | { type: "setPartition"; id: string; ok: boolean; error?: string }
  | { type: "activePartition"; id: string; partitionId: string; error?: string }
  | { type: "explainV2"; id: string; result: string; error?: string }
  | { type: "whoCanAccess"; id: string; result: string; error?: string }
  | { type: "accessDiff"; id: string; result: string; error?: string }
  | { type: "listPolicyVersions"; id: string; result: string; error?: string }
  | { type: "rollbackPolicy"; id: string; ok: boolean; error?: string };
