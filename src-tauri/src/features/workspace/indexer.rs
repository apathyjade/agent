use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

/// 项目语言枚举
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectLanguage {
    Rust,
    TypeScript,
    Python,
    Go,
    Java,
    JavaScript,
    Other(String),
}

/// 单个依赖及其可选版本号
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub name: String,
    pub version: Option<String>,
}

/// [`CodebaseIndexer`] 产出的完整代码库索引结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodebaseIndex {
    pub root: PathBuf,
    pub language: Option<ProjectLanguage>,
    pub framework: Option<String>,
    pub dependencies: Vec<Dependency>,
    pub file_count: usize,
    pub dir_count: usize,
    pub last_indexed: String,
    pub file_types: HashMap<String, usize>,
}

/// 扫描项目目录，识别语言、框架、依赖并统计文件类型。
pub struct CodebaseIndexer;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

impl CodebaseIndexer {
    /// 为给定根路径建立代码库索引。
    ///
    /// 返回包含语言、框架、依赖与文件统计的 [`CodebaseIndex`]。
    pub fn index(root: &Path) -> Result<CodebaseIndex> {
        if !root.exists() || !root.is_dir() {
            return Err(format!(
                "Path does not exist or is not a directory: {}",
                root.display()
            )
            .into());
        }

        let (language, framework) = Self::detect_project(root);
        let dependencies = Self::detect_dependencies(root, &language);
        let (file_count, dir_count, file_types) = Self::count_files(root);

        Ok(CodebaseIndex {
            root: root.to_path_buf(),
            language,
            framework,
            dependencies,
            file_count,
            dir_count,
            last_indexed: Utc::now().to_rfc3339(),
            file_types,
        })
    }

    // ── 项目识别 ──────────────────────────────────────────────────────────

    /// 通过已知的清单文件识别项目语言与可选框架。
    fn detect_project(root: &Path) -> (Option<ProjectLanguage>, Option<String>) {
        if root.join("Cargo.toml").exists() {
            let framework = Self::detect_rust_framework(root);
            return (Some(ProjectLanguage::Rust), framework);
        }
        if root.join("package.json").exists() {
            return Self::detect_node_framework(root);
        }
        if root.join("pyproject.toml").exists() || root.join("requirements.txt").exists() {
            return (Some(ProjectLanguage::Python), None);
        }
        if root.join("go.mod").exists() {
            return (Some(ProjectLanguage::Go), None);
        }
        (None, None)
    }

    /// 扫描 Rust 的 `Cargo.toml`，查找已知的 Web/桌面框架关键字。
    fn detect_rust_framework(root: &Path) -> Option<String> {
        let content = fs::read_to_string(root.join("Cargo.toml")).ok()?;
        let frameworks = ["tauri", "actix-web", "axum", "rocket", "warp"];
        for fw in &frameworks {
            if content
                .lines()
                .any(|line| line.trim().starts_with(fw) || line.contains(&format!("\"{}\"", fw)))
            {
                return Some(fw.to_string());
            }
        }
        None
    }

    /// 扫描 Node.js 的 `package.json`，识别框架关键字并区分 TypeScript
    /// 与纯 JavaScript。
    fn detect_node_framework(root: &Path) -> (Option<ProjectLanguage>, Option<String>) {
        let content = match fs::read_to_string(root.join("package.json")) {
            Ok(c) => c,
            Err(_) => return (None, None),
        };
        let json: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => return (None, None),
        };

        let mut framework: Option<String> = None;
        let framework_keywords: [(&str, &str); 4] = [
            ("next", "Next.js"),
            ("react", "React"),
            ("vue", "Vue"),
            ("@tauri-apps/api", "Tauri"),
        ];

        for section in &["dependencies", "devDependencies"] {
            if let Some(deps) = json.get(*section).and_then(|v| v.as_object()) {
                for key in deps.keys() {
                    for (kw, fw_name) in &framework_keywords {
                        if key == kw {
                            framework = Some(fw_name.to_string());
                        }
                    }
                }
            }
        }

        let is_typescript = root.join("tsconfig.json").exists()
            || json
                .get("devDependencies")
                .and_then(|v| v.as_object())
                .map(|d| d.contains_key("typescript"))
                .unwrap_or(false);

        let language = if is_typescript {
            Some(ProjectLanguage::TypeScript)
        } else {
            Some(ProjectLanguage::JavaScript)
        };

        (language, framework)
    }

    // ── 依赖解析 ──────────────────────────────────────────────────────────

    /// 按语言路由到对应的依赖解析器。
    fn detect_dependencies(root: &Path, language: &Option<ProjectLanguage>) -> Vec<Dependency> {
        match language {
            Some(ProjectLanguage::Rust) => Self::parse_cargo_deps(root),
            Some(ProjectLanguage::TypeScript) | Some(ProjectLanguage::JavaScript) => {
                Self::parse_npm_deps(root)
            }
            Some(ProjectLanguage::Python) => Self::parse_python_deps(root),
            Some(ProjectLanguage::Go) => Self::parse_go_deps(root),
            _ => Vec::new(),
        }
    }

    /// 解析 Cargo.toml 的 `[dependencies]`。
    ///
    /// 同时支持简单形式（`name = "version"`）与内联表形式
    /// （`name = { version = "...", … }`）。
    fn parse_cargo_deps(root: &Path) -> Vec<Dependency> {
        let content = match fs::read_to_string(root.join("Cargo.toml")) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };

        let mut deps = Vec::new();
        let mut in_target = false;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                in_target = trimmed.starts_with("[dependencies]");
                continue;
            }
            if !in_target || trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // 尝试在第一个 `=` 处切分
            if let Some(eq_pos) = trimmed.find('=') {
                let name = trimmed[..eq_pos].trim().to_string();
                let value = trimmed[eq_pos + 1..].trim();

                let version = if value.starts_with('"') {
                    // 简单形式：name = "version"
                    Some(value.trim_matches('"').to_string())
                } else if value.starts_with('{') {
                    // 内联表：name = { version = "...", … }
                    let marker = "version = \"";
                    if let Some(ver_start) = value.find(marker) {
                        let rest = &value[ver_start + marker.len()..];
                        rest.find('"').map(|ver_end| rest[..ver_end].to_string())
                    } else {
                        None
                    }
                } else {
                    None
                };

                deps.push(Dependency { name, version });
            }
        }

        deps
    }

    /// 解析 package.json 中的 `dependencies` 与 `devDependencies`。
    fn parse_npm_deps(root: &Path) -> Vec<Dependency> {
        let content = match fs::read_to_string(root.join("package.json")) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let json: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };

        let mut deps = Vec::new();
        for section in &["dependencies", "devDependencies"] {
            if let Some(obj) = json.get(*section).and_then(|v| v.as_object()) {
                for (name, version_val) in obj {
                    deps.push(Dependency {
                        name: name.clone(),
                        version: version_val.as_str().map(|s| s.to_string()),
                    });
                }
            }
        }
        deps
    }

    /// 从 `pyproject.toml`（PEP 621）或 `requirements.txt` 解析依赖。
    fn parse_python_deps(root: &Path) -> Vec<Dependency> {
        let mut deps = Vec::new();

        // ── pyproject.toml 依赖 ──────────────────────────────────────────
        let pyproject_path = root.join("pyproject.toml");
        if pyproject_path.exists() {
            if let Ok(content) = fs::read_to_string(&pyproject_path) {
                // PEP 621 在 `[project]` 下用 `dependencies = ["pkg>=1", ...]`。
                // 这里只找依赖数组内被引号包裹的包声明行。
                let mut in_array = false;
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.contains("dependencies") && trimmed.contains('[') {
                        in_array = true;
                        continue;
                    }
                    if in_array {
                        if trimmed.contains(']') {
                            in_array = false;
                            // `]` 位于某项末尾时，把该项也解析掉
                            let before_bracket = trimmed.split(']').next().unwrap_or("");
                            let item = before_bracket
                                .trim()
                                .trim_start_matches('"')
                                .trim_end_matches(',')
                                .trim_end_matches('"')
                                .trim();
                            if !item.is_empty() {
                                Self::parse_python_dep_item(item, &mut deps);
                            }
                            continue;
                        }
                        let item = trimmed
                            .trim_start_matches('"')
                            .trim_end_matches(',')
                            .trim_end_matches('"')
                            .trim();
                        if !item.is_empty() {
                            Self::parse_python_dep_item(item, &mut deps);
                        }
                    }
                }
            }
        }

        // ── requirements.txt 依赖 ────────────────────────────────────────
        let req_path = root.join("requirements.txt");
        if req_path.exists() {
            if let Ok(content) = fs::read_to_string(&req_path) {
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with('#') {
                        continue;
                    }
                    // 去掉可选的版本限定符
                    let op_pos = trimmed.find(|c: char| "=><~!".contains(c));
                    if let Some(pos) = op_pos {
                        let dep_name = trimmed[..pos].trim().to_string();
                        let version = Some(trimmed[pos..].trim().to_string());
                        if !dep_name.is_empty() && !deps.iter().any(|d| d.name == dep_name) {
                            deps.push(Dependency {
                                name: dep_name,
                                version,
                            });
                        }
                    } else if !trimmed.is_empty()
                        && !deps.iter().any(|d: &Dependency| d.name == trimmed)
                    {
                        deps.push(Dependency {
                            name: trimmed.to_string(),
                            version: None,
                        });
                    }
                }
            }
        }

        deps
    }

    /// 解析单条 Python 依赖声明；不存在同名项时才加入 `deps`。
    fn parse_python_dep_item(item: &str, deps: &mut Vec<Dependency>) {
        let op_pos = item.find(|c: char| "=><~!".contains(c));
        if let Some(pos) = op_pos {
            let dep_name = item[..pos].trim().to_string();
            let version = Some(item[pos..].trim().to_string());
            if !dep_name.is_empty() && !deps.iter().any(|d: &Dependency| d.name == dep_name) {
                deps.push(Dependency {
                    name: dep_name,
                    version,
                });
            }
        } else if !item.is_empty() && !deps.iter().any(|d: &Dependency| d.name == item) {
            deps.push(Dependency {
                name: item.to_string(),
                version: None,
            });
        }
    }

    /// `go.mod` 的简化解析器 —— 只收集 `require` 指令。
    fn parse_go_deps(root: &Path) -> Vec<Dependency> {
        let content = match fs::read_to_string(root.join("go.mod")) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };

        let mut deps = Vec::new();
        let mut in_block = false;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") {
                continue;
            }

            if trimmed.starts_with("require (") {
                in_block = true;
                continue;
            }
            if in_block && trimmed == ")" {
                in_block = false;
                continue;
            }

            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            let (name, version) = if in_block {
                // 块形式："github.com/foo/bar v1.0.0"
                if parts.len() >= 2 {
                    (parts[0].to_string(), Some(parts[1].to_string()))
                } else {
                    continue;
                }
            } else if trimmed.starts_with("require ") && parts.len() >= 3 {
                // 单行形式："require github.com/foo/bar v1.0.0"
                (parts[1].to_string(), Some(parts[2].to_string()))
            } else {
                continue;
            };

            if !name.is_empty() && !deps.iter().any(|d: &Dependency| d.name == name) {
                deps.push(Dependency {
                    name,
                    version,
                });
            }
        }

        deps
    }

    // ── 文件统计 ──────────────────────────────────────────────────────────

    /// 递归遍历 `root` 统计文件与目录数量，跳过标准可忽略目录。
    fn count_files(root: &Path) -> (usize, usize, HashMap<String, usize>) {
        let skip_dirs: HashSet<&str> = [
            ".git",
            "node_modules",
            "target",
            ".next",
            "dist",
            "build",
            ".cache",
            "__pycache__",
            ".venv",
            "venv",
            ".svelte-kit",
        ]
        .iter()
        .cloned()
        .collect();

        let mut file_count = 0;
        let mut dir_count = 0;
        let mut file_types = HashMap::new();

        fn visit(
            dir: &Path,
            file_count: &mut usize,
            dir_count: &mut usize,
            file_types: &mut HashMap<String, usize>,
            skip_dirs: &HashSet<&str>,
        ) {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            if skip_dirs.contains(name) {
                                continue;
                            }
                        }
                        *dir_count += 1;
                        visit(path.as_path(), file_count, dir_count, file_types, skip_dirs);
                    } else if path.is_file() {
                        *file_count += 1;
                        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                            if !ext.is_empty() {
                                *file_types.entry(ext.to_string()).or_insert(0) += 1;
                            }
                        }
                    }
                }
            }
        }

        visit(
            root,
            &mut file_count,
            &mut dir_count,
            &mut file_types,
            &skip_dirs,
        );
        (file_count, dir_count, file_types)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ── 辅助函数 ────────────────────────────────────────────────────────────

    fn create_rust_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "test"
version = "0.1.0"

[dependencies]
serde = "1.0"
tokio = { version = "1", features = ["full"] }
"#,
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/main.rs"), "fn main() {}").unwrap();
        dir
    }

    fn create_node_project(typescript: bool) -> TempDir {
        let dir = TempDir::new().unwrap();
        let ts_dep = if typescript {
            r#","devDependencies": { "typescript": "^5.0" }"#
        } else {
            ""
        };
        fs::write(
            dir.path().join("package.json"),
            format!(
                r#"{{
    "name": "test",
    "dependencies": {{ "react": "^18.0" }}{}
}}"#,
                ts_dep
            ),
        )
        .unwrap();
        if typescript {
            fs::write(dir.path().join("tsconfig.json"), "{}").unwrap();
        }
        fs::write(dir.path().join("index.js"), "console.log('hi');").unwrap();
        dir
    }

    fn create_python_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            r#"[project]
name = "test"
dependencies = [
    "requests>=2.28",
    "click",
]
"#,
        )
        .unwrap();
        fs::write(dir.path().join("app.py"), "def main(): pass").unwrap();
        dir
    }

    fn create_go_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("go.mod"),
            r#"module example.com/test

go 1.21

require (
    github.com/gin-gonic/gin v1.9.0
    github.com/stretchr/testify v1.8.4
)
"#,
        )
        .unwrap();
        fs::write(dir.path().join("main.go"), "package main").unwrap();
        dir
    }

    // ── Rust 用例 ─────────────────────────────────────────────────────────

    #[test]
    fn test_detect_rust_project() {
        let dir = create_rust_project();
        let index = CodebaseIndexer::index(dir.path()).unwrap();
        assert_eq!(index.language, Some(ProjectLanguage::Rust));
        assert_eq!(index.file_count, 2);
        assert_eq!(index.dir_count, 1);
    }

    #[test]
    fn test_cargo_deps_parsed() {
        let dir = create_rust_project();
        let index = CodebaseIndexer::index(dir.path()).unwrap();
        assert!(!index.dependencies.is_empty());
        assert!(index.dependencies.iter().any(|d| d.name == "serde"));
        assert!(index.dependencies.iter().any(|d| d.name == "tokio"));
    }

    #[test]
    fn test_rust_framework() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "axum-app"
version = "0.1.0"

[dependencies]
axum = "0.7"
tokio = { version = "1", features = ["full"] }
"#,
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/main.rs"), "fn main() {}").unwrap();

        let index = CodebaseIndexer::index(dir.path()).unwrap();
        assert_eq!(index.language, Some(ProjectLanguage::Rust));
        assert_eq!(index.framework, Some("axum".to_string()));
    }

    // ── Node / TypeScript 用例 ────────────────────────────────────────────

    #[test]
    fn test_detect_typescript_project() {
        let dir = create_node_project(true);
        let index = CodebaseIndexer::index(dir.path()).unwrap();
        assert_eq!(index.language, Some(ProjectLanguage::TypeScript));
        assert_eq!(index.framework, Some("React".to_string()));
    }

    #[test]
    fn test_detect_javascript_project() {
        let dir = create_node_project(false);
        let index = CodebaseIndexer::index(dir.path()).unwrap();
        assert_eq!(index.language, Some(ProjectLanguage::JavaScript));
        assert_eq!(index.framework, Some("React".to_string()));
    }

    #[test]
    fn test_npm_deps_parsed() {
        let dir = create_node_project(false);
        let index = CodebaseIndexer::index(dir.path()).unwrap();
        assert!(index.dependencies.iter().any(|d| d.name == "react"));
    }

    // ── Python 用例 ───────────────────────────────────────────────────────

    #[test]
    fn test_detect_python_project() {
        let dir = create_python_project();
        let index = CodebaseIndexer::index(dir.path()).unwrap();
        assert_eq!(index.language, Some(ProjectLanguage::Python));
    }

    #[test]
    fn test_python_deps_parsed() {
        let dir = create_python_project();
        let index = CodebaseIndexer::index(dir.path()).unwrap();
        assert!(index.dependencies.iter().any(|d| d.name == "requests"));
        assert!(index.dependencies.iter().any(|d| d.name == "click"));
    }

    // ── Go 用例 ───────────────────────────────────────────────────────────

    #[test]
    fn test_detect_go_project() {
        let dir = create_go_project();
        let index = CodebaseIndexer::index(dir.path()).unwrap();
        assert_eq!(index.language, Some(ProjectLanguage::Go));
    }

    #[test]
    fn test_go_deps_parsed() {
        let dir = create_go_project();
        let index = CodebaseIndexer::index(dir.path()).unwrap();
        assert!(index
            .dependencies
            .iter()
            .any(|d| d.name == "github.com/gin-gonic/gin"));
    }

    // ── 边界情况 ─────────────────────────────────────────────────────────

    #[test]
    fn test_nonexistent_path() {
        let result = CodebaseIndexer::index(Path::new("/nonexistent/path"));
        assert!(result.is_err());
    }

    #[test]
    fn test_file_types() {
        let dir = create_rust_project();
        let index = CodebaseIndexer::index(dir.path()).unwrap();
        assert!(index.file_types.contains_key("toml"));
        assert!(index.file_types.contains_key("rs"));
        assert_eq!(*index.file_types.get("rs").unwrap(), 1);
    }

    #[test]
    fn test_skip_directories_not_counted() {
        let dir = TempDir::new().unwrap();
        // 有效文件
        fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        // 应被跳过的目录
        fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
        fs::write(dir.path().join("node_modules/pkg/index.js"), "").unwrap();
        fs::create_dir_all(dir.path().join("target/debug")).unwrap();
        fs::write(dir.path().join("target/debug/app.exe"), "").unwrap();

        let index = CodebaseIndexer::index(dir.path()).unwrap();
        assert_eq!(index.file_count, 1);
        assert_eq!(index.dir_count, 0); // 只存在被跳过的目录
        assert!(index.file_types.contains_key("rs"));
        assert!(!index.file_types.contains_key("js"));
        assert!(!index.file_types.contains_key("exe"));
    }

    #[test]
    fn test_no_manifest_project() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("readme.md"), "# hello").unwrap();
        let index = CodebaseIndexer::index(dir.path()).unwrap();
        assert_eq!(index.language, None);
        assert_eq!(index.framework, None);
        assert!(index.dependencies.is_empty());
    }
}
