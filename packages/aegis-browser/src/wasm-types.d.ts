declare module "../rust/pkg/aegis_browser.js" {
  export default function init(): Promise<void>;
  export function init_async(schema: string): string;
  export function init_sync(schema: string, inMemory: boolean): string;
  export function check(subject: string, permission: string, resource: string): boolean;
  export function write_relation(subject: string, relation: string, resource: string): string;
  export function delete_relation(subject: string, relation: string, resource: string): string;
  export function list_by_object(resource: string): string;
  export function list_by_subject(subject: string): string;
  export function export_json(): string;
  export function import_json(json: string): string;
  export function set_partition(partitionId: string): string;
  export function active_partition(): string;
  export function explain_v2(subject: string, permission: string, resource: string, consistency: string | null): string;
  export function who_can_access(permission: string, resource: string, pageOffset: number, pageLimit: number, includePaths: boolean): string;
  export function access_diff(schemaBefore: string, schemaAfter: string, maxChecks: number | null): string;
  export function list_policy_versions(): string;
  export function rollback_policy(version: number): string;
}
