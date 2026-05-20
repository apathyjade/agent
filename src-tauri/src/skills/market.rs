use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Base URL for skills.sh search API
const SKILLS_API_BASE: &str = "https://skills.sh";

/// Response from skills.sh /api/search endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchSkillResponse {
    pub query: String,
    #[serde(rename = "searchType")]
    pub search_type: String,
    pub skills: Vec<SearchSkillItem>,
    pub count: i64,
    pub duration_ms: Option<i64>,
}

/// Individual skill item from search results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchSkillItem {
    pub id: String,
    #[serde(rename = "skillId")]
    pub skill_id: String,
    pub name: String,
    pub installs: i64,
    pub source: String,
}

/// Market skill for frontend display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketSkill {
    pub id: String,
    pub name: String,
    pub installs: i64,
    pub source: String,
    /// Human-readable install count display (e.g. "1.6M installs")
    pub description: String,
}

/// Search skills on the skills.sh marketplace
///
/// API: GET /api/search?q=<query>&limit=<count>
/// Minimum query length: 3 characters. Uses "skill" as default for broad results.
pub async fn search_skills(query: &str, limit: Option<i64>) -> Result<Vec<MarketSkill>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("Agent/0.1.0")
        .build()?;

    // Minimum 3 chars per API requirement
    let q = if query.trim().len() < 3 {
        "skill"
    } else {
        query.trim()
    };

    let params = [
        ("q", q.to_string()),
        ("limit", limit.unwrap_or(30).to_string()),
    ];

    let url = format!("{}/api/search", SKILLS_API_BASE);
    let resp = client.get(&url).query(&params).send().await?;

    if !resp.status().is_success() {
        log::warn!("skills.sh search API returned status: {}", resp.status());
        return Ok(vec![]);
    }

    let data: SearchSkillResponse = resp.json().await?;
    let skills: Vec<MarketSkill> = data
        .skills
        .into_iter()
        .map(|item| MarketSkill {
            id: item.skill_id,
            name: item.name,
            installs: item.installs,
            source: item.source,
            description: format!("{} installs", format_installs(item.installs)),
        })
        .collect();

    Ok(skills)
}

/// Fetch popular/top skills by searching with a broad term
pub async fn fetch_popular_skills(limit: Option<i64>) -> Result<Vec<MarketSkill>> {
    search_skills("skill", limit).await
}

fn format_installs(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Resolve npx executable path, trying common variations.
fn resolve_npx() -> String {
    // On Windows, try npx.cmd (batch) first, then npx
    if cfg!(windows) {
        // Check common fnm/node installation paths
        let candidates = ["npx.cmd", "npx"];
        for name in &candidates {
            if let Ok(path) = std::process::Command::new("where")
                .arg(name)
                .output()
            {
                if path.status.success() {
                    let resolved = String::from_utf8_lossy(&path.stdout)
                        .lines()
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    if !resolved.is_empty() {
                        return resolved;
                    }
                }
            }
        }
    }
    "npx".to_string()
}

/// Install a market skill by its source (owner/repo) using npx skills CLI.
///
/// Runs: npx skills add <source> -y -g
/// Installs globally to ~/.agents/skills/<name>/
pub async fn install_market_skill(source: &str) -> Result<String> {
    let npx_path = resolve_npx();

    log::info!("Installing skill from source: {} using npx: {}", source, npx_path);

    let npx_check = tokio::process::Command::new(&npx_path)
        .arg("--version")
        .output()
        .await;

    let npx_path = match npx_check {
        Ok(output) if output.status.success() => npx_path,
        _ => {
            // Fallback: try cmd.exe /c on Windows to get proper PATH resolution
            if cfg!(windows) {
                let cmd_check = tokio::process::Command::new("cmd.exe")
                    .args(["/c", "npx", "--version"])
                    .output()
                    .await;

                match cmd_check {
                    Ok(out) if out.status.success() => {
                        // Use cmd.exe /c for future calls
                        log::info!("Using cmd.exe /c for npx on Windows");
                        return install_via_cmd(source).await;
                    }
                    _ => {
                        return Err(crate::error::AppError::Skill(
                            "npx is not available. Please install Node.js to use the marketplace.".to_string(),
                        ));
                    }
                }
            } else {
                return Err(crate::error::AppError::Skill(
                    "npx is not available. Please install Node.js to use the marketplace.".to_string(),
                ));
            }
        }
    };

    log::info!("npx resolved at: {}", npx_path);

    let install = tokio::process::Command::new(&npx_path)
        .args(["skills", "add", source, "-y", "-g"])
        .output()
        .await
        .map_err(|e| {
            crate::error::AppError::Skill(format!(
                "Failed to run npx skills add: {}",
                e
            ))
        })?;

    if !install.status.success() {
        let stderr = String::from_utf8_lossy(&install.stderr);
        let stdout = String::from_utf8_lossy(&install.stdout);
        log::error!("npx skills add failed. stdout: {} stderr: {}", stdout, stderr);
        return Err(crate::error::AppError::Skill(format!(
            "npx skills add failed: {}",
            stderr
        )));
    }

    // Return global skills directory for rescan
    if let Some(home) = dirs::home_dir() {
        let skills_dir = home.join(".agents").join("skills");
        if skills_dir.exists() {
            return Ok(skills_dir.to_string_lossy().to_string());
        }
    }

    Ok(String::new())
}

/// Fallback installation using cmd.exe /c on Windows for proper PATH resolution.
async fn install_via_cmd(source: &str) -> Result<String> {
    log::info!("Installing via cmd.exe /c npx skills add {}", source);

    let install = tokio::process::Command::new("cmd.exe")
        .args(["/c", "npx", "skills", "add", source, "-y", "-g"])
        .output()
        .await
        .map_err(|e| {
            crate::error::AppError::Skill(format!(
                "Failed to run npx skills add via cmd: {}",
                e
            ))
        })?;

    if !install.status.success() {
        let stderr = String::from_utf8_lossy(&install.stderr);
        let stdout = String::from_utf8_lossy(&install.stdout);
        log::error!("cmd.exe npx skills add failed. stdout: {} stderr: {}", stdout, stderr);
        return Err(crate::error::AppError::Skill(format!(
            "npx skills add failed: {}",
            stderr
        )));
    }

    if let Some(home) = dirs::home_dir() {
        let skills_dir = home.join(".agents").join("skills");
        if skills_dir.exists() {
            return Ok(skills_dir.to_string_lossy().to_string());
        }
    }

    Ok(String::new())
}
