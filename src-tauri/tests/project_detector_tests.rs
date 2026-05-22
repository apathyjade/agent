// ── Project Detector Tests ──

use agent_lib::environment::project::ProjectDetector;
use agent_lib::environment::RuntimeType;

fn create_temp_dir(name: &str) -> (std::path::PathBuf, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    let path = dir.path().join(name);
    std::fs::create_dir_all(&path).expect("Failed to create subdirectory");
    (path, dir)
}

#[tokio::test]
async fn test_parse_nvmrc() {
    let (dir, _guard) = create_temp_dir("test_nvmrc");
    std::fs::write(dir.join(".nvmrc"), "20\n").unwrap();

    let result = ProjectDetector::scan(&dir).await;
    assert!(result.is_ok());
    let scan = result.unwrap();
    assert!(!scan.requirements.is_empty(), "Should detect .nvmrc");
    let has_node = scan.requirements.iter().any(|r| r.runtime_type == RuntimeType::Node);
    assert!(has_node, "Should detect Node requirement");
}

#[tokio::test]
async fn test_parse_python_version() {
    let (dir, _guard) = create_temp_dir("test_python_version");
    std::fs::write(dir.join(".python-version"), "3.12.8\n").unwrap();

    let result = ProjectDetector::scan(&dir).await;
    assert!(result.is_ok());
    let scan = result.unwrap();
    let has_python = scan.requirements.iter().any(|r| r.runtime_type == RuntimeType::Python);
    assert!(has_python, "Should detect Python requirement");
}

#[tokio::test]
async fn test_parse_runtime_version_file() {
    let (dir, _guard) = create_temp_dir("test_runtime_version");
    std::fs::write(
        dir.join(".runtime-version"),
        "version: 1\nruntimes:\n  node: \"20.18.3\"\n  python: \"3.12.8\"\n",
    )
    .unwrap();

    let result = ProjectDetector::scan(&dir).await;
    assert!(result.is_ok());
    let scan = result.unwrap();
    assert_eq!(scan.requirements.len(), 2, "Should detect 2 requirements");
}

#[tokio::test]
async fn test_parse_go_mod() {
    let (dir, _guard) = create_temp_dir("test_go_mod");
    std::fs::write(dir.join("go.mod"), "module example.com/foo\ngo 1.22.4\n").unwrap();

    let result = ProjectDetector::scan(&dir).await;
    assert!(result.is_ok());
    let scan = result.unwrap();
    let has_go = scan.requirements.iter().any(|r| r.runtime_type == RuntimeType::Go);
    assert!(has_go, "Should detect Go requirement");
}

#[tokio::test]
async fn test_parse_package_json_engines() {
    let (dir, _guard) = create_temp_dir("test_package_json");
    std::fs::write(
        dir.join("package.json"),
        r#"{"engines": {"node": ">=18"}}"#,
    )
    .unwrap();

    let result = ProjectDetector::scan(&dir).await;
    assert!(result.is_ok());
    let scan = result.unwrap();
    let has_node = scan.requirements.iter().any(|r| r.runtime_type == RuntimeType::Node);
    assert!(has_node, "Should detect Node from package.json engines");
}

#[tokio::test]
async fn test_scan_empty_directory() {
    let (dir, _guard) = create_temp_dir("test_empty_dir");

    let result = ProjectDetector::scan(&dir).await;
    assert!(result.is_ok());
    let scan = result.unwrap();
    assert!(scan.requirements.is_empty(), "Empty dir should have no requirements");
}

#[tokio::test]
async fn test_cargo_toml_implies_rust() {
    let (dir, _guard) = create_temp_dir("test_cargo");
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();

    let result = ProjectDetector::scan(&dir).await;
    assert!(result.is_ok());
    let scan = result.unwrap();
    let has_rust = scan.requirements.iter().any(|r| r.runtime_type == RuntimeType::Rust);
    assert!(has_rust, "Cargo.toml should imply Rust requirement");
}
