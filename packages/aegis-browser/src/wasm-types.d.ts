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

export interface AegisWasm {
    // V6 methods (existing)
    init_sync(schemaJson: string, inMemory: boolean): string;
    init_async(schemaJson: string): string;
    check(subject: string, permission: string, resource: string): boolean;
    write_relation(subject: string, relation: string, resource: string): string;
    delete_relation(subject: string, relation: string, resource: string): string;
    list_by_object(resource: string): string;
    list_by_subject(subject: string): string;
    export_json(): string;
    import_json(json: string): string;
    set_partition(partitionId: string): string;
    active_partition(): string;
    explain_v2(subject: string, permission: string, resource: string, consistency?: string): string;
    who_can_access(permission: string, resource: string, pageOffset: number, pageLimit: number, includePaths: boolean): string;
    access_diff(schemaBefore: string, schemaAfter: string, maxChecks?: number): string;
    list_policy_versions(): string;
    rollback_policy(version: number): string;

    // V7 Policy Lifecycle
    create_policy_draft(name: string, description: string): string;
    update_policy_draft(id: string, schemaJson: string): string;
    validate_policy_draft(id: string): string;
    submit_policy_draft_for_review(id: string): string;
    approve_policy_draft(id: string): string;
    reject_policy_draft(id: string, reason: string): string;
    publish_policy_draft(id: string): string;
    archive_policy_draft(id: string): string;
    list_policy_drafts(filterStatus?: string): string;

    // V7 Scheduler
    create_analysis_schedule(configJson: string): string;
    list_analysis_schedules(): string;
    delete_analysis_schedule(id: string): string;
    run_analysis_now(scheduleId?: string): string;
    get_analysis_runs(limit?: number): string;

    // V7 Enforcement History
    set_enforcement_history_config(configJson: string): string;
    get_enforcement_history_config(): string;
    enforcement_trends(limit?: number): string;

    // V7 Subscribe
    subscribe(eventTypesJson: string): string;
}
