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

export interface AuditEntry {
  revision: number;
  action: string;
  subject: string;
  relation: string;
  object: string;
  timestamp: string;
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
}

export interface EngineConfig {
  maxReaders?: number;
  busyTimeoutMs?: number;
  walMode?: boolean;
  mmapSize?: number;
}

export function initialize(path: string, schemaYaml: string, config?: EngineConfig): JsAegis;
