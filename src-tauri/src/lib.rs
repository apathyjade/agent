pub mod api;
pub mod commands;
pub mod commands_provider;
pub mod config;
pub mod db;
pub mod error;
pub mod keychain;
pub mod state;
pub mod tools;
pub mod agent;

use tauri::Manager;

pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let app_handle = app.handle();
            let state = state::AppState::new(app_handle)?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::create_conversation,
            commands::list_conversations,
            commands::get_conversation,
            commands::delete_conversation,
            commands::update_conversation_title,
            commands::update_conversation_model,
            commands::update_conversation_system_prompt,
            commands::clear_conversation,
            commands::send_message,
            commands::send_message_stream,
            commands::get_messages,
            commands::get_models,
            commands::add_model,
            commands::remove_model,
            commands::update_model,
            commands::set_default_model,
            commands::get_default_model,
            commands::update_settings,
            commands::get_settings,
            commands::list_tools,
            commands::toggle_tool,
            commands::create_system_prompt,
            commands::list_system_prompts,
            commands::delete_system_prompt,
            commands::set_default_system_prompt,
            commands::get_default_system_prompt,
            commands_provider::list_providers_cmd,
            commands_provider::setup_provider,
            commands_provider::update_provider_config,
            commands_provider::remove_provider,
            commands_provider::get_provider_models,
            commands_provider::get_available_models,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
