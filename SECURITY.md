# Security

## Reporting a Vulnerability

Report security issues to the Aegis maintainers via email or by opening a
confidential GitHub issue.

## Penetration Testing Checklist

Before each GA release, verify the following:

### Input Fuzzing
- [ ] Tuple subject/relation/object fields accept only valid characters (alphanumeric, `:`, `_`, `-`, `/`)
- [ ] Metadata keys are ASCII alphanumeric, `_`, or `-` only
- [ ] Metadata values reject null bytes and control characters
- [ ] Identity fields enforce max length (256 chars)
- [ ] Tuple serialized size limit (64 KiB) enforced
- [ ] Pagination limit capped at 10,000

### Authorization Boundary
- [ ] `check()` returns false (never errors) on unknown subjects/permissions/resources
- [ ] `write()` rejects relations not defined in schema
- [ ] `delete()` is soft (revision-based) — tuples are never permanently removed
- [ ] Audit log is append-only — past entries cannot be modified
- [ ] Schema validation runs on every `write()`, not just dry-run

### Fail-Closed Behavior
- [ ] Storage connection loss returns `AegisError::StorageConnection` (not panic)
- [ ] Rate limiter exhaustion returns `AegisError::RateLimited` (not panic)
- [ ] Cache lock poison skips cache (does not panic)
- [ ] `startup_probe()` fails if storage/schema unavailable
- [ ] All engine operations check `is_closed()` before proceeding

### Cache Security
- [ ] Decision cache entries have a TTL (default 5 minutes)
- [ ] Cache entries invalidate on schema change
- [ ] Traversal cache cleared on schema change

### Operational Security
- [ ] No secrets (passwords, API keys, tokens) appear in logs or error messages
- [ ] `cargo audit` passes with no unaddressed vulnerabilities
- [ ] Engine has no default credentials — API key authentication is opt-in
- [ ] PostgreSQL connections support TLS via `sslmode` in connection string
