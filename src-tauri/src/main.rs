#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//
// Note: the `windows_subsystem` attribute hides the console in release builds.
// Toolchain CLI output therefore requires a debug build (`cargo run -- toolchain status`).

use tauri::Manager;
use agent_lib::{
    app::{commands, state::AppState},
    features
};

/// 启动桌面应用。
///
/// 初始化失败只会写日志：桌面进程没有可返回错误的调用方。
// `tauri::generate_context!` 展开出的代码会被 clippy 判为 unwrap/expect。
pub fn main() {
    env_logger::init();

    let runtime = tokio::runtime::Runtime::new().unwrap_or_else(|e| {
        log::error!("Failed to create tokio runtime: {}", e);
        std::process::exit(1);
    });
    runtime.block_on(async {
        let result = tauri::Builder::default()
            .plugin(tauri_plugin_shell::init())
            .plugin(tauri_plugin_dialog::init())
            .setup(setup)
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
                commands::execute_plan,
                commands::pause_execution,
                commands::resume_execution,
                commands::cancel_execution,
                commands::get_execution_status,
                commands::get_plan_detail,
                commands::archive_session,
                commands::unarchive_session,
                commands::list_archived_sessions,
                commands::list_projects,
                commands::create_project,
                commands::delete_project,
                commands::get_project_sessions,
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
                commands::respond_tool_approval,
                commands::get_skills_env,
                commands::list_skill_agents,
                commands::list_installed_skills,
                commands::get_skill_content,
                commands::preview_skill_source,
                commands::detect_legacy_skills,
                commands::import_legacy_skills,
                commands::install_skills,
                commands::remove_skill,
                commands::check_skill_updates,
                commands::update_skills,
                commands::list_market_top_skills,
                commands::search_market_skills,
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
                commands::list_intent_domains,
                commands::get_active_intent_domain,
                commands::create_intent_domain,
                commands::update_intent_domain,
                commands::delete_intent_domain,
                commands::set_active_intent_domain,
                commands::list_intents,
                commands::get_intent,
                commands::create_intent,
                commands::update_intent,
                commands::delete_intent,
                commands::set_intent_enabled,
                commands::import_intent_domain,
                commands::export_intent_domain,
                commands::get_session_intent_state,
                commands::set_session_intent_domain,
                commands::get_recent_route_traces,
                commands::list_providers_cmd,
                commands::setup_provider,
                commands::update_provider_config,
                commands::remove_provider,
                commands::get_provider_models,
                commands::get_available_models,
                commands::orchestrate::orchestrate_message,
                commands::window::set_window_position,
            ])
            .run(tauri::generate_context!());

        if let Err(error) = result {
            log::error!("error while running tauri application: {}", error);
        }
    });
   
}

/// Tauri 的同步启动钩子：构造并托管 [`AppState`]，然后拉起后台初始化。
///
/// 状态构造失败必须在这里返回错误——后续所有 command 都依赖它，缺了它
/// 应用只会不停报 `state not managed`。
fn setup(app: &mut tauri::App) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let app_handle = app.handle();
    let state = AppState::new(app_handle)?;
    app.manage(state);

    let app_handle_clone = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        bootstrap(app_handle_clone).await;
    });

    Ok(())
}

/// 后台初始化：记忆/人设种子、运行时探测、MCP 自动连接、生命周期配置、归档检查。
///
/// 顺序是有意的：
/// 1. 先写种子数据，后续步骤（尤其 MCP 与归档）读到的就是最终数据；
/// 2. 运行时探测结果会被 MCP 与工具链 UI 直接读取，必须先落缓存；
/// 3. MCP 自动连接依赖运行时探测出的 Node/Python 等，必须排在探测之后；
/// 4. 生命周期配置决定归档阈值，归档检查必须排在它之后。
///
/// 每一步都只记日志、不向上传播错误：单个子系统初始化失败不应该让应用
/// 起不来。
async fn bootstrap(app_handle: tauri::AppHandle) {
    let state = app_handle.state::<AppState>();

    // 1. 内置记忆与内置人设的种子数据（已存在时为空操作）。
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

    // 2. 探测本机运行时并写入缓存，供界面首屏快速读取。
    {
        state.toolchain.detect_all().await;
    }

    // 3. 自动连接配置里标记为自启的 MCP server。
    //    必须先于归档检查之后的所有 UI 读取，否则列表会短暂为空。
    {
        let config = state.config.lock().await;
        let mcp_configs = config.mcp_servers.clone();
        drop(config);
        state.mcp.connect_all(&mcp_configs).await;
    }

    // 4. 载入会话生命周期配置（摘要/归档阈值）。
    {
        state.lifecycle.load_config().await;
    }

    // 5. 按生命周期配置归档过期会话；必须在配置载入之后。
    {
        if let Err(e) = features::session::lifecycle::archiver::run_archive_check(&state.lifecycle).await  {
            log::error!("Failed to run archive check: {}", e);
        }
    }
}

