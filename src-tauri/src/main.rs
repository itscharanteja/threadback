mod commands;

fn main() {
    tauri::Builder::default()
        .setup(commands::initialize)
        .invoke_handler(tauri::generate_handler![
            commands::app_status,
            commands::projects_create,
            commands::projects_list,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run ThreadBack");
}
