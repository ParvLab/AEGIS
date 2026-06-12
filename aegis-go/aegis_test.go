package aegis

import (
	"context"
	"os"
	"testing"
)

const testSchema = `
types:
  user:
    relations:
      owner: {}
      member: {}
  workspace:
    relations:
      owner: { inherit: { from: member } }
      member: {}
`

func TestNewAndHealth(t *testing.T) {
	dir, err := os.MkdirTemp("", "aegis-test-*")
	if err != nil {
		t.Fatal(err)
	}
	defer os.RemoveAll(dir)

	engine, err := New(Config{
		DBPath:     dir + "/test.db",
		SchemaYAML: testSchema,
	})
	if err != nil {
		t.Fatalf("failed to create engine: %v", err)
	}
	defer engine.Close()

	health, err := engine.Health(context.Background())
	if err != nil {
		t.Fatalf("health check failed: %v", err)
	}
	if !health.Healthy {
		t.Fatal("expected engine to be healthy")
	}
	if health.SchemaVersion == 0 {
		t.Fatal("expected non-zero schema version")
	}
}

func TestCheckDeny(t *testing.T) {
	dir, err := os.MkdirTemp("", "aegis-test-*")
	if err != nil {
		t.Fatal(err)
	}
	defer os.RemoveAll(dir)

	engine, err := New(Config{
		DBPath:     dir + "/test.db",
		SchemaYAML: testSchema,
	})
	if err != nil {
		t.Fatalf("failed to create engine: %v", err)
	}
	defer engine.Close()

	// No tuples written yet — check should deny
	result, err := engine.Check(context.Background(), "user:alice", "owner", "workspace:acme")
	if err != nil {
		t.Fatalf("check failed: %v", err)
	}
	if result.Allowed {
		t.Fatal("expected check to deny (no tuples written)")
	}
	if result.Revision == 0 {
		t.Fatal("expected non-zero revision")
	}
}

func TestWriteAndCheck(t *testing.T) {
	dir, err := os.MkdirTemp("", "aegis-test-*")
	if err != nil {
		t.Fatal(err)
	}
	defer os.RemoveAll(dir)

	engine, err := New(Config{
		DBPath:     dir + "/test.db",
		SchemaYAML: testSchema,
	})
	if err != nil {
		t.Fatalf("failed to create engine: %v", err)
	}
	defer engine.Close()

	// Write owner tuple
	writeResult, err := engine.Write(context.Background(), "user:alice", "owner", "workspace:acme")
	if err != nil {
		t.Fatalf("write failed: %v", err)
	}
	if writeResult.Revision == 0 {
		t.Fatal("expected non-zero revision from write")
	}

	// Check owner — should be allowed
	checkResult, err := engine.Check(context.Background(), "user:alice", "owner", "workspace:acme")
	if err != nil {
		t.Fatalf("check failed: %v", err)
	}
	if !checkResult.Allowed {
		t.Fatal("expected check to allow after write")
	}
	if checkResult.Revision == 0 {
		t.Fatal("expected non-zero revision from check")
	}

	// Check non-existent user — should deny
	denyResult, err := engine.Check(context.Background(), "user:bob", "owner", "workspace:acme")
	if err != nil {
		t.Fatalf("check failed: %v", err)
	}
	if denyResult.Allowed {
		t.Fatal("expected check to deny for non-owner")
	}

	// Delete the tuple
	delResult, err := engine.Delete(context.Background(), "user:alice", "owner", "workspace:acme")
	if err != nil {
		t.Fatalf("delete failed: %v", err)
	}
	if delResult.Revision == 0 {
		t.Fatal("expected non-zero revision from delete")
	}

	// Check after delete — should deny
	afterDelete, err := engine.Check(context.Background(), "user:alice", "owner", "workspace:acme")
	if err != nil {
		t.Fatalf("check after delete failed: %v", err)
	}
	if afterDelete.Allowed {
		t.Fatal("expected check to deny after delete")
	}
}

func TestCloseIsIdempotent(t *testing.T) {
	dir, err := os.MkdirTemp("", "aegis-test-*")
	if err != nil {
		t.Fatal(err)
	}
	defer os.RemoveAll(dir)

	engine, err := New(Config{
		DBPath:     dir + "/test.db",
		SchemaYAML: testSchema,
	})
	if err != nil {
		t.Fatalf("failed to create engine: %v", err)
	}

	// Close multiple times should not panic
	if err := engine.Close(); err != nil {
		t.Fatalf("first close failed: %v", err)
	}
	if err := engine.Close(); err != nil {
		t.Fatalf("second close should be idempotent: %v", err)
	}
}
