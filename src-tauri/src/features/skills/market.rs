use serde::{Deserialize, Serialize};

use crate::core::error::Result;

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
