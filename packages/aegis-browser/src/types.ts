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

export type PolicyDraft = {
  id: string;
  name: string;
  description: string;
  schema: string;
  baseVersion: number;
  status: string;
  createdAt: string;
  updatedAt: string;
  createdBy: string;
  approvedBy?: string;
  rejectionReason?: string;
};

export type ValidationReport = {
  schemaValid: boolean;
  accessDiffSummary?: any;
  simulationSummary?: any;
  warnings: string[];
};

export type PublishResult = {
  policyVersion: number;
  accessDiffSummary?: any;
  simulationSummary?: any;
};

export type AnalysisSchedule = {
  id: string;
  name: string;
  intervalSeconds: number;
  queries: Array<{ subject: string; permission: string; resource: string }>;
  compareSchema?: any;
  enabled: boolean;
  createdAt: string;
  updatedAt: string;
};

export type AnalysisRun = {
  id: string;
  scheduleId?: string;
  startedAt: string;
  completedAt: string;
  status: string;
  summary: any;
  errorMessage?: string;
};

export type EnforcementHistoryConfig = {
  enabled: boolean;
  sampling: string;
  maxEventsPerMinute: number;
  maxRows: number;
  maxDays: number;
};

export type EnforcementTrends = {
  totalEvents: number;
  deniedCount: number;
  allowedCount: number;
  byResource: Array<[string, number]>;
  recentEvents: EnforcementEvent[];
};

export type EnforcementEvent = {
  id: string;
  subject: string;
  permission: string;
  resource: string;
  allowed: boolean;
  revision: number;
  timestamp: string;
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
  | { type: "rollbackPolicy"; id: string; version: number }
  // V7 Policy Lifecycle
  | { type: "createPolicyDraft"; id: string; name: string; description: string }
  | { type: "updatePolicyDraft"; id: string; draftId: string; schemaJson: string }
  | { type: "validatePolicyDraft"; id: string; draftId: string }
  | { type: "submitPolicyDraftForReview"; id: string; draftId: string }
  | { type: "approvePolicyDraft"; id: string; draftId: string }
  | { type: "rejectPolicyDraft"; id: string; draftId: string; rejectionReason: string }
  | { type: "publishPolicyDraft"; id: string; draftId: string }
  | { type: "archivePolicyDraft"; id: string; draftId: string }
  | { type: "listPolicyDrafts"; id: string; filterStatus?: string }
  // V7 Scheduler
  | { type: "createAnalysisSchedule"; id: string; configJson: string }
  | { type: "listAnalysisSchedules"; id: string }
  | { type: "deleteAnalysisSchedule"; id: string; scheduleId: string }
  | { type: "runAnalysisNow"; id: string; scheduleId?: string }
  | { type: "getAnalysisRuns"; id: string; limit?: number }
  // V7 Enforcement History
  | { type: "setEnforcementHistoryConfig"; id: string; configJson: string }
  | { type: "getEnforcementHistoryConfig"; id: string }
  | { type: "enforcementTrends"; id: string; limit?: number }
  // V7 Subscribe
  | { type: "subscribe"; id: string; eventTypesJson: string };

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
  | { type: "rollbackPolicy"; id: string; ok: boolean; error?: string }
  // V7 Policy Lifecycle
  | { type: "createPolicyDraft"; id: string; result: string; error?: string }
  | { type: "updatePolicyDraft"; id: string; result: string; error?: string }
  | { type: "validatePolicyDraft"; id: string; result: string; error?: string }
  | { type: "submitPolicyDraftForReview"; id: string; result: string; error?: string }
  | { type: "approvePolicyDraft"; id: string; result: string; error?: string }
  | { type: "rejectPolicyDraft"; id: string; result: string; error?: string }
  | { type: "publishPolicyDraft"; id: string; result: string; error?: string }
  | { type: "archivePolicyDraft"; id: string; result: string; error?: string }
  | { type: "listPolicyDrafts"; id: string; result: string; error?: string }
  // V7 Scheduler
  | { type: "createAnalysisSchedule"; id: string; result: string; error?: string }
  | { type: "listAnalysisSchedules"; id: string; result: string; error?: string }
  | { type: "deleteAnalysisSchedule"; id: string; ok: boolean; error?: string }
  | { type: "runAnalysisNow"; id: string; result: string; error?: string }
  | { type: "getAnalysisRuns"; id: string; result: string; error?: string }
  // V7 Enforcement History
  | { type: "setEnforcementHistoryConfig"; id: string; ok: boolean; error?: string }
  | { type: "getEnforcementHistoryConfig"; id: string; result: string; error?: string }
  | { type: "enforcementTrends"; id: string; result: string; error?: string }
  // V7 Subscribe
  | { type: "subscribe"; id: string; result: string; error?: string };
