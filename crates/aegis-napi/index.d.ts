export interface InitializeResult {
  schemaVersion: number;
  revision: number;
  healthy: boolean;
}

export interface CheckResult {
  allowed: boolean;
  revision: number;
}

export interface WriteResult {
  revision: number;
  nodeId: string;
  timestamp: string;
}

export interface Tuple {
  subject: string;
  relation: string;
  object: string;
  condition?: string;
  metadata?: Record<string, string>;
  validUntil?: string;
}

export interface ConditionContext {
  subjectMeta?: Record<string, string>;
  resourceMeta?: Record<string, string>;
  env?: Record<string, string>;
}

export interface ExplainTrace {
  subject: string;
  relation: string;
  object: string;
}

export interface ExplainResult {
  allowed: boolean;
  revision: number;
  trace: ExplainTrace[];
  resolvedVia: string;
  durationMs: number;
}

export interface ExplainV2TraceStep {
  subject: string;
  relation: string;
  object: string;
  result: boolean;
  depth: number;
}

export interface ExplainV2Result {
  allowed: boolean;
  revision: number;
  trace: ExplainV2TraceStep[];
  resolvedVia: string;
  durationMs: number;
  cacheHit: boolean;
}

export interface WhoCanAccessResult {
  subjects: Array<{ subject: string; path?: string[] }>;
  nextOffset: number;
  totalCount: number;
}

export interface AccessDiffResult {
  changed: boolean;
  added: Array<{ subject: string; permission: string; resource: string }>;
  removed: Array<{ subject: string; permission: string; resource: string }>;
  summary: string;
}

export interface PolicyVersion {
  version: number;
  schema: string;
  created_at: string;
  description?: string;
}

export interface ConnectionStats {
  readActive: number;
  readIdle: number;
  writeBusy: boolean;
}

export interface HealthReport {
  healthy: boolean;
  error?: string;
  revision: number;
  schemaVersion: number;
  backend: string;
  backendHealthy: boolean;
  telemetryHealthy: boolean;
  cacheHitRate: number;
  cacheEntries: number;
  storageIntegrity: boolean;
  totalChecks: number;
  allowedChecks: number;
  deniedChecks: number;
  errorChecks: number;
  cacheSize: number;
  cacheHitRatio: number;
  integrityStatus: string;
  uptimeMs: number;
  storageVersion?: string;
  connections: ConnectionStats;
  walSizeMb?: number;
}

export interface QueryFilter {
  subjectType?: string;
  relation?: string;
  objectType?: string;
  metadataKey?: string;
  metadataValue?: string;
}

export interface Pagination {
  limit: number;
  cursorOffset?: number;
}

export interface PaginatedTuples {
  tuples: Tuple[];
  nextCursor?: number;
  revision: number;
}

export interface SchemaCheckReport {
  compatible: boolean;
  warnings: string[];
  breaking: string[];
}

export interface IntegrityResult {
  ok: boolean;
  brokenChainAt?: number;
  details?: string;
}

export interface AnalysisReport {
  orphanedTupleCount: number;
  orphanedTuples: string[];
  highAccessSubjects: string[];
  integrityOk: boolean;
  summary: string;
}

export interface PartitionInfo {
  id: string;
  isActive: boolean;
  tupleCount: number;
}

export interface AuditEntry {
  revision: number;
  action: string;
  subject: string;
  relation: string;
  object: string;
  timestamp: string;
  identity?: string;
}

export interface WatchEvent {
  eventType: string;
  subject: string;
  relation: string;
  object: string;
  revision: number;
  timestamp: string;
}

export interface ExportSubjectResult {
  subject: string;
  activeTuples: Tuple[];
  exportRevision: number;
  exportedAt: string;
}

export class JsWatchSubscription {
  poll(): WatchEvent | null;
  unsubscribe(): void;
}

export class JsTransaction {
  write(subject: string, relation: string, resource: string): void;
  delete(subject: string, relation: string, resource: string): void;
  savepoint(name: string): void;
  rollbackToSavepoint(name: string): void;
  releaseSavepoint(name: string): void;
  commit(): WriteResult;
  rollback(): void;
}

export class JsAegis {
  initializeResult(): InitializeResult;
  check(subject: string, permission: string, resource: string, consistency?: string): CheckResult;
  checkWithContext(subject: string, permission: string, resource: string, context: ConditionContext, consistency?: string): CheckResult;
  checkDryRunWithContext(subject: string, permission: string, resource: string, context: ConditionContext, consistency?: string): CheckResult;
  write(subject: string, relation: string, resource: string, condition?: string, metadata?: Record<string, string>, validUntil?: string): WriteResult;
  delete(subject: string, relation: string, resource: string): WriteResult;
  listByObject(object: string, relation?: string, consistency?: string): Tuple[];
  listBySubject(subject: string, relation?: string, consistency?: string): Tuple[];
  listByRelation(object: string, relation: string): Tuple[];
  explain(subject: string, permission: string, resource: string, consistency?: string): ExplainResult;
  health(): HealthReport;
  checkDryRun(subject: string, permission: string, resource: string, consistency?: string): CheckResult;
  query(filter: QueryFilter, pagination: Pagination, consistency?: string): PaginatedTuples;
  writeBatch(tuples: Tuple[]): WriteResult;
  writeDryRun(subject: string, relation: string, resource: string, condition?: string, metadata?: Record<string, string>, validUntil?: string): CheckResult;
  migrate(targetVersion: number): void;
  checkSchema(schemaYaml: string): SchemaCheckReport;
  deleteObject(object: string): WriteResult;
  exportSubject(subject: string): ExportSubjectResult;
  deleteSubjectWithPolicy(subject: string, policy: string, transferToSubject?: string): WriteResult;
  queryAudit(object: string, fromRevision?: number, toRevision?: number, limit?: number): AuditEntry[];
  queryAuditAll(fromRevision?: number, toRevision?: number, limit?: number): AuditEntry[];
  close(): void;
  reloadSchema(schemaYaml: string): void;
  watch(subjectType?: string, relation?: string, objectType?: string): JsWatchSubscription;
  transaction(): JsTransaction;
  invalidateCache(): void;
  isClosed(): boolean;
  verifyAuditChain(): IntegrityResult;
  analysisReport(): AnalysisReport;
  accessReviewForSubject(subject: string): string;
  accessReviewForResource(resource: string): string;
  invalidateCacheBefore(revision: number): void;
  createPartition(id: string): void;
  deletePartition(id: string): void;
  listPartitions(): PartitionInfo[];
  activePartition(): string;
  switchPartition(id: string): void;
  backupToPath(destPath: string): void;
  exportJson(): string;
  importJson(json: string): WriteResult;

  // V6 Analysis APIs
  explainV2(subject: string, permission: string, resource: string, consistency?: string): ExplainV2Result;
  whoCanAccess(permission: string, resource: string, pageOffset?: number, pageLimit?: number, includePaths?: boolean): WhoCanAccessResult;
  accessDiff(schemaBefore: string, schemaAfter: string, maxChecks?: number): AccessDiffResult;
  listPolicyVersions(): PolicyVersion[];
  rollbackPolicy(version: number): void;

  // V7 Policy Lifecycle
  createPolicyDraft(name: string, description: string): PolicyDraft;
  updatePolicyDraft(id: string, schemaJson: string): PolicyDraft;
  validatePolicyDraft(id: string): ValidationReport;
  submitPolicyDraftForReview(id: string): PolicyDraft;
  approvePolicyDraft(id: string): PolicyDraft;
  rejectPolicyDraft(id: string, rejectionReason: string): PolicyDraft;
  publishPolicyDraft(id: string): PublishResult;
  archivePolicyDraft(id: string): PolicyDraft;
  listPolicyDrafts(filterStatus?: string): PolicyDraft[];

  // V7 Scheduler
  createAnalysisSchedule(name: string, intervalSeconds: number, queriesJson: string, compareSchemaJson?: string): AnalysisSchedule;
  listAnalysisSchedules(): AnalysisSchedule[];
  deleteAnalysisSchedule(id: string): boolean;
  runAnalysisNow(scheduleId?: string): AnalysisRun[];
  getAnalysisRuns(limit?: number): AnalysisRun[];

  // V7 Enforcement History
  setEnforcementHistoryConfig(configJson: string): void;
  getEnforcementHistoryConfig(): EnforcementHistoryConfig;
  enforcementTrends(limit?: number): EnforcementTrends;

  // V7 Subscribe
  subscribe(eventTypes: string[]): JsWatchSubscription;
}

// V7 interfaces
export interface PolicyDraft {
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
}

export interface ValidationReport {
  schemaValid: boolean;
  accessDiffSummary?: any;
  simulationSummary?: any;
  warnings: string[];
}

export interface PublishResult {
  policyVersion: number;
  accessDiffSummary?: any;
  simulationSummary?: any;
}

export interface AnalysisSchedule {
  id: string;
  name: string;
  intervalSeconds: number;
  queries: Array<{ subject: string; permission: string; resource: string }>;
  compareSchema?: any;
  enabled: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface AnalysisRun {
  id: string;
  scheduleId?: string;
  startedAt: string;
  completedAt: string;
  status: string;
  summary: any;
  errorMessage?: string;
}

export interface EnforcementHistoryConfig {
  enabled: boolean;
  sampling: string;
  maxEventsPerMinute: number;
  maxRows: number;
  maxDays: number;
}

export interface EnforcementTrends {
  totalEvents: number;
  deniedCount: number;
  allowedCount: number;
  byResource: Array<[string, number]>;
  recentEvents: EnforcementEvent[];
}

export interface EnforcementEvent {
  id: string;
  subject: string;
  permission: string;
  resource: string;
  allowed: boolean;
  revision: number;
  timestamp: string;
}

export interface EngineConfig {
  maxReaders?: number;
  busyTimeoutMs?: number;
  walMode?: boolean;
  mmapSize?: number;
}

export function initialize(path: string, schemaYaml: string, config?: EngineConfig): JsAegis;
