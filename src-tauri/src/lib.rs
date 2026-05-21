pub mod api;
pub mod commands;
pub mod commands_provider;
pub mod config;
pub mod db;
pub mod error;
pub mod keychain;
pub mod mcp;
pub mod pipeline;
pub mod state;
pub mod tools;
pub mod agent;
pub mod skills;

use tauri::Manager;

use crate::state::AppState;

pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let app_handle = app.handle();
            let state = AppState::new(app_handle)?;
            app.manage(state);

            // Initialize skill system + MCP connections
            let app_handle_clone = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                let state = app_handle_clone.state::<AppState>();

                // 1. Initialize skills
                {
                    let skill_manager = state.skills.lock().await;
                    if let Err(e) = skill_manager.cleanup_legacy_builtins().await {
                        log::error!("Failed to clean up legacy builtin skills: {}", e);
                    }
                    if let Err(e) = skill_manager.reconcile().await {
                        log::error!("Failed to reconcile skills: {}", e);
                    }
                    if let Err(e) = skill_manager.sync_enabled_to_tools().await {
                        log::error!("Failed to sync enabled skills to tools: {}", e);
                    }
                }

                // 2. Auto-connect MCP servers
                {
                    let config = state.config.lock().await;
                    let mcp_configs = config.mcp_servers.clone();
                    drop(config);
                    state.mcp.connect_all(&mcp_configs).await;
                }
            });

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
            commands::list_skills,
            commands::get_skill_detail,
            commands::install_skill_from_path,
            commands::uninstall_skill,
            commands::toggle_skill,
            commands::configure_skill,
            commands::reconcile_skills,
            commands::list_market_top_skills,
            commands::search_market_skills,
            commands::install_market_skill,
            commands::create_system_prompt,
            commands::list_system_prompts,
            commands::delete_system_prompt,
            commands::list_mcp_connections,
            commands::add_mcp_server,
            commands::remove_mcp_server,
            commands::connect_mcp_server,
            commands::disconnect_mcp_server,
            commands::restart_mcp_server,
            commands::get_mcp_server_logs,
            commands::update_mcp_tool_config,
            commands::update_mcp_startup_policy,
            commands::get_mcp_connection_stats,
            commands::list_workflows,
            commands::run_workflow,
            commands::list_workflow_runs,
            commands::pause_workflow_schedule,
            commands::resume_workflow_schedule,
            commands::get_workflow_run_detail,
            commands::set_workflow_var,
            commands::delete_workflow_var,
            commands::list_workflow_vars,
            commands::set_workflow_secret,
            commands::delete_workflow_secret,
            commands::list_workflow_secrets,
            commands::generate_workflow,
            commands::set_default_system_prompt,
            commands::get_default_system_prompt,
            commands_provider::list_providers_cmd,
            commands_provider::setup_provider,
            commands_provider::update_provider_config,
            commands_provider::remove_provider,
            commands_provider::get_provider_models,
            commands_provider::get_available_models,
            commands::window::set_window_position,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
