package main

import (
	"fmt"
	"os"
	"path/filepath"

	aegis "github.com/ParvLab/AEGIS/go"
)

const schema = `
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
`

func main() {
	tmpDir, err := os.MkdirTemp("", "aegis-example")
	if err != nil {
		panic(err)
	}
	defer os.RemoveAll(tmpDir)

	dbPath := filepath.Join(tmpDir, "example.db")
	eng, err := aegis.NewEngine(dbPath, schema)
	if err != nil {
		panic(err)
	}
	defer eng.Close()

	fmt.Println("Engine initialized")

	// 1. Direct grants
	fmt.Println("\n=== Direct Grants ===")
	must(eng.Write("user:alice", "owner", "repo:acme"))
	must(eng.Write("user:bob", "viewer", "repo:acme"))

	fmt.Printf("alice read repo:acme? %v\n", check(eng, "user:alice", "read", "repo:acme"))
	fmt.Printf("bob read repo:acme?  %v\n", check(eng, "user:bob", "read", "repo:acme"))

	// 2. Role hierarchy
	fmt.Println("\n=== Role Hierarchy ===")
	must(eng.Write("team:eng", "admin", "team:eng"))
	must(eng.Write("user:carol", "member", "team:eng"))
	fmt.Printf("carol view team:eng? %v\n", check(eng, "user:carol", "view", "team:eng"))

	// 3. Subject-set
	fmt.Println("\n=== Subject-Set ===")
	must(eng.Write("team:eng#member", "owner", "repo:infra"))
	fmt.Printf("carol read repo:infra?  %v\n", check(eng, "user:carol", "read", "repo:infra"))
	fmt.Printf("carol admin repo:infra? %v\n", check(eng, "user:carol", "admin", "repo:infra"))

	// 4. Explain
	fmt.Println("\n=== Explain ===")
	er, _ := eng.Explain("user:carol", "read", "repo:infra")
	fmt.Printf("Allowed: %v, Resolved via: %s, Duration: %dms\n", er.Allowed, er.ResolvedBy, er.DurationMs)
	for _, t := range er.Trace {
		fmt.Printf("  %s --%s--> %s\n", t.Subject, t.Relation, t.Object)
	}

	// 5. Batch write
	fmt.Println("\n=== Batch Write ===")
	must(eng.WriteBatch([]aegis.Tuple{
		{Subject: "user:dave", Relation: "viewer", Object: "repo:acme"},
		{Subject: "user:eve", Relation: "viewer", Object: "repo:acme"},
	}))
	tuples, _ := eng.ListByObject("repo:acme", "")
	fmt.Printf("repo:acme has %d tuples\n", len(tuples))

	// 6. Explain denied check
	fmt.Println("\n=== Denied Explain ===")
	er, _ = eng.Explain("user:bob", "admin", "repo:acme")
	fmt.Printf("bob admin repo:acme? %v\n", er.Allowed)
}

func must(_ aegis.WriteResult, err error) {
	if err != nil {
		panic(err)
	}
}

func check(eng *aegis.Engine, sub, perm, res string) bool {
	r, err := eng.Check(sub, perm, res)
	if err != nil {
		panic(err)
	}
	return r.Allowed
}
