# Enterprise Operations Guide

## Backup & Restore
- Use `aegis backup create` for full state backup
- Use `aegis backup restore` for full restore
- Backup format includes integrity checksums
- Verify regular backups with `aegis health`

## Audit & Compliance
- All mutations are logged to the audit event log
- Actor identity can be set via `engine.set_actor()`
- Audit events are SHA-256 hash-chained for tamper evidence
- Verify chain integrity with `engine.verify_audit_chain()`

## V6 Analysis Operations
- `engine.analysis_report()` — Full integrity scan across all categories
- `engine.tenant_leakage_detection()` — Cross-partition isolation verification
- `engine.find_orphaned_tuples()` — Dead tuple detection
- `engine.find_high_access_subjects()` — Privilege escalation detection
- `engine.who_can_access()` — Subject enumeration for resource+permission
- `engine.access_diff()` — Semantic policy diff for change review
- `engine.simulate_changes()` — What-if policy change simulation

## V7 Policy Lifecycle
- `engine.create_policy_draft()` → `validate()` → `submit_for_review()` → `approve()` → `publish()`
- Drafts are immutable once submitted; edits require status to be `Drafting`
- Publishing a draft creates a new policy version via `rollback_policy()`
- See `ROADMAP-V7.md` for workflow details

## V7 Scheduled Analysis
- `engine.schedule_analysis(config)` — Cron-like recurring integrity checks
- Findings emitted as events via `engine.subscribe()`
- No webhook/notification built into Aegis — applications subscribe and route

## Monitoring
- Health endpoint provides: revision, cache hit rate, WAL size
- Integrity checks run automatically when configured
- Rate limiter metrics available via health report
- Enforcement history (V7, opt-in) tracks check decisions with sampling

## Upgrade Procedure
1. Backup current state: `aegis backup create`
2. Validate schema compatibility: `aegis schema lint --strict`
3. Apply migration if needed
4. Verify post-upgrade: `aegis health`
