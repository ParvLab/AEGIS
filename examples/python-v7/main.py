"""Python example showing Aegis V7 features: policy lifecycle, scheduler, enforcement history, subscribe."""
import os
import sys
import json
import tempfile

try:
    import aegis
except ImportError:
    print("aegis module not found. Build with: cd crates/aegis-pyo3 && maturin develop")
    sys.exit(1)

SCHEMA = """
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
"""


def main():
    tmpdir = tempfile.mkdtemp()
    db_path = os.path.join(tmpdir, "example.db")

    engine = aegis.Aegis(db_path, SCHEMA)
    print("Engine initialized")

    # ── 1. Policy Lifecycle ──
    print("\n=== Policy Lifecycle ===")

    draft = json.loads(engine.create_policy_draft("v2-schema", "Upgrade to V2 schema"))
    print(f"Draft created: {draft['name']} | status: {draft['status']} | id: {draft['id']}")

    report = json.loads(engine.validate_policy_draft(draft['id']))
    print(f"Validation report - schema_valid: {report['schema_valid']}")

    submitted = json.loads(engine.submit_policy_draft_for_review(draft['id']))
    print(f"Submitted for review - status: {submitted['status']}")

    approved = json.loads(engine.approve_policy_draft(draft['id']))
    print(f"Approved - status: {approved['status']}")

    publish_result = json.loads(engine.publish_policy_draft(draft['id']))
    print(f"Published - policy_version: {publish_result['policy_version']}")

    drafts = json.loads(engine.list_policy_drafts())
    print(f"Drafts count: {len(drafts)}")

    # ── 2. Scheduler ──
    print("\n=== Scheduler ===")

    queries = json.dumps([
        {"subject": "user:alice", "permission": "read", "resource": "repo:acme"},
    ])
    schedule = json.loads(engine.create_analysis_schedule("hourly-review", 3600, queries))
    print(f"Schedule created: {schedule['name']} | id: {schedule['id']}")

    schedules = json.loads(engine.list_analysis_schedules())
    print(f"Schedules count: {len(schedules)}")

    runs = json.loads(engine.run_analysis_now(schedule['id']))
    print(f"Analysis runs: {len(runs)} | status: {runs[0]['status']}")

    recent_runs = json.loads(engine.get_analysis_runs(10))
    print(f"Recent runs count: {len(recent_runs)}")

    # ── 3. Enforcement History ──
    print("\n=== Enforcement History ===")

    config = json.dumps({
        "enabled": True,
        "sampling": "DeniedOnly",
        "max_events_per_minute": 10000,
        "max_rows": 100000,
        "max_days": 7,
    })
    engine.set_enforcement_history_config(config)

    engine.write("user:alice", "owner", "repo:acme")
    engine.write("user:bob", "viewer", "repo:acme")

    print(f"alice read repo:acme? {engine.check('user:alice', 'read', 'repo:acme').allowed}")
    print(f"bob admin repo:acme? {engine.check('user:bob', 'admin', 'repo:acme').allowed}")

    trends = json.loads(engine.enforcement_trends(100))
    print(f"Trends - total_events: {trends['total_events']} | denied: {trends['denied_count']} | allowed: {trends['allowed_count']}")

    # ── 4. Subscribe ──
    print("\n=== Subscribe ===")

    sub_result = json.loads(engine.subscribe(["TupleAdded", "PolicyVersionCreated"]))
    sub_id = sub_result.get("subscription_id")
    print(f"Subscribed to TupleAdded, PolicyVersionCreated - subscription_id: {sub_id}")

    engine.write("user:carol", "viewer", "repo:acme")
    print("Write triggered - events may be available via polling")

    print("\nAll V7 features demonstrated successfully.")


if __name__ == "__main__":
    main()
