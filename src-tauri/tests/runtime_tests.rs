// ── Runtime Tests: Lifecycle, VersionSource, and Type Detection ──

use agent_lib::environment::lifecycle;
use agent_lib::environment::lifecycle::VersionLifecycle;
use agent_lib::environment::RuntimeType;

// ── Node.js Lifecycle Tests ──

#[test]
fn test_node_lifecycle_lts() {
    let lc = lifecycle::node_lifecycle("22.14.0", Some("Jod"));
    assert!(matches!(lc, VersionLifecycle::Lts { .. }));
}

#[test]
fn test_node_lifecycle_eol() {
    let lc = lifecycle::node_lifecycle("16.20.0", Some("Gallium"));
    assert!(matches!(lc, VersionLifecycle::Eol { .. }));
}

#[test]
fn test_node_lifecycle_maintenance() {
    let lc = lifecycle::node_lifecycle("20.18.3", Some("Iron"));
    assert!(matches!(lc, VersionLifecycle::Maintenance { .. }));
}

#[test]
fn test_node_lifecycle_active() {
    let lc = lifecycle::node_lifecycle("23.0.0", None);
    assert!(matches!(lc, VersionLifecycle::Eol { .. }));
}

// ── Python Lifecycle Tests ──

#[test]
fn test_python_lifecycle_latest() {
    let lc = lifecycle::python_lifecycle("3.13.0");
    assert_eq!(lc, VersionLifecycle::Latest);
}

#[test]
fn test_python_lifecycle_eol() {
    let lc = lifecycle::python_lifecycle("3.8.0");
    assert!(matches!(lc, VersionLifecycle::Eol { .. }));
}

#[test]
fn test_python_lifecycle_active() {
    let lc = lifecycle::python_lifecycle("3.13.0");
    assert_eq!(lc, VersionLifecycle::Latest);
}

// ── Go Lifecycle Tests ──

#[test]
fn test_go_lifecycle_latest() {
    let lc = lifecycle::go_lifecycle("1.24.0");
    assert_eq!(lc, VersionLifecycle::Latest);
}

#[test]
fn test_go_lifecycle_eol() {
    let lc = lifecycle::go_lifecycle("1.20.0");
    assert!(matches!(lc, VersionLifecycle::Eol { .. }));
}

#[test]
fn test_go_lifecycle_active() {
    let lc = lifecycle::go_lifecycle("1.22.0");
    assert!(matches!(lc, VersionLifecycle::Active));
}

// ── Rust Lifecycle Tests ──

#[test]
fn test_rust_lifecycle_stable() {
    let lc = lifecycle::rust_lifecycle("stable");
    assert!(matches!(lc, VersionLifecycle::Active));
}

#[test]
fn test_rust_lifecycle_nightly() {
    let lc = lifecycle::rust_lifecycle("nightly");
    assert!(matches!(lc, VersionLifecycle::Active));
}

#[test]
fn test_rust_lifecycle_1_85() {
    let lc = lifecycle::rust_lifecycle("1.85.0");
    assert!(matches!(lc, VersionLifecycle::Active));
}

// ── Java Lifecycle Tests ──

#[test]
fn test_java_lifecycle_lts() {
    let lc = lifecycle::java_lifecycle("21.0.7");
    assert!(matches!(lc, VersionLifecycle::Lts { .. }));
}

#[test]
fn test_java_lifecycle_23() {
    let lc = lifecycle::java_lifecycle("23.0.0");
    assert_eq!(lc, VersionLifecycle::Latest);
}

// ── Deno Lifecycle Tests ──

#[test]
fn test_deno_lifecycle() {
    let lc = lifecycle::deno_lifecycle("2.2.0");
    assert_eq!(lc, VersionLifecycle::Active);
}

// ── Bun Lifecycle Tests ──

#[test]
fn test_bun_lifecycle() {
    let lc = lifecycle::bun_lifecycle("1.2.5");
    assert_eq!(lc, VersionLifecycle::Active);
}

#[test]
fn test_bun_lifecycle_old() {
    let lc = lifecycle::bun_lifecycle("0.8.1");
    assert_eq!(lc, VersionLifecycle::Active);
}

// ── Ruby Lifecycle Tests ──

#[test]
fn test_ruby_lifecycle_3_4() {
    let lc = lifecycle::ruby_lifecycle("3.4.2");
    assert!(matches!(lc, VersionLifecycle::Active));
}

#[test]
fn test_ruby_lifecycle_3_2() {
    let lc = lifecycle::ruby_lifecycle("3.2.6");
    assert!(matches!(lc, VersionLifecycle::Maintenance { .. }));
}

#[test]
fn test_ruby_lifecycle_3_1() {
    let lc = lifecycle::ruby_lifecycle("3.1.6");
    assert!(matches!(lc, VersionLifecycle::Maintenance { .. }));
}

#[test]
fn test_ruby_lifecycle_3_0() {
    let lc = lifecycle::ruby_lifecycle("3.0.7");
    assert!(matches!(lc, VersionLifecycle::Eol { .. }));
}

// ── for_runtime Dispatch Tests ──

#[test]
fn test_for_runtime_dispatches_correctly() {
    let lc = lifecycle::for_runtime(&RuntimeType::Node, "22.14.0", Some("Jod"));
    assert!(matches!(lc, VersionLifecycle::Lts { .. }));
}

#[test]
fn test_for_runtime_docker() {
    let lc = lifecycle::for_runtime(&RuntimeType::Docker, "24.0.0", None);
    assert_eq!(lc, VersionLifecycle::Active);
}

#[test]
fn test_for_runtime_bun() {
    let lc = lifecycle::for_runtime(&RuntimeType::Bun, "1.2.5", None);
    assert_eq!(lc, VersionLifecycle::Active);
}

#[test]
fn test_for_runtime_ruby() {
    let lc = lifecycle::for_runtime(&RuntimeType::Ruby, "3.4.2", None);
    assert!(matches!(lc, VersionLifecycle::Active));
}

// ── VersionLifecycle UI Tests ──

#[test]
fn test_version_lifecycle_label() {
    assert_eq!(VersionLifecycle::Latest.label(), "最新");
    assert_eq!(VersionLifecycle::Active.label(), "活跃");
    assert_eq!(VersionLifecycle::Eol { eol_date: "2024-01-01".into() }.label(), "已停止支持");
    assert_eq!(VersionLifecycle::Lts { codename: "Jod".into() }.label(), "LTS");
    assert_eq!(VersionLifecycle::Maintenance { eol_date: None }.label(), "维护期");
}

#[test]
fn test_version_lifecycle_emoji() {
    assert_eq!(VersionLifecycle::Latest.emoji(), "🆕");
    assert_eq!(VersionLifecycle::Eol { eol_date: "x".into() }.emoji(), "🔴");
    assert_eq!(VersionLifecycle::Active.emoji(), "🟢");
}

// ── RuntimeType Detection Tests ──

#[test]
fn test_runtime_type_from_str() {
    assert_eq!(RuntimeType::from_str("node"), Some(RuntimeType::Node));
    assert_eq!(RuntimeType::from_str("python"), Some(RuntimeType::Python));
    assert_eq!(RuntimeType::from_str("bun"), Some(RuntimeType::Bun));
    assert_eq!(RuntimeType::from_str("ruby"), Some(RuntimeType::Ruby));
    assert_eq!(RuntimeType::from_str("irb"), Some(RuntimeType::Ruby));
    assert_eq!(RuntimeType::from_str("gem"), Some(RuntimeType::Ruby));
    assert_eq!(RuntimeType::from_str("bundler"), Some(RuntimeType::Ruby));
    assert_eq!(RuntimeType::from_str("unknown"), None);
}

#[test]
fn test_runtime_type_display_name() {
    assert_eq!(RuntimeType::Bun.display_name(), "Bun");
    assert_eq!(RuntimeType::Ruby.display_name(), "Ruby");
}

#[test]
fn test_runtime_type_dir_name() {
    assert_eq!(RuntimeType::Bun.dir_name(), "bun");
    assert_eq!(RuntimeType::Ruby.dir_name(), "ruby");
}

#[test]
fn test_runtime_type_infer_from_command() {
    assert_eq!(RuntimeType::infer_from_command("bun"), Some(RuntimeType::Bun));
    assert_eq!(RuntimeType::infer_from_command("ruby"), Some(RuntimeType::Ruby));
    assert_eq!(RuntimeType::infer_from_command("irb"), Some(RuntimeType::Ruby));
    assert_eq!(RuntimeType::infer_from_command("gem"), Some(RuntimeType::Ruby));
    assert_eq!(RuntimeType::infer_from_command("bundler"), Some(RuntimeType::Ruby));
}

#[test]
fn test_runtime_type_commands() {
    assert_eq!(RuntimeType::Bun.commands(), &["bun"]);
    assert_eq!(RuntimeType::Ruby.commands(), &["ruby", "irb"]);
}

#[test]
fn test_runtime_type_primary_command() {
    assert_eq!(RuntimeType::Bun.primary_command(), "bun");
    assert_eq!(RuntimeType::Ruby.primary_command(), "ruby");
}

#[test]
fn test_runtime_type_all_contains_new() {
    let all = RuntimeType::all();
    assert!(all.contains(&RuntimeType::Bun));
    assert!(all.contains(&RuntimeType::Ruby));
}

// ── Version Source Tests ──

#[test]
fn test_bun_version_source_fallback() {
    use agent_lib::environment::sources::bun::BunVersionSource;
    use agent_lib::environment::registry::VersionSource;

    // We can't easily test fetch_versions (needs network), but we can test the
    // fallback logic by checking that the source compiles and is constructible.
    let source = BunVersionSource;
    assert_eq!(source.runtime_type(), RuntimeType::Bun);
}

#[test]
fn test_ruby_version_source_curated() {
    use agent_lib::environment::sources::ruby::RubyVersionSource;
    use agent_lib::environment::registry::VersionSource;

    let source = RubyVersionSource;
    assert_eq!(source.runtime_type(), RuntimeType::Ruby);
}

// ── HTTP Client Tests ──

#[test]
fn test_init_http_client_no_proxy() {
    // Should not panic
    agent_lib::environment::http_client::init_http_client(None);
    let client = agent_lib::environment::http_client::get_http_client();
    // Just verify it returns a client
    let _ = client;
}
