# AEGIS ReBAC Demo — GitHub-Style Access Control

An interactive Next.js demo showing AEGIS authorization engine features through a realistic GitHub-org-style ReBAC scenario.

## Quick Start

```bash
cd examples/github-rebac
npm install
npm run dev
```

Open [http://localhost:3000](http://localhost:3000).

Click **Seed Minimal** on the Dashboard to populate the access graph, then explore.

## What It Demonstrates

| Feature | Page | Description |
|---------|------|-------------|
| V1 Core | Check | Role hierarchy, subject-set resolution |
| V2 Explain | Explain | Full resolution trace with deny-override highlighting |
| V4 Multi-tenant | (schema) | Org/team isolation in a single engine |
| V6 Intelligence | Who Can Access | Reverse permission lookup |
| V6 Simulation | Simulation | What-if dry-run analysis |
| V6 Graph | Explorer | Interactive force-directed access graph |
| V7 Policies | Policies | Draft → Approve → Publish lifecycle |

## Schema

Three types with GitHub-inspired permissions:

- **Org** — member, admin — view/manage
- **Team** — member, maintainer, admin — pull/push/admin
- **Repo** — viewer, maintainer, admin, banned — pull/push/admin with deny override

## Story

1. **Acme Corp** is set up with org, teams (engineering, security), and repos (payment-api, docs)
2. **Alice** (admin) and **Bob** (member) are in engineering — Alice can push, Bob can only pull
3. **Carol** is in security — can view payment-api but not push
4. **Mallory** is a contractor — you can **ban** her from repos to demonstrate deny-override
5. Use **Explain** to see the full trace showing how deny rules override allow paths
6. Use **Graph Explorer** to visualize all relationships interactively

## Built With

- [Next.js 16](https://nextjs.org) (App Router)
- [Tailwind CSS v4](https://tailwindcss.com)
- [react-force-graph-2d](https://github.com/vasturiano/react-force-graph)
- [@aegis-v/engine](https://github.com/ParvLab/AEGIS)
