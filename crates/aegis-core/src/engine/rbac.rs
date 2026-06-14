//! # RBAC-to-Graph Translation
//!
//! This module implements Role-Based Access Control (RBAC) on top of Aegis's
//! Relationship-Based Access Control (ReBAC) graph model.
//!
//! ## How RBAC Maps to the Graph
//!
//! In pure RBAC, a subject `S` has role `R` on resource `O`. In Aegis's graph,
//! this is represented as a single tuple:
//!
//! ```text
//! (S, R, O)
//! ```
//!
//! For example: `(user:alice, admin, repo:fluxbus)` means "Alice has the admin
//! role on repo:fluxbus".
//!
//! ## Role Hierarchy (Inheritance)
//!
//! Roles can inherit from other roles. If `admin` inherits from `editor`, a
//! subject with `admin` is also considered to have `editor`. This is defined
//! in the schema:
//!
//! ```yaml
//! roles:
//!   admin:
//!     inherits_from: [editor]
//!     permissions: [read, write]
//!   editor:
//!     inherits_from: [viewer]
//!     permissions: [read]
//!   viewer:
//!     permissions: [read]
//! ```
//!
//! The `check_role()` function resolves this by checking if any child role
//! (that inherits from the requested role) is directly assigned.
//!
//! ## Roles vs Permissions
//!
//! Roles are ReBAC relations (edges in the graph). Permissions are logical
//! expressions over relations. A role assignment tuple `(S, role, O)` can be
//! checked directly as a relation, or used as a building block for permissions.
//!
//! ## RBAC via check()
//!
//! Since roles are just relations, `engine.check(subject, "role", resource)`
//! works natively. The `check_role()` function additionally resolves role
//! inheritance where `admin` → `editor` means a subject with `admin` also
//! has `editor` access.
//!
//! ## Example
//!
//! ```text
//! Schema defines: admin, editor, viewer roles
//! Tuple: (user:alice, admin, repo:fluxbus)
//!
//! check_role("admin")  → true   (direct assignment)
//! check_role("editor") → true   (admin inherits from editor)
//! check_role("viewer") → true   (editor inherits from viewer)
//! ```

use crate::engine::GraphEngine;
use crate::error::AegisResult;
use crate::types::*;
use std::collections::{BTreeSet, HashMap, HashSet};

/// Assign a role to a subject on a resource.
/// Writes a ReBAC tuple for the role relation.
pub fn assign_role(
    engine: &GraphEngine,
    subject: &SubjectId,
    role: &str,
    resource: &ResourceId,
) -> AegisResult<RevisionToken> {
    let tuple = RelationshipTuple::new(
        subject.clone(),
        Relation::new(role)?,
        resource.clone(),
    );
    engine.write(&tuple)
}

/// Remove a role assignment from a subject on a resource.
pub fn unassign_role(
    engine: &GraphEngine,
    subject: &SubjectId,
    role: &str,
    resource: &ResourceId,
) -> AegisResult<RevisionToken> {
    let key = TupleKey {
        subject: subject.clone(),
        relation: Relation::new(role)?,
        object: resource.clone(),
    };
    engine.delete(&key)
}

/// Check whether a subject has a specific role on a resource.
/// Resolves role inheritance: if `admin` inherits from `editor`,
/// a subject with `editor` also has `admin` access.
pub fn check_role(
    engine: &GraphEngine,
    subject: &SubjectId,
    role: &str,
    resource: &ResourceId,
) -> AegisResult<CheckResult> {
    // 1. Direct role assignment check via engine.check (treats role as a permission)
    let direct = engine.check(subject, role, resource, None)?;
    if direct.allowed {
        return Ok(direct);
    }

    // 2. Reverse inheritance: check if any child role (that inherits from `role`)
    //    is directly assigned to the subject.
    let schema = engine.schema();
    let resource_type = resource_type_name(resource.as_str());
    if let Some(type_def) = schema.types.get(&resource_type) {
        for (child_role_name, child_role_def) in &type_def.roles {
            if child_role_def.inherits_from.contains(&role.to_string()) {
                // Check if subject has the child role relation directly.
                // Use list_by_subject to check for a direct tuple match,
                // since engine.check would resolve it as a permission (not what we want).
                let tuples = engine.list_by_subject(subject, Some(&Relation::new(child_role_name).unwrap()), None)?;
                if tuples.iter().any(|t| t.object == *resource) {
                    let rev = engine.storage().current_revision().unwrap_or(Revision::ZERO);
                    return Ok(CheckResult {
                        allowed: true,
                        revision: rev,
                    });
                }
            }
        }
    }

    Ok(CheckResult {
        allowed: false,
        revision: direct.revision,
    })
}

/// List all roles a subject has on a resource, including inherited roles.
/// e.g. if `admin` inherits from `editor`, and the subject has `admin`,
/// they are also considered to have `editor`.
pub fn get_roles(
    engine: &GraphEngine,
    subject: &SubjectId,
    resource: &ResourceId,
) -> AegisResult<Vec<String>> {
    let tuples = engine.list_by_subject(subject, None, None)?;
    let direct_roles: Vec<String> = tuples
        .into_iter()
        .filter(|t| t.object == *resource)
        .map(|t| t.relation.as_str().to_string())
        .collect();

    // Expand inherited roles using schema
    let schema = engine.schema();
    let resource_type = resource_type_name(resource.as_str());
    let mut all_roles: BTreeSet<String> = direct_roles.iter().cloned().collect();

    if let Some(type_def) = schema.types.get(&resource_type) {
        // For each direct role, add its parent roles (reverse inheritance)
        for direct_role in &direct_roles {
            add_inherited_roles(direct_role, &type_def.roles, &mut all_roles, &mut HashSet::new());
        }
    }

    Ok(all_roles.into_iter().collect())
}

/// Recursively add parent roles (those that this role inherits from).
fn add_inherited_roles(
    role_name: &str,
    all_roles: &HashMap<String, crate::types::schema::RoleDef>,
    acc: &mut BTreeSet<String>,
    visited: &mut HashSet<String>,
) {
    if !visited.insert(role_name.to_string()) {
        return; // Cycle protection
    }
    if let Some(role_def) = all_roles.get(role_name) {
        for parent in &role_def.inherits_from {
            acc.insert(parent.clone());
            add_inherited_roles(parent, all_roles, acc, visited);
        }
    }
}

fn resource_type_name(s: &str) -> String {
    s.split(':').next().unwrap_or("").to_string()
}
