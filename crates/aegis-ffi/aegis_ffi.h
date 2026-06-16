#ifndef AEGIS_FFI_H
#define AEGIS_FFI_H

#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ── Result structs ── */

typedef struct {
    uint64_t revision;
    char* error;
} AegisWriteResult;

typedef struct {
    bool allowed;
    uint64_t revision;
    char* error;
} AegisCheckResult;

typedef struct {
    bool healthy;
    uint64_t revision;
    int32_t schema_version;
    char* error;
} AegisHealthResult;

typedef struct {
    bool allowed;
    uint64_t revision;
    char* trace_json;
    char* resolved_via;
    uint64_t duration_ms;
    char* error;
} AegisExplainResult;

typedef struct {
    char* tuples_json;
    char* error;
} AegisListResult;

typedef struct {
    char* tuples_json;
    uint64_t export_revision;
    char* error;
} AegisExportResult;

typedef struct {
    char* entries_json;
    char* error;
} AegisAuditResult;

typedef struct {
    char* tuples_json;
    uint64_t next_cursor;
    uint64_t revision;
    char* error;
} AegisQueryResult;

/* ── Opaque handles ── */
typedef struct AegisEngine AegisEngine;
typedef struct AegisWatchSubscription AegisWatchSubscription;
typedef struct AegisTransaction AegisTransaction;

/* ── Lifecycle ── */
AegisEngine* aegis_engine_create(const char* db_path, const char* schema_yaml);
AegisEngine* aegis_engine_create_with_config(const char* db_path, const char* schema_yaml, const char* config_json);
void aegis_engine_destroy(AegisEngine* engine);
char* aegis_engine_close(AegisEngine* engine);
bool aegis_engine_is_closed(const AegisEngine* engine);

/* ── Core operations ── */
AegisCheckResult aegis_engine_check(AegisEngine* engine, const char* subject, const char* permission, const char* resource);
AegisWriteResult aegis_engine_write(AegisEngine* engine, const char* subject, const char* relation, const char* resource);
AegisWriteResult aegis_engine_delete(AegisEngine* engine, const char* subject, const char* relation, const char* resource);
AegisHealthResult aegis_engine_health(AegisEngine* engine);

/* ── Extended check operations ── */
AegisCheckResult aegis_engine_check_dry_run(AegisEngine* engine, const char* subject, const char* permission, const char* resource);
AegisCheckResult aegis_engine_check_ex(AegisEngine* engine, const char* subject, const char* permission, const char* resource, int32_t consistency);
AegisCheckResult aegis_engine_check_with_context(AegisEngine* engine, const char* subject, const char* permission, const char* resource, const char* context_json, int32_t consistency);

/* ── Extended write operations ── */
AegisWriteResult aegis_engine_write_ex(AegisEngine* engine, const char* subject, const char* relation, const char* resource, const char* condition, const char* metadata_json, const char* valid_until);
AegisCheckResult aegis_engine_write_dry_run(AegisEngine* engine, const char* subject, const char* relation, const char* resource);

/* ── Extended read operations ── */
AegisExplainResult aegis_engine_explain(AegisEngine* engine, const char* subject, const char* permission, const char* resource);
AegisListResult aegis_engine_list_by_object(AegisEngine* engine, const char* object, const char* relation);
AegisListResult aegis_engine_list_by_subject(AegisEngine* engine, const char* subject, const char* relation);
AegisListResult aegis_engine_list_by_relation(AegisEngine* engine, const char* object, const char* relation);
AegisQueryResult aegis_engine_query(AegisEngine* engine, const char* filter_json, uint64_t limit, uint64_t cursor_offset);

/* ── Batch & migration ── */
AegisWriteResult aegis_engine_write_batch(AegisEngine* engine, const char* tuples_json);
char* aegis_engine_migrate(AegisEngine* engine, int32_t target_version);
AegisWriteResult aegis_engine_delete_object(AegisEngine* engine, const char* object);
char* aegis_engine_check_schema(AegisEngine* engine, const char* schema_yaml);
char* aegis_engine_reload_schema(AegisEngine* engine, const char* schema_yaml);

/* ── GDPR ── */
AegisExportResult aegis_engine_export_subject(AegisEngine* engine, const char* subject);
AegisWriteResult aegis_engine_delete_subject_with_policy(AegisEngine* engine, const char* subject, const char* policy, const char* transfer_to_subject);

/* ── Audit ── */
AegisAuditResult aegis_engine_query_audit(AegisEngine* engine, const char* object, int64_t from_revision, int64_t to_revision, uint64_t limit);

/* ── Watch ── */
typedef struct {
    int32_t event_type;
    char* subject;
    char* relation;
    char* object;
    uint64_t revision;
    char* timestamp;
    char* error;
} AegisWatchEvent;

AegisWatchSubscription* aegis_engine_watch(AegisEngine* engine, const char* subject_type, const char* relation, const char* object_type);
AegisWatchEvent* aegis_watch_poll(AegisWatchSubscription* sub);
void aegis_watch_free(AegisWatchSubscription* sub);
void aegis_watch_event_free(AegisWatchEvent* evt);

/* ── Transaction ── */
AegisTransaction* aegis_engine_transaction_begin(AegisEngine* engine);
char* aegis_transaction_write(AegisTransaction* txn, const char* subject, const char* relation, const char* resource);
char* aegis_transaction_delete(AegisTransaction* txn, const char* subject, const char* relation, const char* resource);
char* aegis_transaction_savepoint(AegisTransaction* txn, const char* name);
char* aegis_transaction_rollback_to_savepoint(AegisTransaction* txn, const char* name);
char* aegis_transaction_release_savepoint(AegisTransaction* txn, const char* name);
AegisWriteResult aegis_transaction_commit(AegisTransaction* txn);
char* aegis_transaction_rollback(AegisTransaction* txn);
void aegis_transaction_free(AegisTransaction* txn);

/* ── Actor identity ── */
char* aegis_engine_set_actor(AegisEngine* engine, const char* actor);
char* aegis_engine_active_actor(const AegisEngine* engine);

/* ── Logger ── */
typedef void (*AegisLogFn)(int level, const char* target, const char* msg, void* user_data);
void aegis_engine_set_logger(AegisEngine* engine, AegisLogFn callback, void* user_data);

/* ── Rate limiter ── */
char* aegis_engine_set_rate_limiter(AegisEngine* engine, const char* config_json);

/* ── Memory management ── */
void aegis_free_string(char* s);

/* Consistency mode constants for aegis_engine_check_ex / aegis_engine_check_with_context:
   -1 = default (eventual consistency)
    0 = minimize latency
    1 = fully consistent
    >= 2 = at revision (value passed as revision number)
*/

#ifdef __cplusplus
}
#endif

#endif /* AEGIS_FFI_H */
