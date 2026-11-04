// ── CLI Toolchain Tests ──
//
// Tests that CLI types compile and construct correctly.

#![allow(clippy::disallowed_methods)]

#[test]
fn test_cli_compiles() {
    // Verify the CLI types exist and are constructible
    use agent_lib::app::cli::toolchain::ToolchainAction;
    let _ls = ToolchainAction::Ls {
        runtime_type: None,
        remote: false,
        json: false,
    };
    let _install = ToolchainAction::Install {
        runtime_type: "node".into(),
        version: Some("20.18.3".into()),
    };
}

#[test]
fn test_toolchain_action_variants() {
    use agent_lib::app::cli::toolchain::{ProjectAction, ToolchainAction};

    let use_action = ToolchainAction::Use {
        runtime_type: "bun".into(),
        version: "1.2.5".into(),
    };
    match use_action {
        ToolchainAction::Use {
            runtime_type,
            version,
        } => {
            assert_eq!(runtime_type, "bun");
            assert_eq!(version, "1.2.5");
        }
        _ => panic!("Expected Use variant"),
    }

    let uninstall_action = ToolchainAction::Uninstall {
        runtime_type: "ruby".into(),
        version: "3.4.2".into(),
    };
    match uninstall_action {
        ToolchainAction::Uninstall {
            runtime_type,
            version,
        } => {
            assert_eq!(runtime_type, "ruby");
            assert_eq!(version, "3.4.2");
        }
        _ => panic!("Expected Uninstall variant"),
    }

    let _project_ls = ProjectAction::Ls;
    let _project_add = ProjectAction::Add {
        path: "/tmp/test".into(),
    };
    let _project_remove = ProjectAction::Remove { id: "abc".into() };
    let _project_sync = ProjectAction::Sync { id: "abc".into() };
}

#[test]
fn test_cli_toolchain_command_and_runtime_alias() {
    use agent_lib::app::cli::{Cli, Command};
    use clap::Parser;

    let cli = Cli::try_parse_from(["agent", "toolchain", "ls"]).unwrap();
    assert!(matches!(cli.command, Some(Command::Toolchain { .. })));

    // `runtime` is kept as a compatibility alias.
    let cli = Cli::try_parse_from(["agent", "runtime", "ls"]).unwrap();
    assert!(matches!(cli.command, Some(Command::Toolchain { .. })));

    // No subcommand still means "start the desktop app".
    let cli = Cli::try_parse_from(["agent"]).unwrap();
    assert!(cli.command.is_none());
}
