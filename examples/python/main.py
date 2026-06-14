"""Python example showing Aegis V2.5 authorization features via PyO3 bindings."""
import os
import tempfile
import sys

# Assuming aegis-pyo3 is installed or on PYTHONPATH
try:
    import aegis
except ImportError:
    print("aegis module not found. Build with: cd crates/aegis-pyo3 && maturin develop")
    sys.exit(1)

SCHEMA = """
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
"""


def main():
    tmpdir = tempfile.mkdtemp()
    db_path = os.path.join(tmpdir, "example.db")

    engine = aegis.Aegis(db_path, SCHEMA)
    print("Engine initialized")

    # 1. Direct grants
    print("\n=== Direct Grants ===")
    engine.write("user:alice", "owner", "repo:acme")
    engine.write("user:bob", "viewer", "repo:acme")

    print(f"alice read repo:acme? {engine.check('user:alice', 'read', 'repo:acme')}")
    print(f"bob read repo:acme?  {engine.check('user:bob', 'read', 'repo:acme')}")
    print(f"bob write repo:acme? {engine.check('user:bob', 'write', 'repo:acme')}")

    # 2. Role hierarchy
    print("\n=== Role Hierarchy ===")
    engine.write("team:eng", "admin", "team:eng")
    engine.write("user:carol", "member", "team:eng")
    print(f"carol view team:eng? {engine.check('user:carol', 'view', 'team:eng')}")

    # 3. Subject-set
    print("\n=== Subject-Set ===")
    engine.write("team:eng#member", "owner", "repo:infra")
    print(f"carol read repo:infra?  {engine.check('user:carol', 'read', 'repo:infra')}")
    print(f"carol admin repo:infra? {engine.check('user:carol', 'admin', 'repo:infra')}")

    # 4. Explain
    print("\n=== Explain ===")
    result = engine.explain("user:carol", "read", "repo:infra")
    print(f"Allowed: {result.allowed}")
    print(f"Trace: {result.trace}")
    print(f"Resolved via: {result.resolved_via}")

    # 5. Batch write
    print("\n=== Batch Write ===")
    engine.write_batch([
        ("user:dave", "viewer", "repo:acme"),
        ("user:eve", "viewer", "repo:acme"),
    ])
    print(f"repo:acme tuples: {engine.list_by_object('repo:acme')}")

    # 6. Explain on denied check
    print("\n=== Denied Explain ===")
    result = engine.explain("user:bob", "admin", "repo:acme")
    print(f"bob admin repo:acme? {result.allowed}")
    print(f"Trace: {result.trace}")


if __name__ == "__main__":
    main()
