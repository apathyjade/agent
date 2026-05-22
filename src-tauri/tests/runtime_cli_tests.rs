// ── CLI Runtime Tests ──
//
// Tests that CLI types compile and construct correctly.

#[test]
fn test_cli_compiles() {
    // Verify the CLI types exist and are constructible
    use agent_lib::environment::cli::RuntimeAction;
    let _ls = RuntimeAction::Ls {
        runtime_type: None,
        remote: false,
        json: false,
    };
    let _install = RuntimeAction::Install {
        runtime_type: "node".into(),
        version: Some("20.18.3".into()),
    };
}

#[test]
fn test_runtime_action_variants() {
    use agent_lib::environment::cli::{ProjectAction, RuntimeAction};

    let use_action = RuntimeAction::Use {
        runtime_type: "bun".into(),
        version: "1.2.5".into(),
    };
    match use_action {
        RuntimeAction::Use { runtime_type, version } => {
            assert_eq!(runtime_type, "bun");
            assert_eq!(version, "1.2.5");
        }
        _ => panic!("Expected Use variant"),
    }

    let uninstall_action = RuntimeAction::Uninstall {
        runtime_type: "ruby".into(),
        version: "3.4.2".into(),
    };
    match uninstall_action {
        RuntimeAction::Uninstall { runtime_type, version } => {
            assert_eq!(runtime_type, "ruby");
            assert_eq!(version, "3.4.2");
        }
        _ => panic!("Expected Uninstall variant"),
    }

    let _project_ls = ProjectAction::Ls;
    let _project_add = ProjectAction::Add { path: "/tmp/test".into() };
    let _project_remove = ProjectAction::Remove { id: "abc".into() };
    let _project_sync = ProjectAction::Sync { id: "abc".into() };
}
