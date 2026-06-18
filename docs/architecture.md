# Architecture Principles

## Governing Principle

> **Aegis generates intelligence and events. Applications decide transport, visualization, alerting, and consumption.**

## What Aegis Is

An embedded authorization runtime that runs inside an application process. Permission checks are local function calls, not HTTP requests. Aegis manages:

- Relationships between identities and resources (ReBAC)
- Permission evaluation through graph traversal
- Policy definitions, inheritance, and versioning
- Authorization traces and access explanations
- Multi-tenant permission graphs
- Authorization intelligence (V6): analysis, diff, simulation, diagnostics
- Operational intelligence (V7): scheduled analysis, enforcement history, policy lifecycle, event streams

## What Aegis Does NOT Build

- **WebSocket/SSE servers** — Aegis produces events via `WatchSubscription`; applications own transport
- **Message queues** — No Kafka, RabbitMQ, NATS integration
- **Durable delivery / retry queues** — Events are best-effort via subscription
- **Distributed scheduling** — No cron, etcd, K8s CronJob integration
- **Dashboards or UI** — Aegis is embedded, not a service
- **External infrastructure integrations** — No PagerDuty, Slack, Discord, email

These belong to the application or infrastructure layer.

## Embedding Model

```
Application
  ├── Business Logic
  ├── Main Database
  └── Aegis Runtime (embedded)
       ├── Graph Engine (Rust core)
       ├── Policy Evaluator
       ├── Connection Manager
       └── Storage Adapter (SQLite / RocksDB / PG / MySQL / IndexedDB)
```

## Version History

| Version | Theme |
|---|---|
| V1-V3 | Authorization Engine |
| V4 | Enterprise Authorization |
| V5 | Browser / Offline Authorization |
| V6 | Authorization Intelligence |
| V7 | Operational Intelligence |
