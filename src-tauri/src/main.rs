mod commands;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(commands::initialize)
        .invoke_handler(tauri::generate_handler![
            commands::app_status,
            commands::projects_create,
            commands::projects_list,
            commands::projects_default_exclusions,
            commands::recovery_reports,
            commands::threads_list,
            commands::threads_start,
            commands::threads_pause,
            commands::threads_resume,
            commands::threads_end,
            commands::threads_archive,
            commands::checkpoints_save_place,
            commands::checkpoints_list,
            commands::checkpoints_get,
            commands::checkpoints_approve,
            commands::checkpoints_corrections,
            commands::checkpoints_preview,
            commands::checkpoints_generate,
            commands::checkpoints_discard,
            commands::checkpoints_cancel_generation,
            commands::configuration_status,
            commands::configuration_store,
            commands::configuration_remove_key,
            commands::configuration_test,
            commands::events_list,
            commands::events_add_note,
            commands::events_capture_git,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run ThreadBack");
}
