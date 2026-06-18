# V7 Roadmap: Operational Intelligence

> **Governing principle:** Aegis generates intelligence and events. Applications decide transport, visualization, alerting, and consumption.

## Never Build in Aegis

- WebSocket/SSE servers
- Message queues (Kafka, RabbitMQ, NATS)
- Durable delivery / retry queues
- Distributed scheduling
- Dashboards or UI
- External infrastructure integrations

## V7-M4: Policy Lifecycle (highest ROI, lowest risk)

Reuses V6's `access_diff()`, `simulate_changes()`, `rollback_policy()` as the engine under a workflow layer.

### Status
`Drafting` → `validate()` → `UnderReview` → `approve()` → `Approved` → `publish()` → `Published`
                                                                            → `archive()` → `Archived`
                                                    → `reject()` → `Rejected` → `Archived`

### Types
- `DraftStatus { Drafting, UnderReview, Approved, Published, Rejected, Superseded, Archived }`
- `PolicyDraft { id, name, description, schema, base_version, status, timestamps, created_by, approved_by }`
- `ValidationReport { schema_valid, access_diff_summary, simulation_summary, warnings }`
- `PublishResult { policy_version, access_diff, simulation }`

### GraphEngine Methods
| Method | Description |
|---|---|
| `create_policy_draft(name, desc)` | Create new draft |
| `update_policy_draft(id, schema)` | Edit (only if Drafting) |
| `validate_policy_draft(id)` | Schema val + diff + simulation; no status change |
| `submit_for_review(id)` | Status → UnderReview |
| `approve_policy_draft(id, approver)` | Status → Approved |
| `publish_policy_draft(id)` | Calls `rollback_policy()`; emits `PolicyVersionCreated` |
| `reject_policy_draft(id, reason)` | Status → Rejected |
| `archive_policy_draft(id)` | Status → Archived |
| `list_policy_drafts(filter)` | Query by status, author, date |

### Storage
New table `_aegis_policy_drafts` with fields: id, name, description, schema_json, base_version, status, timestamps, created_by, approved_by.

## V7-M1: Scheduled Analysis

Reuses V6 `integrity_check()`, `analysis_report()`, `find_high_access_subjects()`, `tenant_leakage_detection()`.

### Types
- `AnalysisSeverity { Info, Warning, Critical }`
- `AnalysisRunStatus { Success, Failed, Partial }`
- `AnalysisFinding { severity, category, detail, resource, subject }`
- `AnalysisRun { id, run_type, status, findings, duration_ms, started_at }`
- `AnalysisSchedule { interval_secs, enabled_checks, severity_overrides }`

### GraphEngine Methods
| Method | Description |
|---|---|
| `schedule_analysis(config)` | `tokio::spawn` periodic loop |
| `clear_analysis_schedule()` | Cancel scheduled runs |
| `list_analysis_runs(filter, pagination)` | Query persisted runs |
| `run_analysis_now(categories)` | One-shot ad-hoc run |

### Events
No webhook_url in config. Findings emitted as `IntegrityFinding` / `AnalysisCompleted` events.

## V7-M2: Enforcement History (Sampled)

Opt-in sampled recording of `check()` decisions.

### Types
- `SamplingMode { None, SamplingRate(f64), ErrorsOnly, DeniedOnly }`
- `RetentionPolicy { MaxDays(u64), MaxRows(u64) }`
- `EnforcementConfig { sampling, max_events_per_minute, aggregation_window_minutes, retention }`

### Guardrails
1. Disabled by default
2. `DeniedOnly` recommended sampling mode
3. `MaxEventsPerMinute` hard cap (default 10,000)
4. `RetentionPolicy` periodic cleanup

## V7-M3: Event Stream API

New event types on existing `watch()` infrastructure. No new transport.

### New WatchEventType Variants
- `PolicyVersionCreated` (from M4 publish)
- `PolicyRolledBack`
- `IntegrityFinding` (from M1 analysis)
- `AnalysisCompleted` (from M1 analysis)
- `RateLimitWarning`

### GraphEngine Method
- `subscribe(events: &[WatchEventType]) -> WatchSubscription` — convenience wrapper over `watch()`

## Delivery Order

| Order | Phase | Rationale |
|---|---|---|
| 1 | V4 Partition Benchmark | Foundation risk |
| 2 | M4 Policy Lifecycle | Highest value, lowest risk |
| 3 | M1 Scheduled Analysis | Thin layer on V6 analysis |
| 4 | M2 Enforcement History | Storage risk, needs benchmarking |
| 5 | M3 Event Stream API | New event types tie M1/M2/M4 |

## Score

| Version | Score |
|---|---|
| V6 | 10/10 |
| **V7** | **9.8/10** |
