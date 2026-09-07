/* tslint:disable */
/* eslint-disable */

export function access_diff(schema_before_json: string, schema_after_json: string, max_checks?: bigint | null): string;

export function active_partition(): string;

export function approve_policy_draft(id: string): string;

export function archive_policy_draft(id: string): string;

export function check(subject: string, permission: string, resource: string): boolean;

export function create_analysis_schedule(config_json: string): string;

export function create_policy_draft(name: string, description: string): string;

export function delete_analysis_schedule(id: string): string;

export function delete_relation(subject: string, relation: string, resource: string): string;

export function enforcement_trends(limit?: number | null): string;

export function explain_v2(subject: string, permission: string, resource: string, consistency_opt?: string | null): string;

export function export_json(): string;

export function get_analysis_runs(limit?: number | null): string;

export function get_enforcement_history_config(): string;

export function import_json(json: string): string;

export function init_async(schema_json: string): string;

export function init_sync(schema_json: string, _in_memory: boolean): string;

export function list_analysis_schedules(): string;

export function list_by_object(resource: string): string;

export function list_by_subject(subject: string): string;

export function list_policy_drafts(filter_status?: string | null): string;

export function list_policy_versions(): string;

export function publish_policy_draft(id: string): string;

export function reject_policy_draft(id: string, reason: string): string;

export function rollback_policy(version: number): string;

export function run_analysis_now(schedule_id?: string | null): string;

export function set_enforcement_history_config(config_json: string): string;

export function set_partition(partition_id: string): string;

export function submit_policy_draft_for_review(id: string): string;

export function subscribe(event_types_json: string): string;

export function update_policy_draft(id: string, schema_json: string): string;

export function validate_policy_draft(id: string): string;

export function who_can_access(permission: string, resource: string, page_offset: bigint, page_limit: bigint, include_paths: boolean): string;

export function write_relation(subject: string, relation: string, resource: string): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly access_diff: (a: number, b: number, c: number, d: number, e: number, f: bigint) => [number, number, number, number];
    readonly active_partition: () => [number, number, number, number];
    readonly approve_policy_draft: (a: number, b: number) => [number, number, number, number];
    readonly archive_policy_draft: (a: number, b: number) => [number, number, number, number];
    readonly check: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number, number];
    readonly create_analysis_schedule: (a: number, b: number) => [number, number, number, number];
    readonly create_policy_draft: (a: number, b: number, c: number, d: number) => [number, number, number, number];
    readonly delete_analysis_schedule: (a: number, b: number) => [number, number, number, number];
    readonly delete_relation: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number, number, number];
    readonly enforcement_trends: (a: number) => [number, number, number, number];
    readonly explain_v2: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number) => [number, number, number, number];
    readonly export_json: () => [number, number, number, number];
    readonly get_analysis_runs: (a: number) => [number, number, number, number];
    readonly get_enforcement_history_config: () => [number, number, number, number];
    readonly import_json: (a: number, b: number) => [number, number, number, number];
    readonly init_async: (a: number, b: number) => [number, number, number, number];
    readonly init_sync: (a: number, b: number, c: number) => [number, number, number, number];
    readonly list_analysis_schedules: () => [number, number, number, number];
    readonly list_by_object: (a: number, b: number) => [number, number, number, number];
    readonly list_by_subject: (a: number, b: number) => [number, number, number, number];
    readonly list_policy_drafts: (a: number, b: number) => [number, number, number, number];
    readonly list_policy_versions: () => [number, number, number, number];
    readonly publish_policy_draft: (a: number, b: number) => [number, number, number, number];
    readonly reject_policy_draft: (a: number, b: number, c: number, d: number) => [number, number, number, number];
    readonly rollback_policy: (a: number) => [number, number, number, number];
    readonly run_analysis_now: (a: number, b: number) => [number, number, number, number];
    readonly set_enforcement_history_config: (a: number, b: number) => [number, number, number, number];
    readonly set_partition: (a: number, b: number) => [number, number, number, number];
    readonly submit_policy_draft_for_review: (a: number, b: number) => [number, number, number, number];
    readonly subscribe: (a: number, b: number) => [number, number, number, number];
    readonly update_policy_draft: (a: number, b: number, c: number, d: number) => [number, number, number, number];
    readonly validate_policy_draft: (a: number, b: number) => [number, number, number, number];
    readonly who_can_access: (a: number, b: number, c: number, d: number, e: bigint, f: bigint, g: number) => [number, number, number, number];
    readonly write_relation: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number, number, number];
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
