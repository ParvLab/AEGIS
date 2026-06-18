#ifndef AEGIS_FFI_H
#define AEGIS_FFI_H

#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Opaque handle to the Aegis engine. */
typedef struct AegisEngine AegisEngine;

/* ── Result structs ── */

typedef struct {
    uint64_t revision;
    char*    error;   /* null on success, owned string on error */
} AegisWriteResult;

typedef struct {
    bool     allowed;
    uint64_t revision;
    char*    error;
} AegisCheckResult;

typedef struct {
    bool     healthy;
    uint64_t revision;
    int32_t  schema_version;
    char*    error;
} AegisHealthResult;

/* ── Engine lifecycle ── */

AegisEngine* aegis_engine_create(const char* db_path, const char* schema_yaml);

void aegis_engine_destroy(AegisEngine* engine);

/* ── Core operations ── */

AegisCheckResult aegis_engine_check(
    AegisEngine*     engine,
    const char*      subject,
    const char*      permission,
    const char*      resource
);

AegisWriteResult aegis_engine_write(
    AegisEngine* engine,
    const char*  subject,
    const char*  relation,
    const char*  resource
);

AegisWriteResult aegis_engine_delete(
    AegisEngine* engine,
    const char*  subject,
    const char*  relation,
    const char*  resource
);

AegisHealthResult aegis_engine_health(AegisEngine* engine);

/* ── Memory management ── */

void aegis_free_string(char* s);

#ifdef __cplusplus
}
#endif

#endif /* AEGIS_FFI_H */
