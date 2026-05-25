pub mod api;
pub mod agent;
pub mod commands;
pub mod commands_provider;
pub mod config;
pub mod db;
pub mod environment;
pub mod error;
pub mod intent;
pub mod keychain;
pub mod lifecycle;
pub mod mcp;
pub mod memory;
pub mod persona;
pub mod pipeline;
pub mod skills;
pub mod state;
pub mod tools;

use tauri::Manager;

use crate::state::AppState;

pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_handle = app.handle();
            let state = AppState::new(app_handle)?;
            app.manage(state);

            // Initialize memory, skills, runtimes, MCP
            let app_handle_clone = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                let state = app_handle_clone.state::<AppState>();

                // 0. Seed built-in memories and personas (no-op if already seeded)
                {
                    if let Err(e) = state.memory.seed_defaults().await {
                        log::error!("Failed to seed memories: {}", e);
                    }
                }
                {
                    if let Err(e) = state.persona.seed_defaults().await {
                        log::error!("Failed to seed personas: {}", e);
                    }
                }

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

                // 2. Detect runtimes (cache for fast UI access)
                {
                    state.runtime_manager.detect_all().await;
                }

                // 3. Auto-connect MCP servers
                {
                    let config = state.config.lock().await;
                    let mcp_configs = config.mcp_servers.clone();
                    drop(config);
                    state.mcp.connect_all(&mcp_configs).await;
                }

                // 4. Load lifecycle config
                {
                    state.lifecycle.load_config().await;
                }

                // 5. Archive old sessions
                {
                    if let Err(e) = crate::lifecycle::archiver::run_archive_check(&state.lifecycle).await {
                        log::error!("Failed to run archive check: {}", e);
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::create_session,
            commands::list_sessions,
            commands::get_session,
            commands::delete_session,
            commands::update_session_title,
            commands::update_session_model,
            commands::update_session_system_prompt,
            commands::update_session_config,
            commands::clear_session,
            commands::send_message,
            commands::send_message_stream,
            commands::get_messages,
            commands::save_request_context,
            commands::get_request_context,
            commands::archive_session,
            commands::unarchive_session,
            commands::list_archived_sessions,
            commands::get_session_summaries,
            commands::force_generate_summary,
            commands::get_lifecycle_config,
            commands::update_lifecycle_config,
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
            commands::list_runtimes,
            commands::get_cached_runtimes,
            commands::validate_runtime,

            commands::install_runtime,
            commands::refresh_runtime,
            commands::suggest_runtime_for_command,
            commands::list_available_versions,
            commands::refresh_version_cache,
            commands::list_installed_versions,
            commands::switch_runtime_version,
            commands::uninstall_runtime_version,
            commands::open_version_directory,
            commands::get_runtime_install_dir,
            commands::set_runtime_install_dir,
            commands::scan_project,
            commands::add_bound_project,
            commands::list_bound_projects,
            commands::remove_bound_project,
            commands::sync_project,
            commands::set_runtime_default,
            commands::get_runtime_default,
            commands::resolve_version,
            commands::check_runtime_updates,
            commands::detect_path_conflicts,
            commands::batch_install_runtimes,
            commands::get_version_managers,
            commands::set_active_manager,
            commands::get_active_manager,
            commands::install_manager_tool,
            commands::get_runtime_disk_usage,
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
            commands::create_memory,
            commands::list_memories,
            commands::get_memory,
            commands::search_memories,
            commands::update_memory,
            commands::delete_memory,
            commands::create_persona,
            commands::list_personas,
            commands::get_persona,
            commands::update_persona,
            commands::delete_persona,
            commands::resolve_persona,
            commands::link_memory_to_persona,
            commands::unlink_memory_from_persona,
            commands::get_persona_memories,
            commands::bind_persona_project,
            commands::unbind_persona_project,
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
