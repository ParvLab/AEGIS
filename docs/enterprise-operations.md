# Enterprise Operations Guide

## Backup & Restore
- Use `aegis backup create` for full state backup
- Use `aegis backup restore` for full restore
- Backup format v3 includes integrity checksums
- Verify regular backups with `aegis health`

## Audit & Compliance
- All mutations are logged to the audit event log
- Actor identity can be set via `engine.set_actor()`
- Audit events are hash-chained for tamper evidence
- Verify chain integrity with `engine.verify_audit_chain()`

## Monitoring
- Health endpoint provides: revision, cache hit rate, WAL size
- Integrity checks run automatically when configured
- Rate limiter metrics available via health report

## Upgrade Procedure
1. Backup current state: `aegis backup create`
2. Validate schema compatibility: `aegis schema lint --strict`
3. Apply migration if needed
4. Verify post-upgrade: `aegis health`
