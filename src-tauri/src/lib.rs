// NiceSSH library entry point — registers all IPC commands and plugins.

pub mod commands;
pub mod config_store;
pub mod scanner;

#[cfg(test)]
mod test_helpers;
mod error;
mod fs_safety;
mod git_config;
mod history;
mod paths;
mod runner;
mod ssh_config;
mod ssh_keys;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::default().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            commands::identity::list_identities,
            commands::identity::create_identity,
            commands::identity::update_identity,
            commands::identity::delete_identity,
            commands::scanner::scan_existing_identities,
            commands::project::list_projects,
            commands::project::add_project,
            commands::project::remove_project,
            commands::project::assign_identity,
            commands::ssh_key::list_keys,
            commands::ssh_key::ssh_key_exists,
            commands::ssh_key::generate_key,
            commands::ssh_key::delete_key,
            commands::ssh_key::get_public_key,
            commands::ssh_key::copy_public_key_to_clipboard,
            commands::ssh_key::ssh_add_test,
            commands::ssh_key::is_key_encrypted,
            commands::ssh_config::get_ssh_config,
            commands::ssh_config::upsert_github_host_block,
            commands::ssh_config::validate_ssh_config,
            commands::git::is_git_repo,
            commands::git::apply_identity_to_repo,
            commands::git::get_recent_commits,
            commands::git::get_repo_git_config,
            commands::git::get_global_git_config,
            commands::git::set_global_git_config,
            commands::git::test_ssh_connection,
            commands::history::list_history,
            commands::history::rollback,
            commands::settings::check_environment,
            commands::settings::clear_history,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
