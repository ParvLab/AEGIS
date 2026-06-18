package aegis

/*
#cgo LDFLAGS: -laegis_ffi
#include <stdlib.h>
#include "../aegis-ffi/aegis_ffi.h"
*/
import "C"
import (
	"encoding/json"
	"errors"
	"runtime"
	"sync"
	"unsafe"
)

// Global logger bridge for C callbacks.
var (
	loggerMu     sync.Mutex
	loggerFn     func(int, string, string)
)

//export aegisGoLogBridge
func aegisGoLogBridge(level C.int, target, msg *C.char, user_data unsafe.Pointer) {
	loggerMu.Lock()
	cb := loggerFn
	loggerMu.Unlock()
	if cb != nil {
		cb(int(level), C.GoString(target), C.GoString(msg))
	}
}

// Engine wraps the C AegisEngine handle.
type Engine struct {
	ptr *C.AegisEngine
}

// CheckResult holds the result of a permission check.
type CheckResult struct {
	Allowed  bool
	Revision uint64
}

// WriteResult holds the result of a write/delete operation.
type WriteResult struct {
	Revision uint64
}

// HealthReport holds engine health information.
type HealthReport struct {
	Healthy       bool
	Revision      uint64
	SchemaVersion int
	Error         string
	Backend       string
	BackendHealthy bool
	CacheHitRate  float64
	CacheEntries  int
	StorageIntegrity bool
	TotalChecks   float64
	AllowedChecks float64
	DeniedChecks  float64
	ErrorChecks   float64
	UptimeMs      float64
}

// ExplainResult holds the result of an explain call.
type ExplainResult struct {
	Allowed    bool
	Revision   uint64
	Trace      []TraceEntry
	ResolvedBy string
	DurationMs uint64
}

// TraceEntry is a single step in an explain trace.
type TraceEntry struct {
	Subject  string `json:"subject"`
	Relation string `json:"relation"`
	Object   string `json:"object"`
}

// Tuple is a relationship tuple.
type Tuple struct {
	Subject    string            `json:"subject"`
	Relation   string            `json:"relation"`
	Object     string            `json:"object"`
	Condition  string            `json:"condition,omitempty"`
	Metadata   map[string]string `json:"metadata,omitempty"`
	ValidUntil string            `json:"valid_until,omitempty"`
}

// SchemaCheckReport holds schema compatibility info.
type SchemaCheckReport struct {
	Compatible bool     `json:"compatible"`
	Warnings   []string `json:"warnings"`
	Breaking   []string `json:"breaking"`
}

// AuditEntry holds a single audit log entry.
type AuditEntry struct {
	Revision  uint64 `json:"revision"`
	Action    string `json:"action"`
	Subject   string `json:"subject"`
	Relation  string `json:"relation"`
	Object    string `json:"object"`
	Timestamp string `json:"timestamp"`
}

// QueryFilter holds filter parameters for the Query method.
type QueryFilter struct {
	SubjectType   string `json:"subject_type,omitempty"`
	Relation      string `json:"relation,omitempty"`
	ObjectType    string `json:"object_type,omitempty"`
	MetadataKey   string `json:"metadata_key,omitempty"`
	MetadataValue string `json:"metadata_value,omitempty"`
}

// PaginatedTuples holds a page of query results.
type PaginatedTuples struct {
	Tuples     []Tuple `json:"tuples"`
	NextCursor uint64  `json:"next_cursor"`
	Revision   uint64  `json:"revision"`
}

// V7 Policy Draft types
type PolicyDraft struct {
	ID              string             `json:"id"`
	Name            string             `json:"name"`
	Description     string             `json:"description"`
	Status          string             `json:"status"`
	Schema          string             `json:"schema"`
	CreatedAt       string             `json:"created_at"`
	UpdatedAt       string             `json:"updated_at"`
	CreatedBy       string             `json:"created_by"`
	ApprovedBy      string             `json:"approved_by,omitempty"`
	RejectionReason string             `json:"rejection_reason,omitempty"`
	Validation      *ValidationReport  `json:"validation,omitempty"`
}

type ValidationReport struct {
	Valid    bool     `json:"valid"`
	Errors   []string `json:"errors,omitempty"`
	Warnings []string `json:"warnings,omitempty"`
}

type PublishResult struct {
	Version          uint32 `json:"version"`
	PublishedAt      string `json:"published_at"`
	PreviousVersion  uint32 `json:"previous_version"`
}

type AnalysisSchedule struct {
	ID        string `json:"id"`
	Query     string `json:"query"`
	Cron      string `json:"cron_expression"`
	Enabled   bool   `json:"enabled"`
	CreatedAt string `json:"created_at,omitempty"`
}

type AnalysisRun struct {
	ID          string `json:"id"`
	ScheduleID  string `json:"schedule_id,omitempty"`
	Status      string `json:"status"`
	StartedAt   string `json:"started_at"`
	CompletedAt string `json:"completed_at,omitempty"`
	Result      string `json:"result,omitempty"`
}

type EnforcementHistoryConfig struct {
	Enabled         bool    `json:"enabled"`
	SamplingMode    string  `json:"sampling_mode"`
	SamplingRate    float64 `json:"sampling_rate"`
	MaxEventsPerMin uint64  `json:"max_events_per_minute"`
	MaxRows         *uint64 `json:"max_rows,omitempty"`
	MaxDays         *uint64 `json:"max_days,omitempty"`
}

type EnforcementTrends struct {
	TotalEvents  uint64            `json:"total_events"`
	DeniedCount  uint64            `json:"denied_count"`
	AllowedCount uint64            `json:"allowed_count"`
	PeriodStart  string            `json:"period_start"`
	PeriodEnd    string            `json:"period_end"`
	ByPermission map[string]uint64 `json:"by_permission,omitempty"`
	ByResource   map[string]uint64 `json:"by_resource,omitempty"`
}

// Consistency mode constants
const (
	ConsistencyDefault        = -1
	ConsistencyMinimizeLatency = 0
	ConsistencyFullyConsistent = 1
)

// ── Lifecycle ──

// EngineConfig overrides default SqliteConfig values.
type EngineConfig struct {
	MaxReaders    *uint32
	BusyTimeoutMs *uint32
	WalMode       *bool
	MmapSize      *uint64
}

// NewEngine creates a new Aegis engine instance with default config.
func NewEngine(dbPath, schemaYAML string) (*Engine, error) {
	return NewEngineWithConfig(dbPath, schemaYAML, EngineConfig{})
}

// NewEngineWithConfig creates a new Aegis engine with custom config.
func NewEngineWithConfig(dbPath, schemaYAML string, cfg EngineConfig) (*Engine, error) {
	cDB := C.CString(dbPath)
	cSchema := C.CString(schemaYAML)
	defer C.free(unsafe.Pointer(cDB))
	defer C.free(unsafe.Pointer(cSchema))

	if cfg.MaxReaders == nil && cfg.BusyTimeoutMs == nil && cfg.WalMode == nil && cfg.MmapSize == nil {
		ptr := C.aegis_engine_create(cDB, cSchema)
		if ptr == nil {
			return nil, errors.New("aegis_engine_create failed")
		}
		e := &Engine{ptr: ptr}
		runtime.SetFinalizer(e, (*Engine).Destroy)
		return e, nil
	}

	cfgMap := make(map[string]interface{})
	if cfg.MaxReaders != nil {
		cfgMap["max_readers"] = *cfg.MaxReaders
	}
	if cfg.BusyTimeoutMs != nil {
		cfgMap["busy_timeout_ms"] = *cfg.BusyTimeoutMs
	}
	if cfg.WalMode != nil {
		cfgMap["wal_mode"] = *cfg.WalMode
	}
	if cfg.MmapSize != nil {
		cfgMap["mmap_size"] = *cfg.MmapSize
	}
	cfgJSON, err := json.Marshal(cfgMap)
	if err != nil {
		return nil, err
	}
	cCfg := C.CString(string(cfgJSON))
	defer C.free(unsafe.Pointer(cCfg))

	ptr := C.aegis_engine_create_with_config(cDB, cSchema, cCfg)
	if ptr == nil {
		return nil, errors.New("aegis_engine_create_with_config failed")
	}
	e := &Engine{ptr: ptr}
	runtime.SetFinalizer(e, (*Engine).Destroy)
	return e, nil
}

// Close gracefully closes the engine.
func (e *Engine) Close() error {
	if e.ptr == nil {
		return nil
	}
	res := C.aegis_engine_close(e.ptr)
	if res != nil {
		defer C.aegis_free_string(res)
		return errors.New(C.GoString(res))
	}
	return nil
}

// Destroy destroys the engine handle (finalizer).
func (e *Engine) Destroy() {
	if e.ptr != nil {
		C.aegis_engine_destroy(e.ptr)
		e.ptr = nil
	}
}

// IsClosed returns whether the engine is closed.
func (e *Engine) IsClosed() bool {
	if e.ptr == nil {
		return true
	}
	return bool(C.aegis_engine_is_closed(e.ptr))
}

// SetLogger registers a callback for engine log messages.
// Pass nil to clear the logger.
func (e *Engine) SetLogger(callback func(int, string, string)) {
	loggerMu.Lock()
	loggerFn = callback
	loggerMu.Unlock()

	if callback != nil {
		C.aegis_engine_set_logger(e.ptr, (*[0]byte)(C.aegisGoLogBridge), nil)
	} else {
		C.aegis_engine_set_logger(e.ptr, nil, nil)
	}
}

// ── Core operations ──

// Check whether a subject has a permission on a resource.
func (e *Engine) Check(subject, permission, resource string) (CheckResult, error) {
	return e.CheckWithConsistency(subject, permission, resource, ConsistencyDefault)
}

// CheckWithConsistency checks with a specific consistency mode.
func (e *Engine) CheckWithConsistency(subject, permission, resource string, consistency int) (CheckResult, error) {
	cSub := C.CString(subject)
	cPerm := C.CString(permission)
	cRes := C.CString(resource)
	defer C.free(unsafe.Pointer(cSub))
	defer C.free(unsafe.Pointer(cPerm))
	defer C.free(unsafe.Pointer(cRes))

	res := C.aegis_engine_check_ex(e.ptr, cSub, cPerm, cRes, C.int32_t(consistency))
	if res.error != nil {
		defer C.aegis_free_string(res.error)
		return CheckResult{}, errors.New(C.GoString(res.error))
	}
	return CheckResult{Allowed: bool(res.allowed), Revision: uint64(res.revision)}, nil
}

// CheckWithContext checks with ABAC context for condition evaluation.
func (e *Engine) CheckWithContext(subject, permission, resource, contextJSON string, consistency int) (CheckResult, error) {
	cSub := C.CString(subject)
	cPerm := C.CString(permission)
	cRes := C.CString(resource)
	cCtx := C.CString(contextJSON)
	defer C.free(unsafe.Pointer(cSub))
	defer C.free(unsafe.Pointer(cPerm))
	defer C.free(unsafe.Pointer(cRes))
	defer C.free(unsafe.Pointer(cCtx))

	res := C.aegis_engine_check_with_context(e.ptr, cSub, cPerm, cRes, cCtx, C.int32_t(consistency))
	if res.error != nil {
		defer C.aegis_free_string(res.error)
		return CheckResult{}, errors.New(C.GoString(res.error))
	}
	return CheckResult{Allowed: bool(res.allowed), Revision: uint64(res.revision)}, nil
}

// Write a relationship tuple.
func (e *Engine) Write(subject, relation, resource string) (WriteResult, error) {
	cSub := C.CString(subject)
	cRel := C.CString(relation)
	cRes := C.CString(resource)
	defer C.free(unsafe.Pointer(cSub))
	defer C.free(unsafe.Pointer(cRel))
	defer C.free(unsafe.Pointer(cRes))

	res := C.aegis_engine_write(e.ptr, cSub, cRel, cRes)
	if res.error != nil {
		defer C.aegis_free_string(res.error)
		return WriteResult{}, errors.New(C.GoString(res.error))
	}
	return WriteResult{Revision: uint64(res.revision)}, nil
}

// WriteAdvanced writes a relationship tuple with optional condition, metadata, and expiry.
// Pass empty strings or nil for fields you don't want to set.
func (e *Engine) WriteAdvanced(subject, relation, resource, condition string, metadata map[string]string, validUntil string) (WriteResult, error) {
	cSub := C.CString(subject)
	cRel := C.CString(relation)
	cRes := C.CString(resource)
	defer C.free(unsafe.Pointer(cSub))
	defer C.free(unsafe.Pointer(cRel))
	defer C.free(unsafe.Pointer(cRes))

	var cCond *C.char
	if condition != "" {
		cCond = C.CString(condition)
		defer C.free(unsafe.Pointer(cCond))
	} else {
		cCond = nil
	}

	var cMeta *C.char
	if metadata != nil {
		metaJSON, err := json.Marshal(metadata)
		if err != nil {
			return WriteResult{}, err
		}
		cMeta = C.CString(string(metaJSON))
		defer C.free(unsafe.Pointer(cMeta))
	} else {
		cMeta = nil
	}

	var cValid *C.char
	if validUntil != "" {
		cValid = C.CString(validUntil)
		defer C.free(unsafe.Pointer(cValid))
	} else {
		cValid = nil
	}

	res := C.aegis_engine_write_ex(e.ptr, cSub, cRel, cRes, cCond, cMeta, cValid)
	if res.error != nil {
		defer C.aegis_free_string(res.error)
		return WriteResult{}, errors.New(C.GoString(res.error))
	}
	return WriteResult{Revision: uint64(res.revision)}, nil
}

// Delete a relationship tuple.
func (e *Engine) Delete(subject, relation, resource string) (WriteResult, error) {
	cSub := C.CString(subject)
	cRel := C.CString(relation)
	cRes := C.CString(resource)
	defer C.free(unsafe.Pointer(cSub))
	defer C.free(unsafe.Pointer(cRel))
	defer C.free(unsafe.Pointer(cRes))

	r := C.aegis_engine_delete(e.ptr, cSub, cRel, cRes)
	if r.error != nil {
		defer C.aegis_free_string(r.error)
		return WriteResult{}, errors.New(C.GoString(r.error))
	}
	return WriteResult{Revision: uint64(r.revision)}, nil
}

// Health returns engine health information.
func (e *Engine) Health() HealthReport {
	res := C.aegis_engine_health(e.ptr)
	if res.error != nil {
		defer C.aegis_free_string(res.error)
	}
	return HealthReport{
		Healthy:       bool(res.healthy),
		Revision:      uint64(res.revision),
		SchemaVersion: int(res.schema_version),
	}
}

// ── Extended operations ──

// Explain returns a detailed trace of a permission check.
func (e *Engine) Explain(subject, permission, resource string) (ExplainResult, error) {
	cSub := C.CString(subject)
	cPerm := C.CString(permission)
	cRes := C.CString(resource)
	defer C.free(unsafe.Pointer(cSub))
	defer C.free(unsafe.Pointer(cPerm))
	defer C.free(unsafe.Pointer(cRes))

	res := C.aegis_engine_explain(e.ptr, cSub, cPerm, cRes)
	if res.error != nil {
		defer C.aegis_free_string(res.error)
		return ExplainResult{}, errors.New(C.GoString(res.error))
	}

	var trace []TraceEntry
	if res.trace_json != nil {
		defer C.aegis_free_string(res.trace_json)
		json.Unmarshal([]byte(C.GoString(res.trace_json)), &trace)
	}

	resolvedBy := ""
	if res.resolved_via != nil {
		defer C.aegis_free_string(res.resolved_via)
		resolvedBy = C.GoString(res.resolved_via)
	}

	return ExplainResult{
		Allowed:    bool(res.allowed),
		Revision:   uint64(res.revision),
		Trace:      trace,
		ResolvedBy: resolvedBy,
		DurationMs: uint64(res.duration_ms),
	}, nil
}

// ListByObject returns all tuples for a given object, optionally filtered by relation.
func (e *Engine) ListByObject(object, relation string) ([]Tuple, error) {
	cObj := C.CString(object)
	var cRel *C.char
	if relation != "" {
		cRel = C.CString(relation)
	} else {
		cRel = nil
	}
	defer C.free(unsafe.Pointer(cObj))
	if cRel != nil {
		defer C.free(unsafe.Pointer(cRel))
	}

	res := C.aegis_engine_list_by_object(e.ptr, cObj, cRel)
	if res.error != nil {
		defer C.aegis_free_string(res.error)
		return nil, errors.New(C.GoString(res.error))
	}

	var tuples []Tuple
	if res.tuples_json != nil {
		defer C.aegis_free_string(res.tuples_json)
		json.Unmarshal([]byte(C.GoString(res.tuples_json)), &tuples)
	}
	return tuples, nil
}

// ListBySubject returns all tuples for a given subject, optionally filtered by relation.
func (e *Engine) ListBySubject(subject, relation string) ([]Tuple, error) {
	cSub := C.CString(subject)
	var cRel *C.char
	if relation != "" {
		cRel = C.CString(relation)
	} else {
		cRel = nil
	}
	defer C.free(unsafe.Pointer(cSub))
	if cRel != nil {
		defer C.free(unsafe.Pointer(cRel))
	}

	res := C.aegis_engine_list_by_subject(e.ptr, cSub, cRel)
	if res.error != nil {
		defer C.aegis_free_string(res.error)
		return nil, errors.New(C.GoString(res.error))
	}

	var tuples []Tuple
	if res.tuples_json != nil {
		defer C.aegis_free_string(res.tuples_json)
		json.Unmarshal([]byte(C.GoString(res.tuples_json)), &tuples)
	}
	return tuples, nil
}

// ListByRelation returns all tuples for an object+relation combination.
func (e *Engine) ListByRelation(object, relation string) ([]Tuple, error) {
	cObj := C.CString(object)
	cRel := C.CString(relation)
	defer C.free(unsafe.Pointer(cObj))
	defer C.free(unsafe.Pointer(cRel))

	res := C.aegis_engine_list_by_relation(e.ptr, cObj, cRel)
	if res.error != nil {
		defer C.aegis_free_string(res.error)
		return nil, errors.New(C.GoString(res.error))
	}

	var tuples []Tuple
	if res.tuples_json != nil {
		defer C.aegis_free_string(res.tuples_json)
		json.Unmarshal([]byte(C.GoString(res.tuples_json)), &tuples)
	}
	return tuples, nil
}

// WriteBatch writes multiple tuples atomically.
func (e *Engine) WriteBatch(tuples []Tuple) (WriteResult, error) {
	data, err := json.Marshal(tuples)
	if err != nil {
		return WriteResult{}, err
	}
	cJSON := C.CString(string(data))
	defer C.free(unsafe.Pointer(cJSON))

	res := C.aegis_engine_write_batch(e.ptr, cJSON)
	if res.error != nil {
		defer C.aegis_free_string(res.error)
		return WriteResult{}, errors.New(C.GoString(res.error))
	}
	return WriteResult{Revision: uint64(res.revision)}, nil
}

// Migrate runs storage migrations to a target version.
func (e *Engine) Migrate(targetVersion int) error {
	res := C.aegis_engine_migrate(e.ptr, C.int32_t(targetVersion))
	if res != nil {
		defer C.aegis_free_string(res)
		return errors.New(C.GoString(res))
	}
	return nil
}

// DeleteObject deletes all tuples referencing an object.
func (e *Engine) DeleteObject(object string) (WriteResult, error) {
	cObj := C.CString(object)
	defer C.free(unsafe.Pointer(cObj))

	res := C.aegis_engine_delete_object(e.ptr, cObj)
	if res.error != nil {
		defer C.aegis_free_string(res.error)
		return WriteResult{}, errors.New(C.GoString(res.error))
	}
	return WriteResult{Revision: uint64(res.revision)}, nil
}

// CheckSchema tests schema compatibility without applying it.
func (e *Engine) CheckSchema(schemaYAML string) (SchemaCheckReport, error) {
	cYAML := C.CString(schemaYAML)
	defer C.free(unsafe.Pointer(cYAML))

	res := C.aegis_engine_check_schema(e.ptr, cYAML)
	if res == nil {
		return SchemaCheckReport{}, errors.New("check_schema returned null")
	}
	defer C.aegis_free_string(res)

	var report SchemaCheckReport
	if err := json.Unmarshal([]byte(C.GoString(res)), &report); err != nil {
		return SchemaCheckReport{}, err
	}
	return report, nil
}

// CheckDryRun performs a dry-run permission check (no cache side effects).
func (e *Engine) CheckDryRun(subject, permission, resource string) (CheckResult, error) {
	cSub := C.CString(subject)
	cPerm := C.CString(permission)
	cRes := C.CString(resource)
	defer C.free(unsafe.Pointer(cSub))
	defer C.free(unsafe.Pointer(cPerm))
	defer C.free(unsafe.Pointer(cRes))

	res := C.aegis_engine_check_dry_run(e.ptr, cSub, cPerm, cRes)
	if res.error != nil {
		defer C.aegis_free_string(res.error)
		return CheckResult{}, errors.New(C.GoString(res.error))
	}
	return CheckResult{Allowed: bool(res.allowed), Revision: uint64(res.revision)}, nil
}

// WriteDryRun tests whether a write would succeed without persisting.
func (e *Engine) WriteDryRun(subject, relation, resource string) (CheckResult, error) {
	cSub := C.CString(subject)
	cRel := C.CString(relation)
	cRes := C.CString(resource)
	defer C.free(unsafe.Pointer(cSub))
	defer C.free(unsafe.Pointer(cRel))
	defer C.free(unsafe.Pointer(cRes))

	res := C.aegis_engine_write_dry_run(e.ptr, cSub, cRel, cRes)
	if res.error != nil {
		defer C.aegis_free_string(res.error)
		return CheckResult{}, errors.New(C.GoString(res.error))
	}
	return CheckResult{Revision: uint64(res.revision)}, nil
}

// ExportSubject exports all tuples for a subject (GDPR).
func (e *Engine) ExportSubject(subject string) ([]Tuple, uint64, error) {
	cSub := C.CString(subject)
	defer C.free(unsafe.Pointer(cSub))

	res := C.aegis_engine_export_subject(e.ptr, cSub)
	if res.error != nil {
		defer C.aegis_free_string(res.error)
		return nil, 0, errors.New(C.GoString(res.error))
	}

	var tuples []Tuple
	if res.tuples_json != nil {
		defer C.aegis_free_string(res.tuples_json)
		json.Unmarshal([]byte(C.GoString(res.tuples_json)), &tuples)
	}
	return tuples, uint64(res.export_revision), nil
}

// DeleteSubjectWithPolicy deletes a subject with a GDPR policy.
func (e *Engine) DeleteSubjectWithPolicy(subject, policy string, transferToSubject string) (WriteResult, error) {
	cSub := C.CString(subject)
	cPol := C.CString(policy)
	defer C.free(unsafe.Pointer(cSub))
	defer C.free(unsafe.Pointer(cPol))

	var cTransfer *C.char
	if transferToSubject != "" {
		cTransfer = C.CString(transferToSubject)
		defer C.free(unsafe.Pointer(cTransfer))
	} else {
		cTransfer = nil
	}

	res := C.aegis_engine_delete_subject_with_policy(e.ptr, cSub, cPol, cTransfer)
	if res.error != nil {
		defer C.aegis_free_string(res.error)
		return WriteResult{}, errors.New(C.GoString(res.error))
	}
	return WriteResult{Revision: uint64(res.revision)}, nil
}

// QueryAudit returns audit entries for an object.
func (e *Engine) QueryAudit(object string, fromRevision, toRevision int64, limit uint64) ([]AuditEntry, error) {
	cObj := C.CString(object)
	defer C.free(unsafe.Pointer(cObj))

	res := C.aegis_engine_query_audit(e.ptr, cObj, C.int64_t(fromRevision), C.int64_t(toRevision), C.uint64_t(limit))
	if res.error != nil {
		defer C.aegis_free_string(res.error)
		return nil, errors.New(C.GoString(res.error))
	}

	var entries []AuditEntry
	if res.entries_json != nil {
		defer C.aegis_free_string(res.entries_json)
		json.Unmarshal([]byte(C.GoString(res.entries_json)), &entries)
	}
	return entries, nil
}

// ReloadSchema hot-reloads the schema at runtime.
func (e *Engine) ReloadSchema(schemaYAML string) error {
	cYAML := C.CString(schemaYAML)
	defer C.free(unsafe.Pointer(cYAML))

	res := C.aegis_engine_reload_schema(e.ptr, cYAML)
	if res != nil {
		defer C.aegis_free_string(res)
		return errors.New(C.GoString(res))
	}
	return nil
}

// Query returns paginated tuples matching a filter.
func (e *Engine) Query(filter QueryFilter, limit, cursorOffset uint64) (PaginatedTuples, error) {
	filterJSON, err := json.Marshal(filter)
	if err != nil {
		return PaginatedTuples{}, err
	}
	cFilter := C.CString(string(filterJSON))
	defer C.free(unsafe.Pointer(cFilter))

	res := C.aegis_engine_query(e.ptr, cFilter, C.uint64_t(limit), C.uint64_t(cursorOffset))
	if res.error != nil {
		defer C.aegis_free_string(res.error)
		return PaginatedTuples{}, errors.New(C.GoString(res.error))
	}

	var result PaginatedTuples
	if res.tuples_json != nil {
		defer C.aegis_free_string(res.tuples_json)
		json.Unmarshal([]byte(C.GoString(res.tuples_json)), &result.Tuples)
	}
	result.NextCursor = uint64(res.next_cursor)
	result.Revision = uint64(res.revision)
	return result, nil
}

// ── Rate limiter ──

// RateLimitConfig configures the engine's token bucket rate limiter.
type RateLimitConfig struct {
	ChecksPerSecond    *uint32 `json:"checks_per_second,omitempty"`
	CheckBurst         *uint32 `json:"check_burst,omitempty"`
	WritesPerSecond    *uint32 `json:"writes_per_second,omitempty"`
	WriteBurst         *uint32 `json:"write_burst,omitempty"`
	MaxTraversalDepth  *uint   `json:"max_traversal_depth,omitempty"`
	MaxTraversalVisits *uint   `json:"max_traversal_visits,omitempty"`
	MaxKeys            *uint   `json:"max_keys,omitempty"`
}

// SetRateLimiter replaces the rate limiter with a new configuration.
func (e *Engine) SetRateLimiter(cfg RateLimitConfig) error {
	data, err := json.Marshal(cfg)
	if err != nil {
		return err
	}
	cJSON := C.CString(string(data))
	defer C.free(unsafe.Pointer(cJSON))

	res := C.aegis_engine_set_rate_limiter(e.ptr, cJSON)
	if res != nil {
		defer C.aegis_free_string(res)
		return errors.New(C.GoString(res))
	}
	return nil
}

// ── Watch ──

// WatchEvent holds a single watch event.
type WatchEvent struct {
	EventType int
	Subject   string
	Relation  string
	Object    string
	Revision  uint64
	Timestamp string
}

// WatchSubscription represents a subscription to engine change events.
type WatchSubscription struct {
	ptr *C.AegisWatchSubscription
}

// Watch subscribes to engine change events with optional filters.
// Pass empty string for any filter you don't want to apply.
func (e *Engine) Watch(subjectType, relation, objectType string) *WatchSubscription {
	var cSub *C.char
	if subjectType != "" {
		cSub = C.CString(subjectType)
		defer C.free(unsafe.Pointer(cSub))
	}
	var cRel *C.char
	if relation != "" {
		cRel = C.CString(relation)
		defer C.free(unsafe.Pointer(cRel))
	}
	var cObj *C.char
	if objectType != "" {
		cObj = C.CString(objectType)
		defer C.free(unsafe.Pointer(cObj))
	}

	ptr := C.aegis_engine_watch(e.ptr, cSub, cRel, cObj)
	if ptr == nil {
		return nil
	}
	return &WatchSubscription{ptr: ptr}
}

// Poll returns the next watch event, or nil if none available.
func (s *WatchSubscription) Poll() *WatchEvent {
	evt := C.aegis_watch_poll(s.ptr)
	if evt == nil {
		return nil
	}
	defer C.aegis_watch_event_free(evt)

	subject := ""
	if evt.subject != nil {
		subject = C.GoString(evt.subject)
	}
	relation := ""
	if evt.relation != nil {
		relation = C.GoString(evt.relation)
	}
	object := ""
	if evt.object != nil {
		object = C.GoString(evt.object)
	}
	timestamp := ""
	if evt.timestamp != nil {
		timestamp = C.GoString(evt.timestamp)
	}

	return &WatchEvent{
		EventType: int(evt.event_type),
		Subject:   subject,
		Relation:  relation,
		Object:    object,
		Revision:  uint64(evt.revision),
		Timestamp: timestamp,
	}
}

// Free releases the watch subscription.
func (s *WatchSubscription) Free() {
	if s.ptr != nil {
		C.aegis_watch_free(s.ptr)
		s.ptr = nil
	}
}

// ── Transaction ──

// Transaction represents a database transaction.
type Transaction struct {
	ptr *C.AegisTransaction
}

// BeginTransaction starts a new database transaction.
func (e *Engine) BeginTransaction() (*Transaction, error) {
	ptr := C.aegis_engine_transaction_begin(e.ptr)
	if ptr == nil {
		return nil, errors.New("aegis_engine_transaction_begin failed")
	}
	return &Transaction{ptr: ptr}, nil
}

func txnCheckErr(res *C.char) error {
	if res != nil {
		defer C.aegis_free_string(res)
		return errors.New(C.GoString(res))
	}
	return nil
}

// Write writes a tuple within the transaction.
func (t *Transaction) Write(subject, relation, resource string) error {
	cSub := C.CString(subject)
	cRel := C.CString(relation)
	cRes := C.CString(resource)
	defer C.free(unsafe.Pointer(cSub))
	defer C.free(unsafe.Pointer(cRel))
	defer C.free(unsafe.Pointer(cRes))

	res := C.aegis_transaction_write(t.ptr, cSub, cRel, cRes)
	return txnCheckErr(res)
}

// Delete deletes a tuple within the transaction.
func (t *Transaction) Delete(subject, relation, resource string) error {
	cSub := C.CString(subject)
	cRel := C.CString(relation)
	cRes := C.CString(resource)
	defer C.free(unsafe.Pointer(cSub))
	defer C.free(unsafe.Pointer(cRel))
	defer C.free(unsafe.Pointer(cRes))

	res := C.aegis_transaction_delete(t.ptr, cSub, cRel, cRes)
	return txnCheckErr(res)
}

// Savepoint creates a named savepoint within the transaction.
func (t *Transaction) Savepoint(name string) error {
	cName := C.CString(name)
	defer C.free(unsafe.Pointer(cName))

	res := C.aegis_transaction_savepoint(t.ptr, cName)
	return txnCheckErr(res)
}

// RollbackToSavepoint rolls back to a named savepoint.
func (t *Transaction) RollbackToSavepoint(name string) error {
	cName := C.CString(name)
	defer C.free(unsafe.Pointer(cName))

	res := C.aegis_transaction_rollback_to_savepoint(t.ptr, cName)
	return txnCheckErr(res)
}

// ReleaseSavepoint releases a named savepoint.
func (t *Transaction) ReleaseSavepoint(name string) error {
	cName := C.CString(name)
	defer C.free(unsafe.Pointer(cName))

	res := C.aegis_transaction_release_savepoint(t.ptr, cName)
	return txnCheckErr(res)
}

// Commit commits the transaction.
func (t *Transaction) Commit() (WriteResult, error) {
	res := C.aegis_transaction_commit(t.ptr)
	if res.error != nil {
		defer C.aegis_free_string(res.error)
		return WriteResult{}, errors.New(C.GoString(res.error))
	}
	return WriteResult{Revision: uint64(res.revision)}, nil
}

// Rollback rolls back the transaction.
func (t *Transaction) Rollback() error {
	res := C.aegis_transaction_rollback(t.ptr)
	return txnCheckErr(res)
}

// Free releases the transaction handle (rolls back if not committed).
func (t *Transaction) Free() {
	if t.ptr != nil {
		C.aegis_transaction_free(t.ptr)
		t.ptr = nil
	}
}

// ── V7 Policy Draft ──

// CreatePolicyDraft creates a new policy draft.
func (e *Engine) CreatePolicyDraft(name, description string) (*PolicyDraft, error) {
	cName := C.CString(name)
	cDesc := C.CString(description)
	defer C.free(unsafe.Pointer(cName))
	defer C.free(unsafe.Pointer(cDesc))
	res := C.aegis_engine_create_policy_draft(e.ptr, cName, cDesc)
	if res == nil {
		return nil, errors.New("null result from create_policy_draft")
	}
	defer C.aegis_free_string(res)
	var draft PolicyDraft
	if err := json.Unmarshal([]byte(C.GoString(res)), &draft); err != nil {
		return nil, err
	}
	return &draft, nil
}

// UpdatePolicyDraft updates an existing policy draft's schema.
func (e *Engine) UpdatePolicyDraft(id, schemaJSON string) (*PolicyDraft, error) {
	cID := C.CString(id)
	cSchema := C.CString(schemaJSON)
	defer C.free(unsafe.Pointer(cID))
	defer C.free(unsafe.Pointer(cSchema))
	res := C.aegis_engine_update_policy_draft(e.ptr, cID, cSchema)
	if res == nil {
		return nil, errors.New("null result from update_policy_draft")
	}
	defer C.aegis_free_string(res)
	var draft PolicyDraft
	if err := json.Unmarshal([]byte(C.GoString(res)), &draft); err != nil {
		return nil, err
	}
	return &draft, nil
}

// ValidatePolicyDraft validates a policy draft's schema.
func (e *Engine) ValidatePolicyDraft(id string) (*ValidationReport, error) {
	cID := C.CString(id)
	defer C.free(unsafe.Pointer(cID))
	res := C.aegis_engine_validate_policy_draft(e.ptr, cID)
	if res == nil {
		return nil, errors.New("null result from validate_policy_draft")
	}
	defer C.aegis_free_string(res)
	var report ValidationReport
	if err := json.Unmarshal([]byte(C.GoString(res)), &report); err != nil {
		return nil, err
	}
	return &report, nil
}

// SubmitPolicyDraftForReview submits a policy draft for review.
func (e *Engine) SubmitPolicyDraftForReview(id string) (*PolicyDraft, error) {
	cID := C.CString(id)
	defer C.free(unsafe.Pointer(cID))
	res := C.aegis_engine_submit_policy_draft_for_review(e.ptr, cID)
	if res == nil {
		return nil, errors.New("null result from submit_policy_draft_for_review")
	}
	defer C.aegis_free_string(res)
	var draft PolicyDraft
	if err := json.Unmarshal([]byte(C.GoString(res)), &draft); err != nil {
		return nil, err
	}
	return &draft, nil
}

// ApprovePolicyDraft approves a policy draft.
func (e *Engine) ApprovePolicyDraft(id string) (*PolicyDraft, error) {
	cID := C.CString(id)
	defer C.free(unsafe.Pointer(cID))
	res := C.aegis_engine_approve_policy_draft(e.ptr, cID)
	if res == nil {
		return nil, errors.New("null result from approve_policy_draft")
	}
	defer C.aegis_free_string(res)
	var draft PolicyDraft
	if err := json.Unmarshal([]byte(C.GoString(res)), &draft); err != nil {
		return nil, err
	}
	return &draft, nil
}

// RejectPolicyDraft rejects a policy draft with a reason.
func (e *Engine) RejectPolicyDraft(id, reason string) (*PolicyDraft, error) {
	cID := C.CString(id)
	cReason := C.CString(reason)
	defer C.free(unsafe.Pointer(cID))
	defer C.free(unsafe.Pointer(cReason))
	res := C.aegis_engine_reject_policy_draft(e.ptr, cID, cReason)
	if res == nil {
		return nil, errors.New("null result from reject_policy_draft")
	}
	defer C.aegis_free_string(res)
	var draft PolicyDraft
	if err := json.Unmarshal([]byte(C.GoString(res)), &draft); err != nil {
		return nil, err
	}
	return &draft, nil
}

// PublishPolicyDraft publishes a policy draft as a new version.
func (e *Engine) PublishPolicyDraft(id string) (*PublishResult, error) {
	cID := C.CString(id)
	defer C.free(unsafe.Pointer(cID))
	res := C.aegis_engine_publish_policy_draft(e.ptr, cID)
	if res == nil {
		return nil, errors.New("null result from publish_policy_draft")
	}
	defer C.aegis_free_string(res)
	var result PublishResult
	if err := json.Unmarshal([]byte(C.GoString(res)), &result); err != nil {
		return nil, err
	}
	return &result, nil
}

// ArchivePolicyDraft archives a policy draft.
func (e *Engine) ArchivePolicyDraft(id string) (*PolicyDraft, error) {
	cID := C.CString(id)
	defer C.free(unsafe.Pointer(cID))
	res := C.aegis_engine_archive_policy_draft(e.ptr, cID)
	if res == nil {
		return nil, errors.New("null result from archive_policy_draft")
	}
	defer C.aegis_free_string(res)
	var draft PolicyDraft
	if err := json.Unmarshal([]byte(C.GoString(res)), &draft); err != nil {
		return nil, err
	}
	return &draft, nil
}

// ListPolicyDrafts returns all policy drafts.
func (e *Engine) ListPolicyDrafts() ([]PolicyDraft, error) {
	res := C.aegis_engine_list_policy_drafts(e.ptr)
	if res == nil {
		return nil, errors.New("null result from list_policy_drafts")
	}
	defer C.aegis_free_string(res)
	var drafts []PolicyDraft
	if err := json.Unmarshal([]byte(C.GoString(res)), &drafts); err != nil {
		return nil, err
	}
	return drafts, nil
}

// ── V7 Scheduler ──

// CreateAnalysisSchedule creates a new analysis schedule from JSON config.
func (e *Engine) CreateAnalysisSchedule(configJSON string) (*AnalysisSchedule, error) {
	cJSON := C.CString(configJSON)
	defer C.free(unsafe.Pointer(cJSON))
	res := C.aegis_engine_create_analysis_schedule(e.ptr, cJSON)
	if res == nil {
		return nil, errors.New("null result from create_analysis_schedule")
	}
	defer C.aegis_free_string(res)
	var schedule AnalysisSchedule
	if err := json.Unmarshal([]byte(C.GoString(res)), &schedule); err != nil {
		return nil, err
	}
	return &schedule, nil
}

// ListAnalysisSchedules returns all analysis schedules.
func (e *Engine) ListAnalysisSchedules() ([]AnalysisSchedule, error) {
	res := C.aegis_engine_list_analysis_schedules(e.ptr)
	if res == nil {
		return nil, errors.New("null result from list_analysis_schedules")
	}
	defer C.aegis_free_string(res)
	var schedules []AnalysisSchedule
	if err := json.Unmarshal([]byte(C.GoString(res)), &schedules); err != nil {
		return nil, err
	}
	return schedules, nil
}

// DeleteAnalysisSchedule deletes an analysis schedule by ID.
func (e *Engine) DeleteAnalysisSchedule(id string) error {
	cID := C.CString(id)
	defer C.free(unsafe.Pointer(cID))
	res := C.aegis_engine_delete_analysis_schedule(e.ptr, cID)
	if res != nil {
		defer C.aegis_free_string(res)
		return errors.New(C.GoString(res))
	}
	return nil
}

// RunAnalysisNow triggers an immediate analysis run.
func (e *Engine) RunAnalysisNow(scheduleID string) ([]AnalysisRun, error) {
	cID := C.CString(scheduleID)
	defer C.free(unsafe.Pointer(cID))
	res := C.aegis_engine_run_analysis_now(e.ptr, cID)
	if res == nil {
		return nil, errors.New("null result from run_analysis_now")
	}
	defer C.aegis_free_string(res)
	var runs []AnalysisRun
	if err := json.Unmarshal([]byte(C.GoString(res)), &runs); err != nil {
		return nil, err
	}
	return runs, nil
}

// GetAnalysisRuns returns recent analysis runs.
func (e *Engine) GetAnalysisRuns(limit int) ([]AnalysisRun, error) {
	res := C.aegis_engine_get_analysis_runs(e.ptr, C.int32_t(limit))
	if res == nil {
		return nil, errors.New("null result from get_analysis_runs")
	}
	defer C.aegis_free_string(res)
	var runs []AnalysisRun
	if err := json.Unmarshal([]byte(C.GoString(res)), &runs); err != nil {
		return nil, err
	}
	return runs, nil
}

// ── V7 Enforcement History ──

// SetEnforcementHistoryConfig sets the enforcement history sampling configuration.
func (e *Engine) SetEnforcementHistoryConfig(configJSON string) error {
	cJSON := C.CString(configJSON)
	defer C.free(unsafe.Pointer(cJSON))
	res := C.aegis_engine_set_enforcement_history_config(e.ptr, cJSON)
	if res != nil {
		defer C.aegis_free_string(res)
		return errors.New(C.GoString(res))
	}
	return nil
}

// GetEnforcementHistoryConfig returns the current enforcement history configuration.
func (e *Engine) GetEnforcementHistoryConfig() (*EnforcementHistoryConfig, error) {
	res := C.aegis_engine_get_enforcement_history_config(e.ptr)
	if res == nil {
		return nil, errors.New("null result from get_enforcement_history_config")
	}
	defer C.aegis_free_string(res)
	var cfg EnforcementHistoryConfig
	if err := json.Unmarshal([]byte(C.GoString(res)), &cfg); err != nil {
		return nil, err
	}
	return &cfg, nil
}

// EnforcementTrends returns enforcement activity trends.
func (e *Engine) EnforcementTrends(limit int) (*EnforcementTrends, error) {
	res := C.aegis_engine_enforcement_trends(e.ptr, C.int32_t(limit))
	if res == nil {
		return nil, errors.New("null result from enforcement_trends")
	}
	defer C.aegis_free_string(res)
	var trends EnforcementTrends
	if err := json.Unmarshal([]byte(C.GoString(res)), &trends); err != nil {
		return nil, err
	}
	return &trends, nil
}
