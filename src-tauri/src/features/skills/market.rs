//! skills.sh 市场检索。
//!
//! 直接调用 skills.sh 的公开搜索接口，把结果整理成前端展示用的
//! [`MarketSkill`]。接口不可用或返回非 2xx 时返回空列表，不影响本地技能
//! 管理。

use serde::{Deserialize, Serialize};

use crate::core::error::Result;

/// skills.sh 搜索 API 的基础地址
const SKILLS_API_BASE: &str = "https://skills.sh";

/// skills.sh `/api/search` 的响应体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchSkillResponse {
    pub query: String,
    #[serde(rename = "searchType")]
    pub search_type: String,
    pub skills: Vec<SearchSkillItem>,
    pub count: i64,
    pub duration_ms: Option<i64>,
}

/// 搜索结果中的单条技能
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchSkillItem {
    pub id: String,
    #[serde(rename = "skillId")]
    pub skill_id: String,
    pub name: String,
    pub installs: i64,
    pub source: String,
}

/// 前端展示用的市场技能
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketSkill {
    pub id: String,
    pub name: String,
    pub installs: i64,
    pub source: String,
    /// 人类可读的安装量展示（如 `1.6M installs`）
    pub description: String,
}

/// 在 skills.sh 市场搜索技能。
///
/// 接口：`GET /api/search?q=<query>&limit=<count>`。
/// 查询至少 3 个字符；不足时用 `skill` 兜底以获得较宽的结果。
pub async fn search_skills(query: &str, limit: Option<i64>) -> Result<Vec<MarketSkill>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("Agent/0.1.0")
        .build()?;

    // 接口要求查询至少 3 个字符
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

/// 用宽泛关键词拉取热门技能。
pub async fn fetch_popular_skills(limit: Option<i64>) -> Result<Vec<MarketSkill>> {
    search_skills("skill", limit).await
}

/// 把安装量格式化成 `1.2M` / `3.4K` 形式。
fn format_installs(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
