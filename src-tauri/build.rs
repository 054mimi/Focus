fn main() {
    let commands = tauri_build::AppManifest::new().commands(&[
        "list_profiles",
        "save_profile_proposal",
        "approve_profile_version",
        "delete_profile",
        "set_profile_visible",
        "set_profile_order",
        "list_projects",
        "list_tasks",
        "save_project",
        "save_task",
        "get_session_status",
        "start_session",
        "shorten_session_time",
        "start_session_break",
        "skip_session_break",
        "record_session_outcome",
        "simulate_session_policy",
    ]);
    let attributes = tauri_build::Attributes::new().app_manifest(commands);
    tauri_build::try_build(attributes).expect("failed to build FOCUS by MIMI Tauri ACL");
}
