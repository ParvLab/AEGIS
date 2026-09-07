export interface SeedTuple {
  subject: string;
  relation: string;
  object: string;
}

export const SEED_MINIMAL: SeedTuple[] = [
  { subject: "user:alice", relation: "admin", object: "org:acme" },
  { subject: "user:bob", relation: "member", object: "org:acme" },
  { subject: "user:alice", relation: "member", object: "team:engineering" },
  { subject: "user:alice", relation: "admin", object: "team:engineering" },
  { subject: "user:bob", relation: "member", object: "team:engineering" },
  { subject: "user:carol", relation: "member", object: "team:security" },
  { subject: "user:carol", relation: "admin", object: "team:security" },
  { subject: "user:mallory", relation: "member", object: "team:engineering" },
  { subject: "team:engineering#member", relation: "admin", object: "repo:payment-api" },
  { subject: "team:engineering#member", relation: "maintainer", object: "repo:docs" },
  { subject: "team:security#member", relation: "viewer", object: "repo:payment-api" },
];

export const SEED_FULL: SeedTuple[] = [
  ...SEED_MINIMAL,
  { subject: "user:dave", relation: "member", object: "team:engineering" },
  { subject: "user:eve", relation: "member", object: "team:engineering" },
  { subject: "user:eve", relation: "maintainer", object: "team:engineering" },
  { subject: "user:frank", relation: "member", object: "team:security" },
  { subject: "user:grace", relation: "admin", object: "org:acme" },
  { subject: "user:grace", relation: "admin", object: "repo:payment-api" },
  { subject: "user:dave", relation: "maintainer", object: "repo:docs" },
  { subject: "team:engineering#member", relation: "admin", object: "repo:ci-pipeline" },
  { subject: "team:security#member", relation: "admin", object: "repo:vault" },
  { subject: "team:security#member", relation: "viewer", object: "repo:docs" },
];

export const ALL_USERS = [
  "user:alice",
  "user:bob",
  "user:carol",
  "user:mallory",
  "user:dave",
  "user:eve",
  "user:frank",
  "user:grace",
];

export const ALL_RESOURCES = [
  "org:acme",
  "team:engineering",
  "team:security",
  "repo:payment-api",
  "repo:docs",
  "repo:ci-pipeline",
  "repo:vault",
];

export const PERMISSIONS = ["pull", "push", "admin"];

export const RELATIONS = ["member", "maintainer", "admin", "viewer", "banned"];
