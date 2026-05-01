mod commands;
mod crypto;
mod db;
mod inference;
mod prompt;
mod vault;

use std::sync::Mutex;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Resolve the operator's coo_dir and open the SQLite database
            // before any IPC fires. Both are fail-fast: if we cannot find
            // the home directory or migrate the schema, the application is
            // unusable and refusing to launch is correct.
            let coo_dir = vault::default_coo_dir()?;
            let conn = db::open_and_migrate()?;
            app.manage(commands::AppState {
                coo_dir,
                db: Mutex::new(conn),
                vault: Mutex::new(None),
                inference: inference::build_provider(),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::vault_inspect,
            commands::vault_setup,
            commands::vault_unlock,
            commands::write_app_config,
            commands::write_operator_profile,
            commands::write_calibration_setting,
            commands::infer,
            commands::load_conversation,
            commands::append_turn,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
