# Runtime Version Manager — 详细设计文档

---

## 模块一：运行时扩展（Runtime Extension）

### 1.1 新增运行时类型

#### 1.1.1 目标
从当前 5 种运行时扩展到 8+ 种，第一阶段新增 Rust、Java、Deno。

#### 1.1.2 Rust 安装方案

| 方案 | 优点 | 缺点 | 选择 |
|------|------|------|------|
| 调用 rustup CLI | 官方方案，自动管理 PATH | 依赖 rustup 已安装 | **首选** |
| 直接下载 tarball | 无外部依赖 | 需手动管理多版本 | 降级方案 |

**检测策略**：
```
1. 检测 rustup 是否已安装: `rustup which rustc`
2. 若 rustup 可用 → 通过 rustup 管理版本 (rustup install / default)
3. 若 rustup 不可用 → `where rustc` / `which rustc` 检测 PATH 版本
4. 若无任何版本 → 提示安装 rustup
```

**版本发现**：
```rust
// 方式 A: 通过 rustup 获取
$ rustup toolchain list
stable-x86_64-pc-windows-msvc (default)
nightly-x86_64-pc-windows-msvc
1.83.0-x86_64-pc-windows-msvc

// 方式 B: 解析官方频道文件
// https://static.rust-lang.org/dist/channel-rust-stable.toml
// https://static.rust-lang.org/dist/channel-rust-nightly.toml
```

**RustupVersionSource**:
```rust
struct RustupVersionSource;

impl VersionSource for RustupVersionSource {
    async fn fetch_versions(&self) -> Result<Vec<RuntimeVersion>> {
        // 1. 尝试调用 rustup 列出已安装
        // 2. 尝试从官方 channel 获取最新 stable/nightly
        // 3. 回退到硬编码兜底版本
    }
    
    fn download_url(&self, version: &str, platform: &Platform) -> String {
        // Rust 通常通过 rustup 安装，不需要直接下载
        String::new() // 标记为 "需 rustup"
    }
}
```

#### 1.1.3 Java 安装方案

**API 来源**：Adoptium API

```rust
// GET https://api.adoptium.net/v3/assets/version/{feature_version}
// 参数: os=windows&arch=x64&image_type=jdk&project=jdk&vendor=eclipse
// 
// 返回格式:
{
  "assets": [{
    "version": {
      "major": 21,
      "minor": 0,
      "security": 7,
      "semver": "21.0.7+9",
      "openjdk_version": "21.0.7+9"
    },
    "binaries": [{
      "os": "windows",
      "architecture": "x64",
      "image_type": "jdk",
      "package": {
        "name": "OpenJDK21U-jdk_x64_windows_hotspot_21.0.7_9.zip",
        "link": "https://...download/.../OpenJDK21U-jdk_x64...zip"
      }
    }]
  }],
  "pagination": {
    "next": "...",
    "prev": null
  }
}
```

**支持版本范围**：Java 8, 11, 17, 21, 23 等 LTS + 当前最新。

**安装策略**：
```rust
// 下载后解压
// 检出 bin/java.exe → 设置 JAVA_HOME 环境变量 (仅对 Agent 子进程生效)
// 不修改系统 JAVA_HOME（避免破坏用户已有配置）
```

**JavaVersionSource**:
```rust
struct JavaVersionSource;

impl VersionSource for JavaVersionSource {
    async fn fetch_versions(&self) -> Result<Vec<RuntimeVersion>> {
        // 对每个 feature version (8,11,17,21,23...) 请求 Adoptium API
        let lts_versions = vec![8, 11, 17, 21, 23];
        let mut versions = vec![];
        for feat in lts_versions {
            let url = format!(
                "https://api.adoptium.net/v3/assets/version/{}",
                feat
            );
            // 解析返回 → RuntimeVersion { version, display_name, url, lts: true, ... }
        }
        Ok(versions)
    }
}
```

#### 1.1.4 Deno 安装方案

**API 来源**：GitHub Releases

```rust
// GET https://api.github.com/repos/denoland/deno/releases?per_page=50
//
// 解析 release.tag_name 和 assets 中的平台匹配包
// 返回形如:
[
  {
    "tag_name": "v2.2.0",
    "assets": [
      { "name": "deno-x86_64-pc-windows-msvc.zip", "browser_download_url": "..." },
      { "name": "deno-x86_64-unknown-linux-gnu.zip", "browser_download_url": "..." }
    ]
  }
]
```

**安装策略**：下载 zip → 解压到版本目录 → 符号链接到 `deno.exe`。

#### 1.1.5 UI：运行时类型卡片

每个运行时在管理页中展示为卡片，新增的运行时在卡片上有特别标识：

```
┌──────────────────────────────────────────┐
│  🦀 Rust                                 │
│  ────────────────────────────────        │
│  │  当前: 1.85.0 (stable)               │
│  │  来源: rustup                       │
│  │  已安装: stable, nightly, 1.84.1    │
│  │                                      │
│  │  [安装/切换] [版本列表 ▾]            │
│  └──────────────────────────────────────  │
│                                          │
│  ☕ Java (新) ● NEW                      │
│  ────────────────────────────────        │
│  │  未安装                              │
│  │  可用版本: JDK 21 LTS, JDK 17 LTS    │
│  │                                      │
│  │  [安装 JDK 21] [查看更多版本 ▾]      │
│  └──────────────────────────────────────  │
└──────────────────────────────────────────┘
```

---

### 1.2 动态版本发现

#### 1.2.1 VersionSource Trait 详细定义

```rust
/// 平台架构
#[derive(Debug, Clone)]
pub struct Platform {
    pub os: String,       // "windows", "linux", "macos"
    pub arch: String,     // "x64", "arm64"
    pub ext: String,      // "zip", "tar.gz"
}

/// 一个可用的版本
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeVersion {
    pub runtime_type: RuntimeType,
    pub version: String,
    pub display_name: String,
    pub url: String,
    pub lts: Option<String>,     // "Jod", None if not LTS
    pub is_stable: bool,
    pub release_date: Option<String>,
    pub eol_date: Option<String>,
    pub file_size: Option<u64>,
}

/// 版本来源 trait — 每个运行时一个实现
#[async_trait]
pub trait VersionSource: Send + Sync {
    /// 获取可安装版本列表
    async fn fetch_versions(&self) -> Result<Vec<RuntimeVersion>>;
    
    /// 获取当前平台
    fn current_platform(&self) -> Platform {
        Platform {
            os: if cfg!(target_os = "windows") { "windows" }
                else if cfg!(target_os = "macos") { "macos" }
                else { "linux" }.to_string(),
            arch: if cfg!(target_arch = "x86_64") { "x64" }
                  else { "arm64" }.to_string(),
            ext: if cfg!(target_os = "windows") { "zip" }
                 else { "tar.gz" }.to_string(),
        }
    }
}
```

#### 1.2.2 缓存策略

```rust
/// 版本缓存管理器
pub struct VersionCache {
    db: Arc<Mutex<Database>>,
}

impl VersionCache {
    /// 获取缓存版本，如果 TTL 过期则返回 None
    pub async fn get_cached_versions(
        &self,
        rt: &RuntimeType,
    ) -> Result<Option<(Vec<RuntimeVersion>, bool)>> {
        // 返回 (versions, is_stale)
        // is_stale = true 表示 TTL 已过，建议刷新
    }
    
    /// 更新缓存
    pub async fn update_cache(
        &self,
        rt: &RuntimeType,
        versions: &[RuntimeVersion],
    ) -> Result<()>;
    
    /// TTL: 默认 1 小时，可配置
    pub async fn get_ttl(&self) -> Duration {
        Duration::from_secs(3600)
    }
}
```

#### 1.2.3 运行时注册

```rust
/// 运行时注册表 — 所有运行时类型及其 VersionSource
pub struct RuntimeRegistry {
    sources: HashMap<RuntimeType, Box<dyn VersionSource>>,
    cache: VersionCache,
}

impl RuntimeRegistry {
    pub fn new(db: Arc<Mutex<Database>>) -> Self {
        let mut sources: HashMap<RuntimeType, Box<dyn VersionSource>> = HashMap::new();
        sources.insert(RuntimeType::Node, Box::new(NodeVersionSource));
        sources.insert(RuntimeType::Python, Box::new(PythonVersionSource));
        sources.insert(RuntimeType::Go, Box::new(GoVersionSource));
        sources.insert(RuntimeType::Rust, Box::new(RustupVersionSource));
        sources.insert(RuntimeType::Java, Box::new(JavaVersionSource));
        sources.insert(RuntimeType::Deno, Box::new(DenoVersionSource));
        // ... more
        
        Self { sources, cache: VersionCache::new(db) }
    }
    
    /// 获取可用版本（优先缓存，按需刷新）
    pub async fn get_versions(&self, rt: &RuntimeType) -> Result<Vec<RuntimeVersion>> {
        // 1. 查缓存
        if let Some((cached, stale)) = self.cache.get_cached_versions(rt).await? {
            if !stale {
                return Ok(cached);
            }
            // stale: 后台刷新，返回缓存
            let source = self.sources.get(rt).ok_or(...)?;
            let rt = rt.clone();
            tokio::spawn(async move {
                if let Ok(fresh) = source.fetch_versions().await {
                    self.cache.update_cache(&rt, &fresh).await.ok();
                }
            });
            return Ok(cached);
        }
        
        // 2. 无缓存: 同步获取
        let source = self.sources.get(rt).ok_or(...)?;
        let versions = source.fetch_versions().await?;
        self.cache.update_cache(rt, &versions).await?;
        Ok(versions)
    }
}
```

#### 1.2.4 IPC 新增

```rust
// 新增命令：刷新版本缓存
#[tauri::command]
pub async fn refresh_version_cache(
    state: State<'_, AppState>,
    runtime_type: String,
) -> Result<Vec<RuntimeVersion>> {
    let rt = parse_runtime_type(&runtime_type)?;
    // 强制重新拉取远程 API
    state.runtime_registry.force_refresh(&rt).await?;
    state.runtime_registry.get_versions(&rt).await
}

// 修改：list_available_versions 不再从 installer 硬编码返回
// 改为从 RuntimeRegistry 返回动态版本
```

#### 1.2.5 UI：版本选择器升级

当前版本选择器：
```
Select 组件: ["最新版本", "22.14.0", "20.18.3", "18.20.7"]
```

目标版本选择器：

```
┌──────────────────────────────────────────┐
│  选择版本 — Node.js                       │
├──────────────────────────────────────────┤
│  🔍 [搜索版本...            ]             │
│                                          │
│  ── 推荐 ──                              │
│  ○ 22.14.0  🟢 最新 LTS (Jod)      5.2MB│
│  ○ 20.18.3  🟢 推荐的 LTS (Iron)   5.1MB│
│  ● 18.20.7  🟡 维护期              4.8MB│
│                                          │
│  ── 全部版本 ──                          │
│  ○ 23.0.0   🆕 最新                 5.3MB│
│  ○ 22.13.1  🟢 活跃                5.2MB│
│  ○ 22.12.0  🟢 活跃                5.2MB│
│  ○ 21.7.3   🔴 已 EOL              5.0MB│
│  ...                                      │
│                                          │
│  [确认安装]  [取消]                       │
└──────────────────────────────────────────┘
```

**组件设计**：
```tsx
function VersionSelector({
  runtimeType,
  onSelect,
}: {
  runtimeType: RuntimeType;
  onSelect: (version: string) => void;
}) {
  const { versions, versionsLoading, refreshVersions } = useStore();
  const [search, setSearch] = useState('');
  const [groupBy, setGroupBy] = useState<'status' | 'date'>('status');

  useEffect(() => {
    fetchVersions(runtimeType);
  }, [runtimeType]);

  const grouped = useMemo(() => {
    const filtered = versions.filter(v => 
      v.version.includes(search) || 
      v.display_name.toLowerCase().includes(search.toLowerCase())
    );
    return groupByStatus(filtered); // 推荐 / 全部 / 已安装
  }, [versions, search, groupBy]);

  return (
    <div>
      <SearchInput value={search} onChange={setSearch} />
      <VersionGroup label="推荐" versions={grouped.recommended} />
      <VersionGroup label="全部版本" versions={grouped.all} />
    </div>
  );
}
```

---

## 模块二：项目级版本自动切换

### 2.1 UI 设计

#### 2.1.1 新增「项目绑定」Tab

```
┌──────────────────────────────────────────────────────────┐
│  [版本管理]  [项目绑定]  [系统检测]                       │
├──────────────────────────────────────────────────────────┤
│                                                          │
│  ┌─── 已关联项目 ─────────────────────────────────┐      │
│  │                                                  │      │
│  │  my-app (D:\projects\my-app)                     │      │
│  │  ┌────────────────────────────────────┐          │      │
│  │  │ 运行时    需求       当前      状态  │          │      │
│  │  │ ⚡ Node  20.x      20.18.3    ✅   │          │      │
│  │  │ 🐍 Python 3.12.x  3.12.8    ✅   │          │      │
│  │  │ 🦀 Rust   stable   1.85.0    ✅   │          │      │
│  │  └────────────────────────────────────┘          │      │
│  │  [同步]  [编辑 .runtime-version]  [移除项目]     │      │
│  ├──────────────────────────────────────────────────┤      │
│  │  legacy-api (D:\projects\legacy-api)              │      │
│  │  ⚠️ Node 18.20.7 需要 → 当前 20.18.3             │      │
│  │  [切换到 18.20.7] [忽略]                         │      │
│  ├──────────────────────────────────────────────────┤      │
│  │  new-service (D:\projects\new-service)            │      │
│  │  ❌ 缺少 Go 1.22.4                               │      │
│  │  [安装 Go 1.22.4]                                │      │
│  ├──────────────────────────────────────────────────┤      │
│  │  [+ 添加项目]                                     │      │
│  └──────────────────────────────────────────────────┘      │
│                                                          │
│  ─── 快速操作 ───                                        │
│  [扫描此目录] [扫描所有项目] [对齐全部]                    │
└──────────────────────────────────────────────────────────┘
```

#### 2.1.2 项目绑定交互流程

```
Step 1: 点击「添加项目」
Step 2: 文件选择器 → 选择项目根目录
Step 3: 自动扫描 → 结果显示在弹窗中

  ┌──────────────────────────────────────────────┐
  │  扫描结果: D:\projects\new-service            │
  ├──────────────────────────────────────────────┤
  │                                              │
  │  📄 检测到以下配置文件:                       │
  │  ✅ package.json → engines.node: ">=18"       │
  │  ✅ go.mod      → go 1.22.4                  │
  │                                              │
  │  📋 建议运行时需求:                           │
  │  ├── node ">=18" → 将使用 20.18.3 ✅         │
  │  └── go 1.22.4  → 需要安装                  │
  │                                              │
  │  [确认绑定]  [取消]                           │
  └──────────────────────────────────────────────┘

Step 4: 绑定后，项目出现在列表中
Step 5: 任何时候点击「同步」→ 重新检测项目目录
```

#### 2.1.3 通知机制

当检测到项目运行时状态变化时，通过系统通知栏提示：

```
┌──────────────────────────────────────────────────┐
│  ⚠️ 项目 "legacy-api" 的 Node.js 需求 18.x      │
│  但当前活跃版本为 20.18.3                        │
│                                                  │
│  [切换到 18.20.7]  [忽略]  [不再提醒]            │
└──────────────────────────────────────────────────┘
```

### 2.2 实现方案

#### 2.2.1 ProjectDetector

```rust
/// 项目检测结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRuntimeRequirement {
    pub runtime_type: RuntimeType,
    pub version_spec: String,       // ">=18", "3.12.x", "stable"
    pub source_file: String,        // ".nvmrc", "go.mod"
    pub resolved_version: Option<String>,  // 解析后的精确版本
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectScanResult {
    pub project_path: String,
    pub project_name: String,
    pub requirements: Vec<ProjectRuntimeRequirement>,
    pub errors: Vec<String>,
}

/// 项目检测器
pub struct ProjectDetector;

impl ProjectDetector {
    /// 扫描一个项目目录
    pub async fn scan(path: &Path) -> Result<ProjectScanResult> {
        let name = path.file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
            
        let mut requirements = Vec::new();
        
        // 1. 检查 .runtime-version
        if let Some(rv) = Self::parse_runtime_version(path).await {
            requirements.extend(rv);
        }
        
        // 2. 检查 .nvmrc / .node-version
        if !has_runtime_requirement(&requirements, RuntimeType::Node) {
            if let Some(node_ver) = Self::parse_nvmrc(path).await {
                requirements.push(ProjectRuntimeRequirement {
                    runtime_type: RuntimeType::Node,
                    version_spec: node_ver,
                    source_file: ".nvmrc".to_string(),
                    resolved_version: None,
                });
            }
        }
        
        // 3. 检查 .python-version
        if !has_runtime_requirement(&requirements, RuntimeType::Python) {
            if let Some(py_ver) = Self::parse_python_version(path).await {
                requirements.push(ProjectRuntimeRequirement {
                    runtime_type: RuntimeType::Python,
                    version_spec: py_ver,
                    source_file: ".python-version".to_string(),
                    resolved_version: None,
                });
            }
        }
        
        // 4. 检查 go.mod
        if !has_runtime_requirement(&requirements, RuntimeType::Go) {
            if let Some(go_ver) = Self::parse_go_mod(path).await {
                requirements.push(ProjectRuntimeRequirement {
                    runtime_type: RuntimeType::Go,
                    version_spec: go_ver,
                    source_file: "go.mod".to_string(),
                    resolved_version: None,
                });
            }
        }
        
        // 5. 检查 package.json engines
        if !has_runtime_requirement(&requirements, RuntimeType::Node) {
            if let Some(node_ver) = Self::parse_package_json_engines(path).await {
                requirements.push(ProjectRuntimeRequirement {
                    runtime_type: RuntimeType::Node,
                    version_spec: node_ver,
                    source_file: "package.json".to_string(),
                    resolved_version: None,
                });
            }
        }
        
        // 6. 检查 Cargo.toml
        if !has_runtime_requirement(&requirements, RuntimeType::Rust) {
            if Self::has_cargo_toml(path).await {
                requirements.push(ProjectRuntimeRequirement {
                    runtime_type: RuntimeType::Rust,
                    version_spec: "stable".to_string(),
                    source_file: "Cargo.toml".to_string(),
                    resolved_version: None,
                });
            }
        }
        
        Ok(ProjectScanResult {
            project_path: path.to_string_lossy().to_string(),
            project_name: name,
            requirements,
            errors: vec![],
        })
    }
    
    /// 解析 .nvmrc
    async fn parse_nvmrc(path: &Path) -> Option<String> {
        let nvmrc_path = path.join(".nvmrc");
        if !nvmrc_path.exists() { return None; }
        let content = std::fs::read_to_string(nvmrc_path).ok()?;
        let version = content.trim().to_string();
        if version.is_empty() { return None; }
        Some(version) // "20", "18", "lts/*", "20.18.3"
    }
    
    /// 解析 .python-version
    async fn parse_python_version(path: &Path) -> Option<String> {
        let py_path = path.join(".python-version");
        if !py_path.exists() { return None; }
        let content = std::fs::read_to_string(py_path).ok()?;
        let version = content.trim().to_string();
        if version.is_empty() { return None; }
        Some(version) // "3.12.8", "3.11"
    }
    
    /// 从 go.mod 解析 Go 版本
    async fn parse_go_mod(path: &Path) -> Option<String> {
        let go_mod = path.join("go.mod");
        if !go_mod.exists() { return None; }
        let content = std::fs::read_to_string(go_mod).ok()?;
        // 匹配 "go 1.22" 行
        let re = regex::Regex::new(r"(?m)^go\s+(\d+\.\d+)").ok()?;
        let cap = re.captures(&content)?;
        Some(cap[1].to_string()) // "1.22"
    }
    
    /// 从 package.json engines 解析
    async fn parse_package_json_engines(path: &Path) -> Option<String> {
        let pkg = path.join("package.json");
        if !pkg.exists() { return None; }
        let content = std::fs::read_to_string(pkg).ok()?;
        let json: serde_json::Value = serde_json::from_str(&content).ok()?;
        let engines = json.get("engines")?;
        engines.get("node")?.as_str().map(|s| s.to_string())
    }
    
    /// 解析 .runtime-version (YAML)
    async fn parse_runtime_version(path: &Path) -> Option<Vec<ProjectRuntimeRequirement>> {
        let rv_path = path.join(".runtime-version");
        if !rv_path.exists() { return None; }
        let content = std::fs::read_to_string(rv_path).ok()?;
        // YAML 解析 → Vec<ProjectRuntimeRequirement>
        // ...
    }
}
```

#### 2.2.2 项目绑定存储

```rust
/// 绑定的项目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundProject {
    pub id: String,
    pub path: String,          // D:\projects\my-app
    pub name: String,          // my-app
    pub auto_sync: bool,       // 自动同步开关
    pub last_scan: String,     // ISO datetime
    pub requirements: Vec<ProjectRuntimeRequirement>,
}
```

**持久化**：新增 SQLite 表 `bound_projects`

```sql
CREATE TABLE bound_projects (
    id TEXT PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    auto_sync BOOLEAN DEFAULT 1,
    last_scan TEXT,
    requirements TEXT,       -- JSON
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now'))
);
```

#### 2.2.3 版本解析与对齐

```rust
/// 将版本规格解析为精确版本号
pub struct VersionResolver {
    registry: Arc<RuntimeRegistry>,
    aliases: Arc<AliasManager>,
}

impl VersionResolver {
    /// 解析版本规格 → 精确版本号
    /// "20" → "20.18.3" (20.x 最新)
    /// "lts" → "22.14.0" (当前最新 LTS)
    /// "stable" → "1.85.0" (Rust stable)
    /// ">=18" → "20.18.3" (满足条件的最新)
    /// "^20.0.0" → "20.18.3"
    pub async fn resolve(
        &self,
        rt: &RuntimeType,
        spec: &str,
    ) -> Result<String> {
        // 1. 检查是否是别名
        if let Some(alias_version) = self.aliases.resolve(rt, spec).await {
            return Ok(alias_version);
        }
        
        // 2. 检查是否是精确版本
        if is_exact_version(spec) {
            return Ok(spec.to_string());
        }
        
        // 3. 用 semver 范围匹配
        let versions = self.registry.get_versions(rt).await?;
        let range = semver::VersionReq::parse(spec)
            .map_err(|_| AppError::InvalidInput(format!("无效版本范围: {}", spec)))?;
        
        versions.iter()
            .filter_map(|v| {
                semver::Version::parse(&v.version).ok()
                    .map(|sv| (sv, v))
            })
            .filter(|(sv, _)| range.matches(sv))
            .max_by(|(a, _), (b, _)| a.cmp(b))
            .map(|(_, v)| v.version.clone())
            .ok_or_else(|| AppError::NotFound(
                format!("未找到匹配 {} 的版本", spec)
            ))
    }
}
```

#### 2.2.4 对齐流程

```
用户点击「同步」或「对齐全部」
       │
       ▼
for each bound_project:
    for each requirement in project.requirements:
        │
        ├── 当前版本 = alias_manager.get_active(rt)
        ├── 目标版本 = version_resolver.resolve(rt, spec)
        │
        ├── 目标版本 == 当前版本? → ✅ 已对齐
        │
        ├── 目标版本已安装? 
        │   ├── ✅ → switch_version(rt, target)
        │   └── ❌ → install_runtime(rt, target) → switch_version(rt, target)
        │
        └── 结果汇总 → 通知用户
```

---

## 模块三：版本生命周期管理

### 3.1 生命周期标签数据

#### 3.1.1 数据来源

```rust
/// 版本生命周期信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VersionLifecycle {
    /// 🆕 最新发布
    Latest,
    /// ✅ 活跃 LTS
    Active { codename: String },
    /// 🟡 维护期（仅安全更新）
    Maintenance { eol_date: Option<String> },
    /// 🔴 已停止支持
    Eol { eol_date: String },
    /// 🟢 普通活跃版
    Active,
}

impl VersionLifecycle {
    pub fn label(&self) -> &'static str {
        match self {
            Latest       => "最新",
            Active{..}   => "活跃",
            Maintenance{..} => "维护期",
            Eol{..}      => "已停止支持",
            Active       => "活跃",
        }
    }
    
    pub fn emoji(&self) -> &'static str {
        match self {
            Latest       => "🆕",
            Active{..}   => "✅",
            Maintenance{..} => "🟡",
            Eol{..}      => "🔴",
            Active       => "🟢",
        }
    }
    
    pub fn color_class(&self) -> &'static str {
        match self {
            Latest | Active{..} | Active
                => "text-green-600 dark:text-green-400",
            Maintenance{..}
                => "text-yellow-600 dark:text-yellow-400",
            Eol{..}
                => "text-red-600 dark:text-red-400",
        }
    }
}
```

#### 3.1.2 各运行时的生命周期判断

```rust
impl VersionLifecycle {
    /// 判断 Node.js 版本的生命周期
    pub fn from_node_version(version: &str, lts_codename: Option<&str>) -> Self {
        // Node.js index.json 返回:
        // { "version": "v22.14.0", "lts": "Jod", ... }
        // { "version": "v23.0.0", "lts": false, ... }
        // { "version": "v18.20.7", "lts": "Iron", ... }
        match lts_codename {
            Some(name) if !name.is_empty() => {
                // 检查是否为最新的 LTS
                // 如果是 → Active { Active }
                // 如果是旧的 LTS → 检查日期
                Self::Active { codename: name.to_string() }
            }
            Some(_) => Self::Active,
            None => {
                // 非 LTS 版本: 判断是否为最新
                // 或判断是否为 EOL 版本
                Self::Active // 简化
            }
        }
    }
    
    /// 判断 Python 版本的生命周期
    pub fn from_python_version(version: &str) -> Self {
        // https://devguide.python.org/versions/
        // 3.12: 2023-10 发布 → 2028-10 EOL (安全维护)
        // 3.11: 2022-10 发布 → 2027-10 EOL
        // 3.10: 2021-10 发布 → 2026-10 EOL
        // 2.7: 已 EOL
        // 硬编码常见版本的 EOL 日期，或从远程 API 获取
        match version {
            v if v.starts_with("3.13") => Self::Latest,
            v if v.starts_with("3.12") => Self::Active { codename: String::new() },
            v if v.starts_with("3.11") => Self::Maintenance { eol_date: Some("2027-10".into()) },
            v if v.starts_with("3.10") => Self::Maintenance { eol_date: Some("2026-10".into()) },
            v if v.starts_with("3.9")  => Self::Eol { eol_date: "2025-10".into() },
            v if v.starts_with("2.")   => Self::Eol { eol_date: "2020-01".into() },
            _ => Self::Active,
        }
    }
    
    /// 判断 Go 版本的生命周期
    pub fn from_go_version(version: &str) -> Self {
        // Go: 每个大版本有约 2 个 minor 更新，然后 EOL
        // 1.22: 2024-02 → 1.22.x 最新为活跃
        // 1.21: 2023-08 → maintenance
        // 1.20: 2023-02 → EOL
        let major_minor = version.split('.').take(2).collect::<Vec<_>>().join(".");
        match major_minor.as_str() {
            "1.24" | "1.23" => Self::Latest,
            "1.22" => Self::Active,
            "1.21" => Self::Maintenance { eol_date: None },
            _ if major_minor < "1.21" => Self::Eol { eol_date: "unknown".into() },
            _ => Self::Active,
        }
    }
}
```

### 3.2 UI：版本时间线

```
┌──────────────────────────────────────────────┐
│  Node.js 版本发布历史                          │
├──────────────────────────────────────────────┤
│                                              │
│  🆕 23.0.0          2025-05-01  最新         │
│  ✅ 22.14.0 (Jod)    2025-04-23  活跃 LTS    │
│  ✅ 22.13.1 (Jod)    2025-03-11  活跃        │
│  ✅ 22.12.0 (Jod)    2025-02-13  活跃        │
│  🟡 20.18.3 (Iron)   2025-04-02  维护期      │
│      ↓ 2026-04-30 EOL                         │
│  🟡 18.20.7 (Hydro)  2025-04-02  维护期      │
│      ↓ 2025-10-22 EOL ◀ 即将 EOL             │
│  🔴 16.20.7 (Gallium)         已 EOL         │
│  🔴 14.21.3 (Fermium)         已 EOL         │
│                                              │
│  [当前使用: 20.18.3]                          │
├──────────────────────────────────────────────┤
│  升级建议:                                    │
│  Node.js 22.14.0 LTS 可用                     │
│  包含性能提升 + 安全修复                      │
│  [升级到 22.14.0]  [忽略此版本]               │
└──────────────────────────────────────────────┘
```

**实现思路**：用 CSS timeline + `VersionLifecycle` 颜色渲染，每个版本为一个 timeline item。

### 3.3 升级提醒实现

```rust
/// 检查版本更新
pub async fn check_version_updates(
    runtime_manager: &RuntimeManager,
    registry: &RuntimeRegistry,
) -> Result<Vec<VersionUpdate>> {
    let mut updates = Vec::new();
    
    for rt in RuntimeType::all() {
        let current_info = runtime_manager.detect(rt).await;
        if let Some(ref current_version) = current_info.version {
            let available = registry.get_versions(rt).await?;
            
            // 找到比当前更新的版本
            let current_semver = semver::Version::parse(current_version).ok();
            let newer = available.iter()
                .filter_map(|v| {
                    let sv = semver::Version::parse(&v.version).ok()?;
                    current_semver.as_ref().map(|c| (sv > *c, sv, v))
                })
                .filter(|(is_newer, _, _)| *is_newer)
                .collect::<Vec<_>>();
            
            if !newer.is_empty() {
                let latest = newer.last().unwrap(); // sorted by version
                let current_lifecycle = VersionLifecycle::from_runtime(rt, &current_version);
                
                // 仅在有重大更新时提醒
                if matches!(current_lifecycle, 
                    VersionLifecycle::Eol{..} | VersionLifecycle::Maintenance{..}
                ) {
                    updates.push(VersionUpdate {
                        runtime_type: rt.clone(),
                        current_version: current_version.clone(),
                        latest_version: latest.2.version.clone(),
                        latest_lifecycle: VersionLifecycle::from_runtime(rt, &latest.2.version),
                        reason: match current_lifecycle {
                            VersionLifecycle::Eol{..} => "当前版本已停止支持".to_string(),
                            VersionLifecycle::Maintenance{..} => "当前版本仅维护期".to_string(),
                            _ => "新版本可用".to_string(),
                        },
                    });
                }
            }
        }
    }
    
    Ok(updates)
}
```

---

## 模块四：CLI 集成

### 4.1 命令树

```
agent runtime
├── ls [runtime-type] [--remote] [--installed]
│   列出运行时版本
│   --remote: 显示远程可用版本
│   --installed: 仅显示已安装版本（默认）
│
├── use <runtime-type> <version>
│   切换运行时版本
│
├── install <runtime-type> [version]
│   安装运行时
│
├── uninstall <runtime-type> <version>
│   卸载指定版本
│
├── default <runtime-type> <version>
│   设置全局默认版本
│
├── project
│   ├── ls         列出所有已绑定项目
│   ├── add <path> 添加项目绑定
│   ├── remove <id> 移除项目
│   └── sync       同步所有项目版本
│
├── status         运行时健康总览
└── check          检查版本更新
```

### 4.2 Tauri CLI Plugin 实现

Tauri 2.x 通过 `tauri-plugin-cli` 或自定义 CLI 入口实现。

```rust
// src-tauri/src/cli.rs
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "agent", about = "Agent CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<AgentCommand>,
}

#[derive(Subcommand)]
pub enum AgentCommand {
    /// 运行时管理
    Runtime {
        #[command(subcommand)]
        action: RuntimeAction,
    },
}

#[derive(Subcommand)]
pub enum RuntimeAction {
    /// 列出运行时版本
    Ls {
        /// 运行时类型 (node, python, ...)
        runtime_type: Option<String>,
        /// 显示远程可用版本
        #[arg(long)]
        remote: bool,
        /// 仅显示已安装版本
        #[arg(long)]
        installed: bool,
    },
    /// 切换版本
    Use {
        runtime_type: String,
        version: String,
    },
    /// 安装版本
    Install {
        runtime_type: String,
        version: Option<String>,
    },
    /// 卸载版本
    Uninstall {
        runtime_type: String,
        version: String,
    },
    /// 设置默认版本
    Default {
        runtime_type: String,
        version: String,
    },
    /// 项目管理
    Project {
        #[command(subcommand)]
        action: ProjectAction,
    },
    /// 运行时健康总览
    Status,
    /// 检查版本更新
    Check,
}

#[derive(Subcommand)]
pub enum ProjectAction {
    Ls,
    Add { path: String },
    Remove { id: String },
    Sync,
}
```

**CLI 输出格式**：
```rust
// 终端友好输出 + JSON 输出（--json 标志）
fn format_runtime_ls(runtimes: &[RuntimeInfo], json: bool) -> String {
    if json {
        serde_json::to_string_pretty(runtimes).unwrap()
    } else {
        let mut output = String::new();
        for rt in runtimes {
            let status = if rt.available { "✅" } else { "❌" };
            output.push_str(&format!(
                "{} {}  {}  {}\n",
                status,
                rt.display_name,
                rt.version.as_deref().unwrap_or("-"),
                rt.executable_path.as_deref().unwrap_or(""),
            ));
        }
        output
    }
}
```

---

## 模块五：UI 全面升级

### 5.1 页面布局重构

```
┌──────────────────────────────────────────────────────────┐
│  [版本管理]  [项目绑定]  [系统检测]  [健康中心]            │  ← Tab 栏
├──────────────────────────────────────────────────────────┤
│                                                          │
│  ┌──────────────────────────────────────────────┐        │
│  │  筛选: [所有运行时 ▾]  [状态 ▾]  [搜索...]   │        │  ← 筛选栏
│  └──────────────────────────────────────────────┘        │
│                                                          │
│  ┌──────────────────────────────────────────────┐        │
│  │  🦀 Rust                                      │        │
│  │  ─────────────────────────────────────        │        │
│  │  版本: 1.85.0 (stable)  ● 正常                │        │
│  │  ─────────────────────────────────────        │        │
│  │  已安装版本:                                   │        │
│  │  [1.85.0] ● 当前  [1.84.1]  [nightly]        │        │
│  │  远程版本:                                     │        │
│  │  [1.85.0] [1.84.1] [1.83.0] ...              │        │
│  │  ─────────────────────────────────────        │        │
│  │  项目绑定: my-app                              │        │
│  └──────────────────────────────────────────────┘        │
│                                                          │
│  ┌──────────────────────────────────────────────┐        │
│  │  ☕ Java (JDK)  ● NEW                        │        │
│  │  ─────────────────────────────────────        │        │
│  │  状态: ❌ 未安装                              │        │
│  │  一键安装: [安装 JDK 21 LTS]                 │        │
│  └──────────────────────────────────────────────┘        │
│                                                          │
│  ┌──────────────────────────────────────────────┐        │
│  │  ⚡ Node.js                                   │        │
│  │  ─────────────────────────────────────        │        │
│  │  版本: 20.18.3 (Iron) 🟡 维护期              │        │
│  │  升级建议: → 22.14.0 (Jod) ✅ LTS            │        │
│  │  [升级] [查看版本历史]                        │        │
│  └──────────────────────────────────────────────┘        │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

### 5.2 组件层次

```tsx
<RuntimeManagerPage>
  ├── <RuntimeTabs>
  │   ├── Tab: 版本管理 (VersionManagement)
  │   ├── Tab: 项目绑定 (ProjectBinding)
  │   ├── Tab: 系统检测 (SystemDetection)
  │   └── Tab: 健康中心 (HealthCenter)
  │
  ├── <VersionManagement>
  │   ├── <FilterBar> — 搜索、过滤、排序
  │   ├── <RuntimeCardList>
  │   │   └── <RuntimeCard> (per runtime)
  │   │       ├── <VersionBadge> — 当前版本 + 生命周期标签
  │   │       ├── <InstalledVersionChips> — 已安装版本标签云
  │   │       ├── <RemoteVersionList> — 可安装版本列表
  │   │       ├── <UpgradeSuggestion> — 升级建议卡片
  │   │       └── <ProjectBindings> — 关联项目列表
  │   └── <BulkActions>
  │       ├── 批量安装
  │       └── 全部升级
  │
  ├── <ProjectBinding>
  │   ├── <ProjectList>
  │   │   └── <ProjectCard> (per project)
  │   │       ├── <RequirementTable> — 运行时需求 vs 当前状态
  │   │       └── <ProjectActions> — 同步、编辑、移除
  │   └── <AddProjectButton>
  │
  ├── <SystemDetection>
  │   ├── System PATH 检测结果（只读列表）
  │   └── 冲突检测（PATH 上多个同名 exe）
  │
  └── <HealthCenter>
      ├── <HealthSummaryCard>
      ├── <UpdateNotificationList>
      └── <DiagnosticResultList>
```

### 5.3 关键组件：RuntimeCard

```tsx
interface RuntimeCardProps {
  runtimeType: RuntimeType;
  info: RuntimeInfo;
  versions: RuntimeVersion[];
  upgrades: UpgradeInfo[];
  projectBindings: ProjectBinding[];
  onInstall: (version: string) => void;
  onSwitch: (version: string) => void;
  onUninstall: (version: string) => void;
  onShowTimeline: () => void;
}

function RuntimeCard({
  runtimeType, info, versions, upgrades,
  projectBindings, onInstall, onSwitch, onUninstall, onShowTimeline,
}: RuntimeCardProps) {
  const lifecycle = getLifecycle(runtimeType, info.version);
  const [expanded, setExpanded] = useState(false);
  
  return (
    <div className="bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700">
      {/* Header */}
      <div className="p-4 flex items-center gap-4" onClick={() => setExpanded(!expanded)}>
        <RuntimeIcon type={runtimeType} />
        <div className="flex-1">
          <div className="flex items-center gap-2">
            <h3>{info.display_name}</h3>
            {lifecycle && <LifecycleBadge lifecycle={lifecycle} />}
            {!info.available && <span className="text-xs text-red-500">未安装</span>}
          </div>
          <p className="text-xs text-gray-500">
            {info.available ? `${info.version}` : '未安装'}
            {info.executable_path && ` · ${info.executable_path}`}
          </p>
        </div>
        <div className="flex items-center gap-2">
          {upgrades.length > 0 && <UpgradeButton upgrade={upgrades[0]} />}
          <IconButton icon={ChevronDown} rotated={expanded} />
        </div>
      </div>
      
      {/* Expanded content */}
      {expanded && (
        <div className="px-4 pb-4 space-y-3 border-t border-gray-100 dark:border-gray-700 pt-3">
          {/* 版本管理区 */}
          <div>
            <h4 className="text-xs font-medium text-gray-500 mb-2">已安装版本</h4>
            <div className="flex flex-wrap gap-2">
              {info.installed_versions.map(v => (
                <VersionChip
                  key={v.version}
                  version={v.version}
                  isActive={v.is_active}
                  onSwitch={() => onSwitch(v.version)}
                  onUninstall={() => onUninstall(v.version)}
                />
              ))}
              {info.installed_versions.length === 0 && (
                <p className="text-xs text-gray-400">暂无已安装版本</p>
              )}
            </div>
          </div>
          
          {/* 远程版本 */}
          <div>
            <h4 className="text-xs font-medium text-gray-500 mb-2">可用版本</h4>
            <div className="max-h-32 overflow-y-auto space-y-1">
              {versions.map(v => (
                <div key={v.version} className="flex items-center justify-between px-2 py-1 hover:bg-gray-50 dark:hover:bg-gray-700 rounded">
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-mono">{v.version}</span>
                    <LifecycleBadge lifecycle={VersionLifecycle.fromRuntime(runtimeType, &v.version)} />
                  </div>
                  <button onClick={() => onInstall(v.version)} className="...">安装</button>
                </div>
              ))}
            </div>
          </div>
          
          {/* 升级建议 */}
          {upgrades.length > 0 && (
            <UpgradeSuggestion upgrade={upgrades[0]} onUpgrade={() => onInstall(upgrades[0].target_version)} />
          )}
          
          {/* 项目绑定 */}
          {projectBindings.length > 0 && (
            <ProjectBindingList bindings={projectBindings} />
          )}
        </div>
      )}
    </div>
  );
}
```

### 5.4 组件：版本时间线

```tsx
function VersionTimeline({ runtimeType }: { runtimeType: RuntimeType }) {
  const { versions, loading } = useStore();
  
  return (
    <div className="relative pl-6 space-y-0">
      {/* Timeline line */}
      <div className="absolute left-2.5 top-2 bottom-2 w-px bg-gray-200 dark:bg-gray-700" />
      
      {versions.map((v, i) => {
        const lc = getLifecycle(runtimeType, v.version);
        return (
          <div key={v.version} className="relative pb-4 pl-4">
            {/* Dot */}
            <div className={`absolute left-[-18px] top-1.5 w-3 h-3 rounded-full border-2 ${lc.colorClass()} bg-white dark:bg-gray-800`} />
            
            {/* Content */}
            <div className="flex items-center gap-3">
              <span className="font-mono text-sm">{v.version}</span>
              <LifecycleBadge lifecycle={lc} />
              <span className="text-xs text-gray-400">{v.release_date}</span>
            </div>
            {i === 0 && <p className="text-xs text-purple-500 mt-0.5">当前使用</p>}
          </div>
        );
      })}
    </div>
  );
}
```

### 5.5 状态管理新增

```tsx
// runtimeSlice.ts 补充
interface RuntimeSlice {
  // 现有...
  runtimes: RuntimeInfo[];
  
  // 新增
  projectBindings: BoundProject[];
  versionCache: Record<RuntimeType, RuntimeVersion[]>;
  versionCacheLoading: Record<RuntimeType, boolean>;
  upgradeSuggestions: VersionUpdate[];
  healthStatus: HealthCheckResult[];
  
  // 新增 actions
  fetchProjectBindings: () => Promise<void>;
  addProjectBinding: (path: string) => Promise<void>;
  removeProjectBinding: (id: string) => Promise<void>;
  syncProjectVersion: (projectId: string) => Promise<void>;
  syncAllProjects: () => Promise<void>;
  refreshVersionCache: (rt: RuntimeType) => Promise<void>;
  checkVersionUpdates: () => Promise<void>;
}
```

---

## 模块六：版本锁文件 .runtime-version

### 6.1 完整格式规范

```yaml
# .runtime-version — URM 统一运行时描述文件
# 版本锁定在项目根目录，建议提交到 Git

version: 1

runtimes:
  node: "20.18.3"
  python: "3.12.x"
  go: "1.22.4"
  rust: "stable"
  java: "21"
  deno: "2.x"

# 可选: 命名别名（供 CI/CD 引用）
aliases:
  node: "my-node-version"

# 可选: 环境变量设置（Agent 执行时注入）
env:
  NODE_ENV: "development"
  PYTHONUNBUFFERED: "1"

# 可选: 钩子（版本切换时自动执行）
hooks:
  post_checkout:
    - "npm install"    # 切换 Node 版本后
    - "pip install -r requirements.txt"  # 切换 Python 版本后
```

### 6.2 YAML Parser

```rust
/// .runtime-version 文件解析
#[derive(Debug, Deserialize)]
pub struct RuntimeVersionFile {
    pub version: u8,
    pub runtimes: HashMap<String, String>,
    #[serde(default)]
    pub aliases: HashMap<String, String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub hooks: Hooks,
}

#[derive(Debug, Default, Deserialize)]
pub struct Hooks {
    #[serde(default)]
    pub post_checkout: Vec<String>,
}

impl RuntimeVersionFile {
    pub fn parse(content: &str) -> Result<Self> {
        let rv: RuntimeVersionFile = serde_yaml::from_str(content)
            .map_err(|e| AppError::InvalidInput(
                format!(".runtime-version 解析失败: {}", e)
            ))?;
        
        if rv.version != 1 {
            return Err(AppError::InvalidInput(
                format!("不支持的 .runtime-version 版本: {}", rv.version)
            ));
        }
        
        Ok(rv)
    }
    
    /// 将别名映射为 RuntimeType
    pub fn to_requirements(&self) -> Vec<ProjectRuntimeRequirement> {
        self.runtimes.iter().map(|(key, spec)| {
            let rt = RuntimeType::from_str(key)
                .unwrap_or_else(|| panic!("未知运行时: {}", key));
            ProjectRuntimeRequirement {
                runtime_type: rt,
                version_spec: spec.clone(),
                source_file: ".runtime-version".to_string(),
                resolved_version: None,
            }
        }).collect()
    }
}
```

---

## 模块七：系统架构 & 实现路线

### 7.1 完整文件结构

```
src-tauri/src/
├── environment/
│   ├── mod.rs              # RuntimeManager 核心（已有，需扩展现有）
│   ├── detector.rs         # 系统检测器（已有，加冲突检测）
│   ├── installer.rs        # 安装器（已有，扩展新运行时）
│   ├── manifest.rs         # 版本清单（已有）
│   ├── registry.rs         # [新增] RuntimeRegistry + VersionSource trait
│   ├── sources/
│   │   ├── mod.rs          # [新增] 模块导出
│   │   ├── node.rs         # [新增] NodeVersionSource
│   │   ├── python.rs       # [新增] PythonVersionSource
│   │   ├── go.rs           # [新增] GoVersionSource
│   │   ├── rust.rs         # [新增] RustupVersionSource
│   │   ├── java.rs         # [新增] JavaVersionSource
│   │   ├── deno.rs         # [新增] DenoVersionSource
│   │   └── bun.rs          # [新增] BunVersionSource (Phase 2)
│   ├── cache.rs            # [新增] VersionCache
│   ├── lifecycle.rs        # [新增] VersionLifecycle 判断
│   ├── project.rs          # [新增] ProjectDetector + BoundProject
│   ├── resolver.rs         # [新增] VersionResolver (semver)
│   ├── alias.rs            # [新增] AliasManager
│   └── cli.rs              # [新增] CLI 命令解析
│
├── commands/
│   ├── environment.rs      # IPC 命令（现有 12 个 + 扩展）
│   └── ...
│
└── db/
    └── models.rs           # 添加 runtime_version_cache, bound_projects 表

src-ui/src/
├── components/
│   ├── RuntimeManagerPage.tsx   # 主页面（重构 + 多 Tab）
│   ├── RuntimeCard.tsx          # [新增] 运行时卡片
│   ├── VersionSelector.tsx      # [新增] 版本选择器（带搜索/生命周期）
│   ├── VersionTimeline.tsx      # [新增] 版本时间线
│   ├── ProjectBindingPanel.tsx  # [新增] 项目绑定面板
│   ├── ProjectCard.tsx          # [新增] 项目卡片
│   ├── LifecycleBadge.tsx       # [新增] 生命周期标签
│   ├── UpgradeSuggestion.tsx    # [新增] 升级建议
│   └── HealthCenter.tsx         # [新增] 健康中心
│
├── store/
│   ├── runtimeSlice.ts     # Zustand slice（扩展）
│   └── ...
│
└── types/
    └── index.ts            # 类型（扩展）
```

### 7.2 实施路线

```
Phase 1 (核心能力)
═══════════════════
Week 1:
  ├── VersionSource trait + RuntimeRegistry
  ├── Node/Python/Go 动态版本发现（从硬编码改为远程 API）
  ├── VersionCache + SQLite 缓存表
  ├── RuntimeCard 组件（基础版）+ VersionSelector（带生命周期标签）
  └── IPC: refresh_version_cache

Week 2:
  ├── Rust/Java/Deno VersionSource 实现
  ├── installer.rs 扩展支持新运行时
  ├── VersionLifecycle 判断逻辑
  ├── 迁移现有 list_available_versions 到 Registry
  └── RuntimeManagerPage Tab 重构 + 时间线组件

Phase 2 (项目绑定)
═══════════════════
Week 3:
  ├── ProjectDetector（.nvmrc / .python-version / go.mod）
  ├── bound_projects SQLite 表 + CRUD IPC
  ├── VersionResolver（semver 范围解析）
  ├── ProjectBindingPanel UI
  └── 项目扫描 + 一键对齐流程

Week 4:
  ├── .runtime-version 解析器
  ├── AliasManager（default/lts/system）
  ├── 升级提醒机制（check_version_updates）
  └── HealthCenter UI（第一版）

Phase 3 (CLI + 打磨)
═══════════════════
Week 5:
  ├── CLI 子命令 system (ls/use/install/...)
  ├── Tauri CLI plugin 注册
  ├── 终端格式化输出（+ --json flag）
  ├── 系统检测 Tab（PATH 冲突检测）
  └── 批量安装 + 全部升级

Week 6:
  ├── 端到端测试
  ├── 边缘情况处理（无网络/API 失败/权限问题）
  ├── 性能优化（缓存策略调优）
  ├── 文档（用户 + 开发者）
  └── 发布

Phase 4 (后续迭代)
═══════════════════
  ├── Bun/Ruby/Flutter 运行时
  ├── 企业代理支持
  ├── 离线安装包导入
  ├── Agent 深度集成（对话中自动按项目切换）
  └── Docker 镜像版本管理 + 容器运行时
```

### 7.3 关键接口变更

```rust
// environment/mod.rs — 新增模块导出
pub mod registry;
pub mod sources;
pub mod cache;
pub mod lifecycle;
pub mod project;
pub mod resolver;
pub mod alias;

// RuntimeManager 扩展
impl RuntimeManager {
    pub fn registry(&self) -> &RuntimeRegistry;
    pub fn resolver(&self) -> &VersionResolver;
    pub fn alias_manager(&self) -> &AliasManager;
    pub fn project_detector(&self) -> &ProjectDetector;
}

// state.rs — 扩展 AppState
pub struct AppState {
    // 现有...
    pub runtime_registry: Arc<RuntimeRegistry>,
    pub version_resolver: Arc<VersionResolver>,
    pub alias_manager: Arc<AliasManager>,
    pub project_detector: Arc<ProjectDetector>,
}
```

---

## 附录：API 接口文档

### A.1 新增 IPC 命令

```rust
// ── 版本缓存 ──
#[tauri::command]
async fn refresh_version_cache(rt: String) -> Result<Vec<RuntimeVersion>>;
// 强制刷新远程版本列表

// ── 项目绑定 ──
#[tauri::command]
async fn list_bound_projects() -> Result<Vec<BoundProject>>;
#[tauri::command]
async fn add_bound_project(path: String) -> Result<BoundProject>;
#[tauri::command]
async fn remove_bound_project(id: String) -> Result<()>;
#[tauri::command]
async fn scan_project(path: String) -> Result<ProjectScanResult>;
#[tauri::command]
async fn sync_project(id: String) -> Result<SyncResult>;

// ── 版本别名 ──
#[tauri::command]
async fn set_runtime_default(rt: String, version: String) -> Result<()>;
#[tauri::command]
async fn get_runtime_default(rt: String) -> Result<Option<String>>;

// ── 升级提醒 ──
#[tauri::command]
async fn check_runtime_updates() -> Result<Vec<VersionUpdate>>;

// ── 健康检测 ──
#[tauri::command]
async fn runtime_health_check() -> Result<Vec<HealthCheckItem>>;
```

### A.2 修改的现有 IPC

```rust
// list_available_versions — 改为从 RuntimeRegistry 获取
#[tauri::command]
async fn list_available_versions(rt: String) -> Result<Vec<RuntimeVersion>>;
// 现在返回带生命周期标签的完整版本信息
```

---

> 本文档与 PRD 一一对应，涵盖了方向四所有模块的 UI 细节和实现方案。建议按 Phase 1 → Phase 2 → Phase 3 的顺序逐步推进。需要我进一步展开某个具体模块的代码实现吗？
