const { initialize } = require("@aegis-v/engine");
const path = require("path");
const fs = require("fs");

// Schema demonstrating V2.5 features: subject-set, role hierarchy, conditional grants
const SCHEMA = `
types:
  user: {}
  team:
    relations:
      member: {}
      admin: {}
    permissions:
      view:
        include:
          - member
          - admin
      manage:
        include:
          - admin
  repo:
    relations:
      owner: {}
      maintainer: {}
      viewer: {}
    permissions:
      read:
        include:
          - owner
          - maintainer
          - viewer
      write:
        include:
          - maintainer
          - owner
      admin:
        include:
          - owner
`;

async function main() {
  const dbPath = path.join(fs.mkdtempSync("aegis-"), "example.db");
  const engine = initialize(dbPath, SCHEMA);

  console.log("Engine initialized:", engine.initializeResult());

  // ── 1. Direct tuple grants (basic V1) ──
  console.log("\n=== Direct Grants ===");
  engine.write("user:alice", "owner", "repo:acme");
  engine.write("user:bob", "viewer", "repo:acme");

  console.log("alice read repo:acme?", engine.check("user:alice", "read", "repo:acme").allowed);
  console.log("bob read repo:acme?", engine.check("user:bob", "read", "repo:acme").allowed);
  console.log("bob write repo:acme?", engine.check("user:bob", "write", "repo:acme").allowed);

  // ── 2. Role hierarchy ──
  console.log("\n=== Role Hierarchy ===");
  // team:eng#admin inherits team:eng#member
  engine.write("team:eng", "admin", "team:eng");
  engine.write("user:carol", "member", "team:eng");

  // carol is member of team:eng, so she has view permission
  console.log("carol view team:eng?", engine.check("user:carol", "view", "team:eng").allowed);

  // ── 3. Subject-set resolution ──
  console.log("\n=== Subject-Set ===");
  // team:eng#member acts as owner of repo:infra
  engine.write("team:eng#member", "owner", "repo:infra");

  // carol is member of team:eng, resolves through subject-set
  console.log("carol read repo:infra?", engine.check("user:carol", "read", "repo:infra").allowed);
  console.log("carol admin repo:infra?", engine.check("user:carol", "admin", "repo:infra").allowed);

  // ── 4. Explain trace ──
  console.log("\n=== Explain ===");
  const explain = engine.explain("user:carol", "read", "repo:infra");
  console.log("Trace:", JSON.stringify(explain.trace, null, 2));
  console.log("Resolved via:", explain.resolvedVia);
  console.log("Duration:", explain.durationMs, "ms");

  // ── 5. List by object/subject ──
  console.log("\n=== Listing ===");
  console.log("Tuples for repo:infra:", engine.listByObject("repo:infra"));
  console.log("Tuples for user:carol:", engine.listBySubject("user:carol"));
}

main().catch(console.error);
