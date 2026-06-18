#![cfg(feature = "sqlite")]
use aegis_core::engine::GraphEngine;
use aegis_core::engine::condition::ConditionEvalContext;
use aegis_core::engine::{acl, condition, rbac};
use aegis_core::schema::parse_schema;
use aegis_core::storage::sqlite::{SqliteConfig, SqliteStorage};
use aegis_core::types::{PartitionId, Relation, RelationshipTuple, ResourceId, Schema, SubjectId};
use std::collections::HashMap;

fn make_schema_v2() -> Schema {
    let yaml = r#"
schemaVersion: 2
namespace: v2test
types:
  repo:
    relations:
      owner: {}
      viewer: {}
      banned: {}
    permissions:
      read:
        union_of: [viewer, owner]
      write:
        union_of: [owner]
      owner:
        union_of: [owner]
      banned:
        union_of: [banned]
    roles:
      admin:
        permissions: [read, write]
    deny:
      - relations: [banned]
        description: banned users denied
  workspace:
    relations:
      member: {}
      banned: {}
    permissions:
      access:
        union_of: [member]
    deny:
      - relations: [banned]
"#;
    parse_schema(yaml).unwrap()
}

fn make_schema_abac() -> Schema {
    let yaml = r#"
schemaVersion: 2
namespace: v2abac
types:
  doc:
    relations:
      reader: {}
    permissions:
      view:
        union_of: [reader]
        condition: "(role eq admin) OR (region eq us-east)"
"#;
    parse_schema(yaml).unwrap()
}

fn make_schema_effect_deny() -> Schema {
    let yaml = r#"
schemaVersion: 2
namespace: v2effect
types:
  secret:
    relations:
      member: {}
    permissions:
      view:
        union_of: [member]
      blocked:
        union_of: [member]
        effect: Deny
"#;
    parse_schema(yaml).unwrap()
}

fn make_engine(schema: Schema) -> GraphEngine {
    let config = SqliteConfig::in_memory();
    let storage = SqliteStorage::new(config).unwrap();
    GraphEngine::new(Box::new(storage), schema)
}

fn make_engine_v2() -> GraphEngine {
    make_engine(make_schema_v2())
}

#[test]
fn v2_m1_rbac_assign_check_unassign() {
    let engine = make_engine_v2();
    let alice = SubjectId::new("user:alice").unwrap();
    let repo = ResourceId::new("repo:rbac-test").unwrap();

    let token = rbac::assign_role(&engine, &alice, "owner", &repo).unwrap();
    assert!(token.revision.as_u64() > 0);

    let result =
        rbac::check_role(&engine, &PartitionId::default(), &alice, "owner", &repo).unwrap();
    assert!(result.allowed);

    let roles = rbac::get_roles(&engine, &alice, &repo).unwrap();
    assert_eq!(roles.len(), 1);
    assert!(roles.contains(&"owner".to_string()));

    rbac::unassign_role(&engine, &alice, "owner", &repo).unwrap();
    let result =
        rbac::check_role(&engine, &PartitionId::default(), &alice, "owner", &repo).unwrap();
    assert!(!result.allowed);

    let roles = rbac::get_roles(&engine, &alice, &repo).unwrap();
    assert!(roles.is_empty());
}

#[test]
fn v2_m1_rbac_permission_check_via_role() {
    let engine = make_engine_v2();
    let alice = SubjectId::new("user:alice").unwrap();
    let repo = ResourceId::new("repo:rbac-perm").unwrap();

    rbac::assign_role(&engine, &alice, "owner", &repo).unwrap();

    let read_check = engine.check(&alice, "read", &repo, None).unwrap();
    assert!(read_check.allowed);

    let write_check = engine.check(&alice, "write", &repo, None).unwrap();
    assert!(write_check.allowed);
}

#[test]
fn v2_m1_rbac_multiple_roles() {
    let engine = make_engine_v2();
    let alice = SubjectId::new("user:alice").unwrap();
    let repo = ResourceId::new("repo:multi-role").unwrap();

    rbac::assign_role(&engine, &alice, "owner", &repo).unwrap();
    rbac::assign_role(&engine, &alice, "viewer", &repo).unwrap();

    let roles = rbac::get_roles(&engine, &alice, &repo).unwrap();
    assert_eq!(roles.len(), 2);
    assert!(roles.contains(&"owner".to_string()));
    assert!(roles.contains(&"viewer".to_string()));
}

#[test]
fn v2_m1_rbac_role_does_not_imply_different_resource() {
    let engine = make_engine_v2();
    let alice = SubjectId::new("user:alice").unwrap();
    let repo_a = ResourceId::new("repo:a").unwrap();
    let repo_b = ResourceId::new("repo:b").unwrap();

    rbac::assign_role(&engine, &alice, "owner", &repo_a).unwrap();

    let result =
        rbac::check_role(&engine, &PartitionId::default(), &alice, "owner", &repo_b).unwrap();
    assert!(!result.allowed);
}

#[test]
fn v2_m2_acl_grant_revoke_list() {
    let engine = make_engine_v2();
    let alice = SubjectId::new("user:alice").unwrap();
    let bob = SubjectId::new("user:bob").unwrap();
    let repo = ResourceId::new("repo:acl-test").unwrap();

    let token = acl::grant(&engine, &alice, "read", &repo).unwrap();
    assert!(token.revision.as_u64() > 0);

    assert!(engine.check(&alice, "read", &repo, None).unwrap().allowed);
    assert!(!engine.check(&bob, "read", &repo, None).unwrap().allowed);

    acl::grant(&engine, &bob, "write", &repo).unwrap();
    assert!(engine.check(&bob, "write", &repo, None).unwrap().allowed);

    let acls = acl::list_acls(&engine, &repo).unwrap();
    assert_eq!(acls.len(), 2);

    acl::revoke(&engine, &alice, "read", &repo).unwrap();
    assert!(!engine.check(&alice, "read", &repo, None).unwrap().allowed);

    let acls = acl::list_acls(&engine, &repo).unwrap();
    assert_eq!(acls.len(), 1);
}

#[test]
fn v2_m2_acl_grant_resolves_permission_to_relation() {
    let engine = make_engine_v2();
    let alice = SubjectId::new("user:alice").unwrap();
    let repo = ResourceId::new("repo:acl-resolve").unwrap();

    acl::grant(&engine, &alice, "read", &repo).unwrap();

    let tuples = engine.list_by_object(&repo, None, None).unwrap();
    assert_eq!(tuples.len(), 1);
    assert_eq!(tuples[0].relation.as_str(), "viewer");
}

#[test]
fn v2_m2_acl_revoke_idempotent() {
    let engine = make_engine_v2();
    let alice = SubjectId::new("user:alice").unwrap();
    let repo = ResourceId::new("repo:acl-revoke-idem").unwrap();

    let result = acl::revoke(&engine, &alice, "read", &repo);
    assert!(result.is_ok(), "revoke should be idempotent");
}

#[test]
fn v2_m3_deny_overrides_allow_integration() {
    let engine = make_engine_v2();
    let alice = SubjectId::new("user:alice").unwrap();
    let repo = ResourceId::new("repo:deny-test").unwrap();

    rbac::assign_role(&engine, &alice, "owner", &repo).unwrap();
    assert!(engine.check(&alice, "read", &repo, None).unwrap().allowed);

    rbac::assign_role(&engine, &alice, "banned", &repo).unwrap();
    assert!(!engine.check(&alice, "read", &repo, None).unwrap().allowed);
}

#[test]
fn v2_m3_deny_no_effect_when_no_allow() {
    let engine = make_engine_v2();
    let alice = SubjectId::new("user:alice").unwrap();
    let repo = ResourceId::new("repo:deny-no-allow").unwrap();

    rbac::assign_role(&engine, &alice, "banned", &repo).unwrap();
    let result = engine.check(&alice, "read", &repo, None).unwrap();
    assert!(!result.allowed);
}

#[test]
fn v2_m3_deny_multiple_rules() {
    let yaml = r#"
schemaVersion: 2
namespace: v2denymulti
types:
  repo:
    relations:
      owner: {}
      banned: {}
      suspended: {}
    permissions:
      read:
        union_of: [owner]
    deny:
      - relations: [banned]
      - relations: [suspended]
"#;
    let schema = parse_schema(yaml).unwrap();
    let engine = make_engine(schema);
    let alice = SubjectId::new("user:alice").unwrap();
    let repo = ResourceId::new("repo:multi-deny").unwrap();

    rbac::assign_role(&engine, &alice, "owner", &repo).unwrap();
    rbac::assign_role(&engine, &alice, "suspended", &repo).unwrap();

    assert!(!engine.check(&alice, "read", &repo, None).unwrap().allowed);
}

#[test]
fn v2_m4_tuple_with_expiry_struct() {
    let _engine = make_engine_v2();
    let alice = SubjectId::new("user:alice").unwrap();
    let repo = ResourceId::new("repo:expiry-struct").unwrap();

    let future = chrono::Utc::now() + chrono::Duration::hours(1);
    let tuple = RelationshipTuple::with_expiry(
        alice.clone(),
        Relation::new("owner").unwrap(),
        repo.clone(),
        future,
    );
    assert!(tuple.valid_until.is_some());
    assert_eq!(tuple.subject, alice);
    assert_eq!(tuple.relation.as_str(), "owner");
    assert_eq!(tuple.object, repo);
}

#[test]
fn v2_m4_tuple_expiry_passive_filtering() {
    let engine = make_engine_v2();
    let alice = SubjectId::new("user:alice").unwrap();
    let repo = ResourceId::new("repo:expiry-passive").unwrap();

    let past = chrono::Utc::now() - chrono::Duration::hours(1);
    let expired_tuple = RelationshipTuple::with_expiry(
        alice.clone(),
        Relation::new("owner").unwrap(),
        repo.clone(),
        past,
    );
    engine.write(&expired_tuple).unwrap();

    let result = engine.check(&alice, "read", &repo, None).unwrap();
    assert!(!result.allowed, "expired tuple should not grant access");
}

#[test]
fn v2_m4_tuple_future_expiry_still_works() {
    let engine = make_engine_v2();
    let alice = SubjectId::new("user:alice").unwrap();
    let repo = ResourceId::new("repo:future-expiry").unwrap();

    let future = chrono::Utc::now() + chrono::Duration::hours(1);
    let tuple = RelationshipTuple::with_expiry(
        alice.clone(),
        Relation::new("owner").unwrap(),
        repo.clone(),
        future,
    );
    engine.write(&tuple).unwrap();

    let result = engine.check(&alice, "read", &repo, None).unwrap();
    assert!(
        result.allowed,
        "future-expiry tuple should still grant access"
    );
}

#[test]
fn v2_m5_abac_condition_with_context_integration() {
    let engine = make_engine(make_schema_abac());
    let alice = SubjectId::new("user:alice").unwrap();
    let doc = ResourceId::new("doc:abac-test").unwrap();

    rbac::assign_role(&engine, &alice, "reader", &doc).unwrap();

    let result = engine.check(&alice, "view", &doc, None).unwrap();
    assert!(!result.allowed, "condition without context should deny");

    let mut ctx = condition::ConditionEvalContext::default();
    ctx.subject_meta
        .insert("role".to_string(), "admin".to_string());
    let result = engine
        .check_with_context(&alice, "view", &doc, None, ctx)
        .unwrap();
    assert!(
        result.allowed,
        "matching context (role eq admin) should allow"
    );

    let mut ctx = condition::ConditionEvalContext::default();
    ctx.resource_meta
        .insert("region".to_string(), "us-east".to_string());
    let result = engine
        .check_with_context(&alice, "view", &doc, None, ctx)
        .unwrap();
    assert!(
        result.allowed,
        "matching context (region eq us-east) should allow"
    );

    let mut ctx = condition::ConditionEvalContext::default();
    ctx.resource_meta
        .insert("region".to_string(), "eu-west".to_string());
    let result = engine
        .check_with_context(&alice, "view", &doc, None, ctx)
        .unwrap();
    assert!(!result.allowed, "non-matching context should deny");
}

#[test]
fn v2_m5_composite_and_condition() {
    let yaml = r#"
schemaVersion: 2
namespace: v2comp
types:
  doc:
    relations:
      reader: {}
    permissions:
      view:
        union_of: [reader]
        condition: "(role eq admin) AND (region eq us-east)"
"#;
    let schema = parse_schema(yaml).unwrap();
    let engine = make_engine(schema);
    let alice = SubjectId::new("user:alice").unwrap();
    let doc = ResourceId::new("doc:composite-and").unwrap();

    rbac::assign_role(&engine, &alice, "reader", &doc).unwrap();

    let mut ctx = condition::ConditionEvalContext::default();
    ctx.subject_meta
        .insert("role".to_string(), "admin".to_string());
    ctx.resource_meta
        .insert("region".to_string(), "us-east".to_string());
    let result = engine
        .check_with_context(&alice, "view", &doc, None, ctx)
        .unwrap();
    assert!(result.allowed, "AND: both match should allow");

    let mut ctx = condition::ConditionEvalContext::default();
    ctx.subject_meta
        .insert("role".to_string(), "admin".to_string());
    ctx.resource_meta
        .insert("region".to_string(), "eu-west".to_string());
    let result = engine
        .check_with_context(&alice, "view", &doc, None, ctx)
        .unwrap();
    assert!(!result.allowed, "AND: one fails should deny");
}

#[test]
fn v2_m5_not_condition() {
    let yaml = r#"
schemaVersion: 2
namespace: v2not
types:
  doc:
    relations:
      reader: {}
    permissions:
      view:
        union_of: [reader]
        condition: "NOT (region eq restricted)"
"#;
    let schema = parse_schema(yaml).unwrap();
    let engine = make_engine(schema);
    let alice = SubjectId::new("user:alice").unwrap();
    let doc = ResourceId::new("doc:not-test").unwrap();

    rbac::assign_role(&engine, &alice, "reader", &doc).unwrap();

    let mut ctx = condition::ConditionEvalContext::default();
    ctx.resource_meta
        .insert("region".to_string(), "us-east".to_string());
    let result = engine
        .check_with_context(&alice, "view", &doc, None, ctx)
        .unwrap();
    assert!(result.allowed, "NOT restricted should allow us-east");

    let mut ctx = condition::ConditionEvalContext::default();
    ctx.resource_meta
        .insert("region".to_string(), "restricted".to_string());
    let result = engine
        .check_with_context(&alice, "view", &doc, None, ctx)
        .unwrap();
    assert!(!result.allowed, "NOT restricted should deny restricted");
}

#[test]
fn v2_m6_effect_deny_on_permission() {
    let engine = make_engine(make_schema_effect_deny());
    let alice = SubjectId::new("user:alice").unwrap();
    let secret = ResourceId::new("secret:top").unwrap();

    rbac::assign_role(&engine, &alice, "member", &secret).unwrap();

    let view = engine.check(&alice, "view", &secret, None).unwrap();
    assert!(view.allowed, "view permission should still allow");

    let blocked = engine.check(&alice, "blocked", &secret, None).unwrap();
    assert!(
        !blocked.allowed,
        "blocked permission with Deny effect should deny"
    );
}

#[test]
fn v2_m7_schema_roles_yaml_parse() {
    let yaml = r#"
schemaVersion: 2
namespace: v2schema
types:
  team:
    relations:
      member: {}
    permissions:
      access:
        union_of: [member]
    roles:
      contributor:
        permissions: [access]
"#;
    let schema = parse_schema(yaml).unwrap();
    let team_def = schema.types.get("team").unwrap();
    assert!(team_def.roles.contains_key("contributor"));
    let role = team_def.roles.get("contributor").unwrap();
    assert_eq!(role.permissions, vec!["access"]);
}

#[test]
fn v2_m7_schema_deny_yaml_parse() {
    let yaml = r#"
schemaVersion: 2
namespace: v2schema
types:
  repo:
    relations:
      viewer: {}
      blocked: {}
    permissions:
      read:
        union_of: [viewer]
    deny:
      - relations: [blocked]
        description: blocked users cannot read
"#;
    let schema = parse_schema(yaml).unwrap();
    let repo_def = schema.types.get("repo").unwrap();
    assert_eq!(repo_def.deny.len(), 1);
    assert_eq!(repo_def.deny[0].relations, vec!["blocked"]);
}

#[test]
fn v2_m8_deny_with_inheritance() {
    let yaml = r#"
schemaVersion: 2
namespace: v2inherit
types:
  repo:
    relations:
      user: {}
      owner:
        inherit_from: [user]
      banned:
        inherit_from: [owner]
    permissions:
      owner:
        union_of: [owner]
      read:
        union_of: [owner]
    deny:
      - relations: [banned]
"#;
    let schema = parse_schema(yaml).unwrap();
    let engine = make_engine(schema);

    let _root = SubjectId::new("user:root").unwrap();
    let alice = SubjectId::new("user:alice").unwrap();
    let repo = ResourceId::new("repo:inherit-deny").unwrap();

    engine
        .write(&RelationshipTuple::new(
            alice.clone(),
            Relation::new("owner").unwrap(),
            repo.clone(),
        ))
        .unwrap();

    let result = engine.check(&alice, "read", &repo, None).unwrap();
    assert!(result.allowed, "owner without ban should be allowed");

    engine
        .write(&RelationshipTuple::new(
            alice.clone(),
            Relation::new("banned").unwrap(),
            repo.clone(),
        ))
        .unwrap();
    let result = engine.check(&alice, "read", &repo, None).unwrap();
    assert!(!result.allowed, "banned (inherited) should deny");
}

#[test]
fn v2_m9_explain_shows_deny_path() {
    let engine = make_engine_v2();
    let alice = SubjectId::new("user:alice").unwrap();
    let repo = ResourceId::new("repo:explain-deny").unwrap();

    rbac::assign_role(&engine, &alice, "owner", &repo).unwrap();
    rbac::assign_role(&engine, &alice, "banned", &repo).unwrap();

    let explain = engine.explain(&alice, "read", &repo, None).unwrap();
    assert!(!explain.allowed);
}

#[test]
fn v2_m10_dry_run_with_v2_features() {
    let engine = make_engine_v2();
    let alice = SubjectId::new("user:alice").unwrap();
    let repo = ResourceId::new("repo:dry-v2").unwrap();

    rbac::assign_role(&engine, &alice, "owner", &repo).unwrap();
    rbac::assign_role(&engine, &alice, "banned", &repo).unwrap();

    let dry = engine.check_dry_run(&alice, "read", &repo, None).unwrap();
    assert!(!dry.allowed, "dry-run should respect deny rules");
}

#[test]
fn v2_m10_abac_dry_run_with_context() {
    let engine = make_engine(make_schema_abac());
    let alice = SubjectId::new("user:alice").unwrap();
    let doc = ResourceId::new("doc:abac-dry").unwrap();

    rbac::assign_role(&engine, &alice, "reader", &doc).unwrap();

    let mut ctx = condition::ConditionEvalContext::default();
    ctx.subject_meta
        .insert("role".to_string(), "admin".to_string());
    let result = engine
        .check_dry_run_with_context(&alice, "view", &doc, None, ctx)
        .unwrap();
    assert!(result.allowed, "dry-run with matching context should allow");
}

/// ── V2.5 Role hierarchy ──────────────────────────────────────────────────────

fn make_schema_role_hierarchy() -> Schema {
    let yaml = r#"
schemaVersion: 2
namespace: v2test
types:
  repo:
    relations:
      viewer: {}
      editor: {}
      admin: {}
    permissions:
      read:
        union_of: [viewer, editor, admin]
      write:
        union_of: [editor, admin]
      delete:
        union_of: [admin]
    roles:
      admin:
        permissions: [delete, write, read]
        inherits_from: [editor]
      editor:
        permissions: [write, read]
        inherits_from: [viewer]
      viewer:
        permissions: [read]
"#;
    parse_schema(yaml).unwrap()
}

#[test]
fn v2_5_role_hierarchy_inherited_check() {
    let engine = make_engine(make_schema_role_hierarchy());
    let alice = SubjectId::new("user:alice").unwrap();
    let repo = ResourceId::new("repo:hierarchy").unwrap();

    // Assign alice as viewer only
    rbac::assign_role(&engine, &alice, "viewer", &repo).unwrap();

    // viewer should have read
    let r = engine.check(&alice, "read", &repo, None).unwrap();
    assert!(r.allowed, "viewer should have read");

    // viewer should NOT have write
    let r = engine.check(&alice, "write", &repo, None).unwrap();
    assert!(!r.allowed, "viewer should not have write");
}

#[test]
fn v2_5_role_hierarchy_editor_inherits_viewer_permissions() {
    let engine = make_engine(make_schema_role_hierarchy());
    let bob = SubjectId::new("user:bob").unwrap();
    let repo = ResourceId::new("repo:inherits").unwrap();

    // Assign bob as editor
    rbac::assign_role(&engine, &bob, "editor", &repo).unwrap();

    // editor inherits viewer → should have read
    let r = engine.check(&bob, "read", &repo, None).unwrap();
    assert!(r.allowed, "editor should have read (inherited from viewer)");

    // editor has write directly
    let r = engine.check(&bob, "write", &repo, None).unwrap();
    assert!(r.allowed, "editor should have write");

    // editor should NOT have delete
    let r = engine.check(&bob, "delete", &repo, None).unwrap();
    assert!(!r.allowed, "editor should not have delete");
}

#[test]
fn v2_5_role_hierarchy_admin_inherits_all() {
    let engine = make_engine(make_schema_role_hierarchy());
    let carol = SubjectId::new("user:carol").unwrap();
    let repo = ResourceId::new("repo:admin-inherits").unwrap();

    // Assign carol as admin
    rbac::assign_role(&engine, &carol, "admin", &repo).unwrap();

    // admin inherits editor → inherits viewer → should have all
    let r = engine.check(&carol, "read", &repo, None).unwrap();
    assert!(r.allowed, "admin should have read (inherited)");

    let r = engine.check(&carol, "write", &repo, None).unwrap();
    assert!(r.allowed, "admin should have write (inherited)");

    let r = engine.check(&carol, "delete", &repo, None).unwrap();
    assert!(r.allowed, "admin should have delete (direct)");
}

#[test]
fn v2_5_role_hierarchy_get_roles_includes_inherited() {
    let engine = make_engine(make_schema_role_hierarchy());
    let dave = SubjectId::new("user:dave").unwrap();
    let repo = ResourceId::new("repo:get-roles").unwrap();

    rbac::assign_role(&engine, &dave, "admin", &repo).unwrap();

    let roles = rbac::get_roles(&engine, &dave, &repo).unwrap();
    assert!(
        roles.contains(&"admin".to_string()),
        "should have admin role"
    );
    assert!(
        roles.contains(&"editor".to_string()),
        "should have editor role (inherited)"
    );
    assert!(
        roles.contains(&"viewer".to_string()),
        "should have viewer role (inherited)"
    );
}

#[test]
fn v2_5_role_hierarchy_check_role_resolves_inheritance() {
    let engine = make_engine(make_schema_role_hierarchy());
    let eve = SubjectId::new("user:eve").unwrap();
    let repo = ResourceId::new("repo:check-role").unwrap();

    // Assign editor only
    rbac::assign_role(&engine, &eve, "editor", &repo).unwrap();

    // check_role for "viewer" should return true (editor inherits from viewer,
    // so editor IS considered a viewer too)
    let r = rbac::check_role(&engine, &PartitionId::default(), &eve, "viewer", &repo).unwrap();
    assert!(
        r.allowed,
        "editor should be recognized as having viewer role via inheritance"
    );

    // check_role for "admin" should return false (editor does NOT inherit from admin)
    let r = rbac::check_role(&engine, &PartitionId::default(), &eve, "admin", &repo).unwrap();
    assert!(!r.allowed, "editor should not be recognized as admin");
}

/// ── V2.5 Subject-set resolution ──────────────────────────────────────────────

fn make_schema_subject_set() -> Schema {
    let yaml = r#"
schemaVersion: 2
namespace: v2test
types:
  repo:
    relations:
      editor: {}
    permissions:
      edit:
        union_of: [editor]
  team:
    relations:
      member: {}
"#;
    parse_schema(yaml).unwrap()
}

#[test]
fn v2_5_subject_set_direct_resolution() {
    let engine = make_engine(make_schema_subject_set());
    let alice = SubjectId::new("user:alice").unwrap();
    let team = ResourceId::new("team:eng").unwrap();
    let repo = ResourceId::new("repo:fluxbus").unwrap();

    // user:alice is a member of team:eng
    engine
        .write(&RelationshipTuple::new(
            alice.clone(),
            Relation::new("member").unwrap(),
            team.clone(),
        ))
        .unwrap();

    // team:eng#member (subject-set) is an editor of repo:fluxbus
    let subject_set_id = SubjectId::new("team:eng#member").unwrap();
    engine
        .write(&RelationshipTuple::new(
            subject_set_id,
            Relation::new("editor").unwrap(),
            repo.clone(),
        ))
        .unwrap();

    // user:alice should be able to edit repo:fluxbus via subject-set resolution
    let result = engine.check(&alice, "edit", &repo, None).unwrap();
    assert!(
        result.allowed,
        "alice should inherit editor via subject-set membership"
    );

    // A non-member should NOT get access
    let bob = SubjectId::new("user:bob").unwrap();
    let result2 = engine.check(&bob, "edit", &repo, None).unwrap();
    assert!(
        !result2.allowed,
        "bob should not have editor (not a member of team:eng)"
    );
}

#[test]
fn v2_5_subject_set_as_subject_id_string() {
    // Verify that SubjectId with `#` stores and retrieves correctly
    let sid = SubjectId::new("team:eng#member").unwrap();
    assert_eq!(sid.as_str(), "team:eng#member");

    let parsed = sid.as_subject_set().unwrap();
    assert_eq!(parsed.object.as_str(), "team:eng");
    assert_eq!(parsed.relation.as_str(), "member");
}

#[test]
fn v2_5_subject_set_non_member_denied() {
    let engine = make_engine(make_schema_subject_set());
    let alice = SubjectId::new("user:alice").unwrap();
    let team = ResourceId::new("team:sre").unwrap();
    let repo = ResourceId::new("repo:payments").unwrap();

    // alice is a member of team:sre
    engine
        .write(&RelationshipTuple::new(
            alice.clone(),
            Relation::new("member").unwrap(),
            team.clone(),
        ))
        .unwrap();

    // team:eng#member (different team!) is editor of repo:payments
    let subject_set_id = SubjectId::new("team:eng#member").unwrap();
    engine
        .write(&RelationshipTuple::new(
            subject_set_id,
            Relation::new("editor").unwrap(),
            repo.clone(),
        ))
        .unwrap();

    // alice is NOT a member of team:eng, so should be denied
    let result = engine.check(&alice, "edit", &repo, None).unwrap();
    assert!(
        !result.allowed,
        "alice is in team:sre, not team:eng — should be denied"
    );
}

#[test]
fn v2_5_conditional_tuple_denied_without_context() {
    // A tuple with a condition should not grant access when no context is provided
    let engine = make_engine(make_schema_v2());
    let alice = SubjectId::new("user:alice").unwrap();
    let repo = ResourceId::new("repo:fluxbus").unwrap();

    let tuple = RelationshipTuple::with_condition(
        alice.clone(),
        Relation::new("viewer").unwrap(),
        repo.clone(),
        "role eq admin".to_string(),
    );
    engine.write(&tuple).unwrap();

    // Without context, condition cannot be evaluated → tuple is skipped
    let result = engine.check(&alice, "read", &repo, None).unwrap();
    assert!(
        !result.allowed,
        "conditional tuple should be denied without context"
    );
}

#[test]
fn v2_5_conditional_tuple_allowed_with_matching_context() {
    // A tuple with a condition should grant access when context matches
    let engine = make_engine(make_schema_v2());
    let alice = SubjectId::new("user:alice").unwrap();
    let repo = ResourceId::new("repo:fluxbus").unwrap();

    let tuple = RelationshipTuple::with_condition(
        alice.clone(),
        Relation::new("viewer").unwrap(),
        repo.clone(),
        "role eq admin".to_string(),
    );
    engine.write(&tuple).unwrap();

    let mut subject_meta = HashMap::new();
    subject_meta.insert("role".to_string(), "admin".to_string());
    let ctx = ConditionEvalContext {
        subject_meta,
        ..Default::default()
    };

    let result = engine
        .check_with_context(&alice, "read", &repo, None, ctx)
        .unwrap();
    assert!(
        result.allowed,
        "conditional tuple should be allowed with matching context"
    );
}

#[test]
fn v2_5_conditional_tuple_denied_with_non_matching_context() {
    // A tuple with a condition should NOT grant access when context does not match
    let engine = make_engine(make_schema_v2());
    let alice = SubjectId::new("user:alice").unwrap();
    let repo = ResourceId::new("repo:fluxbus").unwrap();

    let tuple = RelationshipTuple::with_condition(
        alice.clone(),
        Relation::new("viewer").unwrap(),
        repo.clone(),
        "role eq admin".to_string(),
    );
    engine.write(&tuple).unwrap();

    let mut subject_meta = HashMap::new();
    subject_meta.insert("role".to_string(), "viewer".to_string());
    let ctx = ConditionEvalContext {
        subject_meta,
        ..Default::default()
    };

    let result = engine
        .check_with_context(&alice, "read", &repo, None, ctx)
        .unwrap();
    assert!(
        !result.allowed,
        "conditional tuple should be denied with non-matching context"
    );
}

#[test]
fn v2_5_conditional_tuple_unconditional_tuples_still_work() {
    // Unconditional tuples should still work without context (regression check)
    let engine = make_engine(make_schema_v2());
    let alice = SubjectId::new("user:alice").unwrap();
    let repo = ResourceId::new("repo:fluxbus").unwrap();

    engine
        .write(&RelationshipTuple::new(
            alice.clone(),
            Relation::new("viewer").unwrap(),
            repo.clone(),
        ))
        .unwrap();

    let result = engine.check(&alice, "read", &repo, None).unwrap();
    assert!(
        result.allowed,
        "unconditional tuple should still work without context"
    );
}

#[test]
fn v2_5_conditional_tuple_with_expiry() {
    // A tuple with both condition AND valid_until should require both to pass
    let engine = make_engine(make_schema_v2());
    let alice = SubjectId::new("user:alice").unwrap();
    let repo = ResourceId::new("repo:fluxbus").unwrap();

    use chrono::Utc;

    // Expired tuple with condition — even with matching context, expired should be denied
    let past = Utc::now() - chrono::Duration::hours(1);
    let tuple = RelationshipTuple {
        subject: alice.clone(),
        relation: Relation::new("viewer").unwrap(),
        object: repo.clone(),
        created_at: Utc::now(),
        metadata: None,
        valid_until: Some(past),
        condition: Some("role eq admin".to_string()),
    };
    engine.write(&tuple).unwrap();

    let mut subject_meta = HashMap::new();
    subject_meta.insert("role".to_string(), "admin".to_string());
    let ctx = ConditionEvalContext {
        subject_meta,
        ..Default::default()
    };

    let result = engine
        .check_with_context(&alice, "read", &repo, None, ctx)
        .unwrap();
    assert!(
        !result.allowed,
        "expired conditional tuple should be denied even with matching context"
    );
}
