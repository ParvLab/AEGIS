package aegis

import (
	"os"
	"testing"
)

const testSchema = `
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
      viewer: {}
    permissions:
      read:
        include:
          - owner
          - viewer
      write:
        include:
          - owner
`

func TestEngineLifecycle(t *testing.T) {
	dbPath := tempDB(t)
	eng, err := NewEngine(dbPath, testSchema)
	if err != nil {
		t.Fatalf("NewEngine failed: %v", err)
	}
	defer eng.Close()

	health := eng.Health()
	if !health.Healthy {
		t.Fatal("engine should be healthy")
	}
}

func TestWriteAndCheck(t *testing.T) {
	dbPath := tempDB(t)
	eng, err := NewEngine(dbPath, testSchema)
	if err != nil {
		t.Fatalf("NewEngine failed: %v", err)
	}
	defer eng.Close()

	wr, err := eng.Write("user:alice", "owner", "repo:acme")
	if err != nil {
		t.Fatalf("Write failed: %v", err)
	}
	if wr.Revision == 0 {
		t.Fatal("expected non-zero revision")
	}

	cr, err := eng.Check("user:alice", "read", "repo:acme")
	if err != nil {
		t.Fatalf("Check failed: %v", err)
	}
	if !cr.Allowed {
		t.Fatal("alice should be allowed to read repo:acme")
	}
	if cr.Revision == 0 {
		t.Fatal("expected non-zero revision in check result")
	}
}

func TestDelete(t *testing.T) {
	dbPath := tempDB(t)
	eng, err := NewEngine(dbPath, testSchema)
	if err != nil {
		t.Fatalf("NewEngine failed: %v", err)
	}
	defer eng.Close()

	eng.Write("user:alice", "owner", "repo:acme")
	eng.Delete("user:alice", "owner", "repo:acme")

	cr, err := eng.Check("user:alice", "read", "repo:acme")
	if err != nil {
		t.Fatalf("Check failed: %v", err)
	}
	if cr.Allowed {
		t.Fatal("alice should not be allowed after deletion")
	}
}

func TestExplain(t *testing.T) {
	dbPath := tempDB(t)
	eng, err := NewEngine(dbPath, testSchema)
	if err != nil {
		t.Fatalf("NewEngine failed: %v", err)
	}
	defer eng.Close()

	eng.Write("user:alice", "owner", "repo:acme")

	er, err := eng.Explain("user:alice", "read", "repo:acme")
	if err != nil {
		t.Fatalf("Explain failed: %v", err)
	}
	if !er.Allowed {
		t.Fatal("expected allowed")
	}
	if len(er.Trace) == 0 {
		t.Fatal("expected non-empty trace")
	}
}

func TestListByObject(t *testing.T) {
	dbPath := tempDB(t)
	eng, err := NewEngine(dbPath, testSchema)
	if err != nil {
		t.Fatalf("NewEngine failed: %v", err)
	}
	defer eng.Close()

	eng.Write("user:alice", "owner", "repo:acme")
	eng.Write("user:bob", "viewer", "repo:acme")

	tuples, err := eng.ListByObject("repo:acme", "")
	if err != nil {
		t.Fatalf("ListByObject failed: %v", err)
	}
	if len(tuples) != 2 {
		t.Fatalf("expected 2 tuples, got %d", len(tuples))
	}

	filtered, err := eng.ListByObject("repo:acme", "owner")
	if err != nil {
		t.Fatalf("ListByObject failed: %v", err)
	}
	if len(filtered) != 1 {
		t.Fatalf("expected 1 filtered tuple, got %d", len(filtered))
	}
}

func TestWriteBatch(t *testing.T) {
	dbPath := tempDB(t)
	eng, err := NewEngine(dbPath, testSchema)
	if err != nil {
		t.Fatalf("NewEngine failed: %v", err)
	}
	defer eng.Close()

	tuples := []Tuple{
		{Subject: "user:alice", Relation: "owner", Object: "repo:acme"},
		{Subject: "user:bob", Relation: "viewer", Object: "repo:acme"},
	}

	wr, err := eng.WriteBatch(tuples)
	if err != nil {
		t.Fatalf("WriteBatch failed: %v", err)
	}
	if wr.Revision == 0 {
		t.Fatal("expected non-zero revision")
	}

	cr, err := eng.Check("user:bob", "read", "repo:acme")
	if err != nil {
		t.Fatalf("Check failed: %v", err)
	}
	if !cr.Allowed {
		t.Fatal("bob should be allowed to read")
	}
}

func TestMigrate(t *testing.T) {
	dbPath := tempDB(t)
	eng, err := NewEngine(dbPath, testSchema)
	if err != nil {
		t.Fatalf("NewEngine failed: %v", err)
	}
	defer eng.Close()

	if err := eng.Migrate(1); err != nil {
		t.Fatalf("Migrate failed: %v", err)
	}
}

func TestDeleteObject(t *testing.T) {
	dbPath := tempDB(t)
	eng, err := NewEngine(dbPath, testSchema)
	if err != nil {
		t.Fatalf("NewEngine failed: %v", err)
	}
	defer eng.Close()

	eng.Write("user:alice", "owner", "repo:acme")
	eng.Write("user:bob", "viewer", "repo:acme")

	eng.DeleteObject("repo:acme")

	tuples, err := eng.ListByObject("repo:acme", "")
	if err != nil {
		t.Fatalf("ListByObject failed: %v", err)
	}
	if len(tuples) != 0 {
		t.Fatalf("expected 0 tuples after delete, got %d", len(tuples))
	}
}

func tempDB(t *testing.T) string {
	t.Helper()
	f, err := os.CreateTemp("", "aegis_test_*.db")
	if err != nil {
		t.Fatalf("temp file: %v", err)
	}
	f.Close()
	t.Cleanup(func() { os.Remove(f.Name()) })
	return f.Name()
}
