#![cfg_attr(test, allow(clippy::disallowed_methods))]

pub mod agent;
pub mod app;
pub mod core;
pub mod features;
pub mod llm;
pub mod runtime;

use tauri::Manager;

use crate::app::state::AppState;

// `tauri::generate_context!` expands to code that clippy sees as unwrap/expect.
#[allow(clippy::disallowed_methods)]
pub fn run() {
    env_logger::init();

    let result = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app: &mut tauri::App| {
            let app_handle = app.handle();
            let state = AppState::new(app_handle)?;
            app.manage(state);

            // Initialize memory, runtimes, MCP
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
                        log::error!("Failed to seed  : {}", e);
                    }
                }

                // 1. Detect runtimes (cache for fast UI access)
                {
                    state.toolchain.detect_all().await;
                }

                // 2. Auto-connect MCP servers
                {
                    let config = state.config.lock().await;
                    let mcp_configs = config.mcp_servers.clone();
                    drop(config);
                    state.mcp.connect_all(&mcp_configs).await;
                }

                // 3. Load lifecycle config
                {
                    state.lifecycle.load_config().await;
                }

                // 4. Archive old sessions
                {
                    if let Err(e) =
                        crate::features::session::lifecycle::archiver::run_archive_check(&state.lifecycle).await
                    {
                        log::error!("Failed to run archive check: {}", e);
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app::commands::create_session,
            app::commands::list_sessions,
            app::commands::get_session,
            app::commands::delete_session,
            app::commands::update_session_title,
            app::commands::update_session_model,
            app::commands::update_session_system_prompt,
            app::commands::update_session_config,
            app::commands::clear_session,
            app::commands::send_message,
            app::commands::send_message_stream,
            app::commands::get_messages,
            app::commands::save_request_context,
            app::commands::get_request_context,
            app::commands::execute_plan,
            app::commands::pause_execution,
            app::commands::resume_execution,
            app::commands::cancel_execution,
            app::commands::get_execution_status,
            app::commands::get_plan_detail,
            app::commands::archive_session,
            app::commands::unarchive_session,
            app::commands::list_archived_sessions,
            app::commands::list_projects,
            app::commands::create_project,
            app::commands::delete_project,
            app::commands::get_project_sessions,
            app::commands::get_session_summaries,
            app::commands::force_generate_summary,
            app::commands::get_lifecycle_config,
            app::commands::update_lifecycle_config,
            app::commands::get_models,
            app::commands::add_model,
            app::commands::remove_model,
            app::commands::update_model,
            app::commands::set_default_model,
            app::commands::get_default_model,
            app::commands::update_settings,
            app::commands::get_settings,
            app::commands::list_tools,
            app::commands::toggle_tool,
            app::commands::get_skills_env,
            app::commands::list_skill_agents,
            app::commands::list_installed_skills,
            app::commands::get_skill_content,
            app::commands::preview_skill_source,
            app::commands::detect_legacy_skills,
            app::commands::import_legacy_skills,
            app::commands::install_skills,
            app::commands::remove_skill,
            app::commands::check_skill_updates,
            app::commands::update_skills,
            app::commands::list_market_top_skills,
            app::commands::search_market_skills,
            app::commands::create_system_prompt,
            app::commands::list_system_prompts,
            app::commands::delete_system_prompt,
            app::commands::list_mcp_connections,
            app::commands::add_mcp_server,
            app::commands::remove_mcp_server,
            app::commands::connect_mcp_server,
            app::commands::disconnect_mcp_server,
            app::commands::restart_mcp_server,
            app::commands::get_mcp_server_logs,
            app::commands::update_mcp_tool_config,
            app::commands::update_mcp_startup_policy,
            app::commands::get_mcp_connection_stats,
            app::commands::list_runtimes,
            app::commands::get_cached_runtimes,
            app::commands::validate_runtime,
            app::commands::install_runtime,
            app::commands::refresh_runtime,
            app::commands::suggest_runtime_for_command,
            app::commands::list_available_versions,
            app::commands::refresh_version_cache,
            app::commands::list_installed_versions,
            app::commands::switch_runtime_version,
            app::commands::uninstall_runtime_version,
            app::commands::open_version_directory,
            app::commands::get_runtime_install_dir,
            app::commands::set_runtime_install_dir,
            app::commands::scan_project,
            app::commands::add_bound_project,
            app::commands::list_bound_projects,
            app::commands::remove_bound_project,
            app::commands::sync_project,
            app::commands::set_runtime_default,
            app::commands::get_runtime_default,
            app::commands::resolve_version,
            app::commands::check_runtime_updates,
            app::commands::detect_path_conflicts,
            app::commands::batch_install_runtimes,
            app::commands::get_version_managers,
            app::commands::set_active_manager,
            app::commands::get_active_manager,
            app::commands::install_manager_tool,
            app::commands::get_runtime_disk_usage,
            app::commands::list_workflows,
            app::commands::run_workflow,
            app::commands::list_workflow_runs,
            app::commands::pause_workflow_schedule,
            app::commands::resume_workflow_schedule,
            app::commands::get_workflow_run_detail,
            app::commands::set_workflow_var,
            app::commands::delete_workflow_var,
            app::commands::list_workflow_vars,
            app::commands::set_workflow_secret,
            app::commands::delete_workflow_secret,
            app::commands::list_workflow_secrets,
            app::commands::generate_workflow,
            app::commands::set_default_system_prompt,
            app::commands::get_default_system_prompt,
            app::commands::create_memory,
            app::commands::list_memories,
            app::commands::get_memory,
            app::commands::search_memories,
            app::commands::update_memory,
            app::commands::delete_memory,
            app::commands::create_persona,
            app::commands::list_personas,
            app::commands::get_persona,
            app::commands::update_persona,
            app::commands::delete_persona,
            app::commands::resolve_persona,
            app::commands::link_memory_to_persona,
            app::commands::unlink_memory_from_persona,
            app::commands::get_persona_memories,
            app::commands::bind_persona_project,
            app::commands::unbind_persona_project,
            app::commands::list_providers_cmd,
            app::commands::setup_provider,
            app::commands::update_provider_config,
            app::commands::remove_provider,
            app::commands::get_provider_models,
            app::commands::get_available_models,
            app::commands::orchestrate::orchestrate_message,
            app::commands::window::set_window_position,
        ])
        .run(tauri::generate_context!());

    if let Err(error) = result {
        log::error!("error while running tauri application: {}", error);
    }
}
