package aegis

/*
#cgo LDFLAGS: -L../target/release -laegis_ffi
#include "../crates/aegis-ffi/src/aegis.h"
*/
import "C"
import (
	"context"
	"fmt"
	"runtime"
	"sync"
	"unsafe"
)

// Aegis is the main authorization engine handle.
type Aegis struct {
	ptr *C.AegisEngine
	mu  sync.Mutex
}

// Config for creating a new Aegis engine.
type Config struct {
	DBPath     string
	SchemaYAML string
}

// CheckResult from a permission check.
type CheckResult struct {
	Allowed  bool
	Revision uint64
}

// WriteResult from a write/delete operation.
type WriteResult struct {
	Revision uint64
}

// HealthReport contains engine health information.
type HealthReport struct {
	Healthy       bool
	Revision      uint64
	SchemaVersion int32
}

// New creates a new Aegis engine instance.
func New(config Config) (*Aegis, error) {
	cDB := C.CString(config.DBPath)
	cSchema := C.CString(config.SchemaYAML)
	defer C.free(unsafe.Pointer(cDB))
	defer C.free(unsafe.Pointer(cSchema))

	ptr := C.aegis_engine_create(cDB, cSchema)
	if ptr == nil {
		return nil, fmt.Errorf("aegis: failed to create engine")
	}

	a := &Aegis{ptr: ptr}
	runtime.SetFinalizer(a, func(a *Aegis) {
		a.Close()
	})
	return a, nil
}

// Check performs a permission check.
func (a *Aegis) Check(ctx context.Context, subject, permission, resource string) (*CheckResult, error) {
	a.mu.Lock()
	defer a.mu.Unlock()

	if a.ptr == nil {
		return nil, fmt.Errorf("aegis: engine is closed")
	}

	cSubject := C.CString(subject)
	cPermission := C.CString(permission)
	cResource := C.CString(resource)
	defer C.free(unsafe.Pointer(cSubject))
	defer C.free(unsafe.Pointer(cPermission))
	defer C.free(unsafe.Pointer(cResource))

	result := C.aegis_engine_check(a.ptr, cSubject, cPermission, cResource)
	if result.error != nil {
		errStr := C.GoString(result.error)
		C.aegis_free_string(result.error)
		return nil, fmt.Errorf("aegis: check failed: %s", errStr)
	}

	return &CheckResult{
		Allowed:  bool(result.allowed),
		Revision: uint64(result.revision),
	}, nil
}

// Write writes a relationship tuple.
func (a *Aegis) Write(ctx context.Context, subject, relation, resource string) (*WriteResult, error) {
	a.mu.Lock()
	defer a.mu.Unlock()

	if a.ptr == nil {
		return nil, fmt.Errorf("aegis: engine is closed")
	}

	cSubject := C.CString(subject)
	cRelation := C.CString(relation)
	cResource := C.CString(resource)
	defer C.free(unsafe.Pointer(cSubject))
	defer C.free(unsafe.Pointer(cRelation))
	defer C.free(unsafe.Pointer(cResource))

	result := C.aegis_engine_write(a.ptr, cSubject, cRelation, cResource)
	if result.error != nil {
		errStr := C.GoString(result.error)
		C.aegis_free_string(result.error)
		return nil, fmt.Errorf("aegis: write failed: %s", errStr)
	}

	return &WriteResult{
		Revision: uint64(result.revision),
	}, nil
}

// Delete removes a relationship tuple.
func (a *Aegis) Delete(ctx context.Context, subject, relation, resource string) (*WriteResult, error) {
	a.mu.Lock()
	defer a.mu.Unlock()

	if a.ptr == nil {
		return nil, fmt.Errorf("aegis: engine is closed")
	}

	cSubject := C.CString(subject)
	cRelation := C.CString(relation)
	cResource := C.CString(resource)
	defer C.free(unsafe.Pointer(cSubject))
	defer C.free(unsafe.Pointer(cRelation))
	defer C.free(unsafe.Pointer(cResource))

	result := C.aegis_engine_delete(a.ptr, cSubject, cRelation, cResource)
	if result.error != nil {
		errStr := C.GoString(result.error)
		C.aegis_free_string(result.error)
		return nil, fmt.Errorf("aegis: delete failed: %s", errStr)
	}

	return &WriteResult{
		Revision: uint64(result.revision),
	}, nil
}

// Health returns the engine health report.
func (a *Aegis) Health(ctx context.Context) (*HealthReport, error) {
	a.mu.Lock()
	defer a.mu.Unlock()

	if a.ptr == nil {
		return nil, fmt.Errorf("aegis: engine is closed")
	}

	result := C.aegis_engine_health(a.ptr)
	if result.error != nil {
		errStr := C.GoString(result.error)
		C.aegis_free_string(result.error)
		return nil, fmt.Errorf("aegis: health check failed: %s", errStr)
	}

	return &HealthReport{
		Healthy:       bool(result.healthy),
		Revision:      uint64(result.revision),
		SchemaVersion: int32(result.schema_version),
	}, nil
}

// Close shuts down the engine and frees resources.
func (a *Aegis) Close() error {
	a.mu.Lock()
	defer a.mu.Unlock()
	if a.ptr != nil {
		C.aegis_engine_destroy(a.ptr)
		a.ptr = nil
	}
	return nil
}
