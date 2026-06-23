# AEGIS ReBAC Demo — GitHub-Style Access Control

An interactive [Next.js 16](https://nextjs.org) demo that exercises the full AEGIS authorization engine API through a realistic GitHub-org-style ReBAC scenario.

## Quick Start

```bash
cd examples/github-rebac
npm install
npm run dev
```

Open [http://localhost:3000](http://localhost:3000).

Click **Seed Minimal** on the Dashboard to populate the access graph, then explore.

> **Windows users:** `npm install` creates a Windows junction symlink for the `@aegis-v/engine` native package, which Turbopack cannot resolve. A `postinstall` script (`scripts/copy-native.js`) replaces it with real files automatically. If you add packages later, run `node scripts/copy-native.js` manually.

## Pages & Features

All 14 pages are listed in the sidebar under four sections:

### Authorization

| Page | Route | What It Shows |
|------|-------|---------------|
| **Dashboard** | `/` | Engine health, active connections, cache-hit rate, revision, tuple/ban counts. Clear cache & migrate version controls. |
| **Check** | `/check` | Role-hierarchy and subject-set permission checks with dry-run mode. |
| **Check w/ Context** | `/check-with-context` | Condition-based checks with key-value editors for subject meta, resource meta, and environment context. Supports dry-run. |
| **Explain** | `/explain` | Full resolution trace (V1) and depth-indented per-step trace with cache-hit indicators (V2). |
| **Who Can Access** | `/who-can-access` | Reverse permission lookup with pagination (Prev/Next) and optional path inclusion. |
| **Simulation** | `/simulate` | Three modes: dry-run check, dry-run write, and access-diff (compare two schemas side-by-side). |

### Graph & Data

| Page | Route | What It Shows |
|------|-------|---------------|
| **Graph Explorer** | `/graph` | Interactive D3 force-directed graph of all tuples. Click nodes for details, double-click for who-can-access. |
| **Tuples** | `/tuples` | Five tabs: **Single** (write/delete/ban/unban/dry-run with condition, metadata, validUntil), **Batch** (JSON array write), **Query** (listByObject / listBySubject / listByRelation), **Delete Object**, **Transaction** (begin, write, savepoint, rollback, commit). |
| **Audit** | `/audit` | Revision-range audit log with object filter. Shows action, subject, relation, object, timestamp per revision. |
| **Export** | `/export` | Export all tuples for a subject as downloadable JSON. |

### Schema & Policies

| Page | Route | What It Shows |
|------|-------|---------------|
| **Schema Editor** | `/schema` | Monaco YAML editor with Validate and Apply buttons. Edit the ReBAC schema live. |
| **Policies** | `/policies` | Full V7 draft lifecycle: create → validate → submit → approve/reject → publish → archive. Rollback published versions. Edit draft schema inline. |

### V7 Advanced

| Page | Route | What It Shows |
|------|-------|---------------|
| **Scheduler** | `/scheduler` | Create analysis schedules (name, interval, queries JSON). List, run now, delete. View recent runs. |
| **Enforcement** | `/enforcement` | View/set enforcement history config. Trends dashboard with total/allowed/denied counts and recent events. |

## API Routes

18 API routes power the UI and are also usable directly:

| Route | Methods | Purpose |
|-------|---------|---------|
| `/api/health` | GET | Engine health, revision, cache-hit rate, connections |
| `/api/seed` | POST | Seed minimal (11 tuples) or full (21 tuples) data |
| `/api/reset` | POST | Reset database to initial schema |
| `/api/check` | POST | Permission check + dry-run |
| `/api/check-with-context` | POST | Condition-context check + dry-run |
| `/api/explain` | POST | V1 explain trace |
| `/api/explain-v2` | POST | V2 explain trace (per-step depth) |
| `/api/tuples` | POST | 10 actions: write, delete, ban, unban, dry-run-write, batch-write, delete-object, delete-subject-with-policy, list-by-object, list-by-subject |
| `/api/query` | POST | Advanced tuple query with filter and cursor pagination |
| `/api/who-can-access` | POST | Reverse lookup with pageOffset, pageLimit, includePaths |
| `/api/simulate` | POST | dry-run-check, dry-run-write, access-diff |
| `/api/graph` | GET | Full tuple list for graph visualization |
| `/api/list` | POST | List types (subjects, objects, relations, permissions) and tuples by relation |
| `/api/schema` | GET, POST | Read schema YAML, validate, apply |
| `/api/policies` | GET, POST | List drafts+versions, full lifecycle actions (10 actions) |
| `/api/audit` | POST | Audit trail with object filter and revision range |
| `/api/export` | POST | Export all tuples for a subject |
| `/api/cache` | POST | Invalidate cache, migrate to version |
| `/api/scheduler` | GET, POST | CRUD for analysis schedules + run now |
| `/api/enforcement` | GET, POST | Get/set config, get trends |
| `/api/events` | POST | Watch, subscribe, poll, unsubscribe |
| `/api/transaction` | POST | Begin, write, delete, savepoint, rollback-to-savepoint, release-savepoint, commit, rollback |

## Schema

Three types with GitHub-inspired permissions:

- **Org** — `member`, `admin` — `view`, `manage`
- **Team** — `member`, `maintainer`, `admin` — `pull`, `push`, `admin`
- **Repo** — `viewer`, `maintainer`, `admin`, `banned` — `pull`, `push`, `admin` with deny override

Relations use `inherit_from:` chains (e.g. `repo#admin` inherits `repo#maintainer`) and permissions use `union_of:` expressions.

## Story

1. **Acme Corp** is set up with org, teams (engineering, security), and repos (payment-api, docs)
2. **Alice** (admin) and **Bob** (member) are in engineering — Alice can push, Bob can only pull
3. **Carol** is in security — can view payment-api but not push
4. **Frank** is in engineering (added via batch write)
5. **Mallory** is a contractor — you can **ban** her from repos to demonstrate deny-override
6. Use **Explain** to see the full trace showing how deny rules override allow paths
7. Use **Graph Explorer** to visualize all relationships interactively
8. Use **Simulation → Access Diff** to see what changes when you modify the schema

## Build

```bash
npm run build
npm start
```

All 14 pages + 22 API routes compile and serve as server-rendered (SSR) or dynamic routes.

## Engine API Coverage

50 of 51 `JsAegis` methods are exercised (`isClosed()` is absent from the NAPI binary despite being present in `.d.ts`). This includes all V1/V2/V4/V6/V7 APIs: check, explain, who-can-access, query, audit, export, schema management, cache, migration, policies (full draft lifecycle), scheduler, enforcement, events (watch/subscribe/poll), and transactions (with savepoints).

## Built With

- [Next.js 16](https://nextjs.org) (App Router, Turbopack)
- [Tailwind CSS v4](https://tailwindcss.com)
- [react-force-graph-2d](https://github.com/vasturiano/react-force-graph)
- [@monaco-editor/react](https://github.com/suren-atoyan/monaco-react)
- [@aegis-v/engine](https://github.com/ParvLab/AEGIS) (via NAPI)
