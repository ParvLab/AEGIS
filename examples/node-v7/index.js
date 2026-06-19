const { initialize } = require("@aegis-v/engine");
const path = require("path");
const fs = require("fs");

const SCHEMA = `
types:
  user: {}
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

function main() {
  const dbPath = path.join(fs.mkdtempSync("aegis-"), "example.db");
  const engine = initialize(dbPath, SCHEMA);

  console.log("Engine initialized:", engine.initializeResult());

  // ── 1. Policy Lifecycle ──
  console.log("\n=== Policy Lifecycle ===");

  // Create a draft
  const draftStr = engine.create_policy_draft("v2-schema", "Upgrade to V2 schema");
  const draft = JSON.parse(draftStr);
  console.log("Draft created:", draft.name, "| status:", draft.status, "| id:", draft.id);

  // Validate the draft
  const validateStr = engine.validate_policy_draft(draft.id);
  const report = JSON.parse(validateStr);
  console.log("Validation report - schema_valid:", report.schema_valid);

  // Submit for review
  const submittedStr = engine.submit_policy_draft_for_review(draft.id);
  const submitted = JSON.parse(submittedStr);
  console.log("Submitted for review - status:", submitted.status);

  // Approve
  const approvedStr = engine.approve_policy_draft(draft.id);
  const approved = JSON.parse(approvedStr);
  console.log("Approved - status:", approved.status);

  // Publish
  const publishStr = engine.publish_policy_draft(draft.id);
  const publishResult = JSON.parse(publishStr);
  console.log("Published - policy_version:", publishResult.policy_version);

  // List drafts
  const draftsStr = engine.list_policy_drafts();
  const drafts = JSON.parse(draftsStr);
  console.log("Drafts count:", drafts.length);

  // ── 2. Scheduler ──
  console.log("\n=== Scheduler ===");

  // Create a schedule
  const queries = JSON.stringify([
    { subject: "user:alice", permission: "read", resource: "repo:acme" },
  ]);
  const scheduleStr = engine.create_analysis_schedule("hourly-review", 3600, queries);
  const schedule = JSON.parse(scheduleStr);
  console.log("Schedule created:", schedule.name, "| id:", schedule.id);

  // List schedules
  const schedulesStr = engine.list_analysis_schedules();
  const schedules = JSON.parse(schedulesStr);
  console.log("Schedules count:", schedules.length);

  // Run analysis now
  const runsStr = engine.run_analysis_now(schedule.id);
  const runs = JSON.parse(runsStr);
  console.log("Analysis runs:", runs.length, "| status:", runs[0].status);

  // Get analysis runs
  const recentRunsStr = engine.get_analysis_runs(10);
  const recentRuns = JSON.parse(recentRunsStr);
  console.log("Recent runs count:", recentRuns.length);

  // ── 3. Enforcement History ──
  console.log("\n=== Enforcement History ===");

  // Enable enforcement history
  const config = JSON.stringify({
    enabled: true,
    sampling: "DeniedOnly",
    max_events_per_minute: 10000,
    max_rows: 100000,
    max_days: 7,
  });
  engine.set_enforcement_history_config(config);

  // Write some tuples and run checks to generate events
  engine.write("user:alice", "owner", "repo:acme");
  engine.write("user:bob", "viewer", "repo:acme");

  console.log("alice read repo:acme?", engine.check("user:alice", "read", "repo:acme").allowed);
  console.log("bob admin repo:acme?", engine.check("user:bob", "admin", "repo:acme").allowed);

  // Get enforcement trends
  const trendsStr = engine.enforcement_trends(100);
  const trends = JSON.parse(trendsStr);
  console.log("Trends - total_events:", trends.total_events, "| denied:", trends.denied_count, "| allowed:", trends.allowed_count);

  // ── 4. Subscribe ──
  console.log("\n=== Subscribe ===");

  const sub = engine.subscribe(["TupleAdded", "PolicyVersionCreated"]);
  console.log("Subscribed to TupleAdded, PolicyVersionCreated");

  // Trigger an event
  engine.write("user:carol", "viewer", "repo:acme");

  // Poll for the event
  const event = sub.poll();
  if (event) {
    console.log("Received event - type:", event.eventType, "| subject:", event.subject, "| relation:", event.relation);
  } else {
    console.log("No event available immediately");
  }

  sub.unsubscribe();
  console.log("Unsubscribed");

  console.log("\nAll V7 features demonstrated successfully.");
}

try {
  main();
} catch (err) {
  console.error("Error:", err.message);
  process.exit(1);
}
