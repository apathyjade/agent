//! 意图定义的 JSON 导入/导出。
//!
//! 纯函数、不碰数据库：[`parse_intent_array`] 与 [`to_records`] 把不可信的
//! JSON 数组转换成校验过的 [`IntentRecord`]，[`to_json_array`] 再转回同样的
//! 结构。持久化与时间戳由调用方负责，因此整批数据可以先在内存里校验完，
//! 再在一个事务里写入。
//!
//! 导入从不信任传入的 `id`：`intents.id` 是全局主键，重新导入一份导出数据
//! 必须生成新 id，否则整批数据会与来源行冲突。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::core::db::models::IntentRecord;
use crate::core::error::{AppError, Result};

/// `path` 缺省为 `fast`，使只带 `name` 的条目仍然合法。
/// 显式空字符串*不*接受，从而保证 `{fast, deep}` 校验有意义。
fn default_path() -> String {
    "fast".to_string()
}

/// 缺省 `enabled` 视为启用，与列默认值一致。若默认为 `false`，一次最小粘贴
/// 会变成全禁用批次，被「至少一条启用的意图」校验整批拒绝。
fn default_true() -> bool {
    true
}

/// 缺省 `confidence_threshold` 使用列默认值。若默认为 `0.0`，会静默关闭置信
/// 度回退，因为置信度不可能为负。
fn default_threshold() -> f64 {
    0.7
}

/// 缺省 `scope` 使用列默认值，而不是留空。
fn default_scope() -> String {
    "global".to_string()
}

/// §6.3 数组格式中单条意图的传输结构。
///
/// 除 `name` 外所有字段在输入时都可选。`id` 在导出时保留供参考，导入时
/// 忽略。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentJson {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub examples: Vec<String>,
    #[serde(default)]
    pub workflow: String,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub default_agent: String,
    #[serde(default = "default_scope")]
    pub scope: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_threshold")]
    pub confidence_threshold: f64,
    #[serde(default)]
    pub fallback_intent: String,
    #[serde(default = "default_path")]
    pub path: String,
    #[serde(default)]
    pub auto_escalate: bool,
    #[serde(default)]
    pub system_prompt_appendix: String,
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub max_iterations: Option<usize>,
}

/// 把导入文本解析成传输条目。
///
/// 这里只拒绝结构性问题（JSON 格式错误、不是数组、空数组）；字段级校验在
/// [`to_records`] 中进行。
pub fn parse_intent_array(json: &str) -> Result<Vec<IntentJson>> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|err| AppError::InvalidInput(format!("JSON 解析失败：{err}")))?;

    if !value.is_array() {
        return Err(AppError::InvalidInput(
            "导入内容必须是一个 JSON 数组".to_string(),
        ));
    }

    let inputs: Vec<IntentJson> = serde_json::from_value(value)
        .map_err(|err| AppError::InvalidInput(format!("导入内容格式不正确：{err}")))?;

    if inputs.is_empty() {
        return Err(AppError::InvalidInput("导入内容不能是空数组".to_string()));
    }

    Ok(inputs)
}

/// 校验整批数据并转换成全新的记录。
///
/// 只有全部条目都合法才会返回结果，调用方因此可以在一个事务里写入整批。
/// 每条记录会获得新 uuid、`version = 1`、`sort_order = index * 10`，以及
/// 设为 `now` 的 `created_at` / `updated_at`。
///
/// `fallback_intent` 可以按传入的 `id` 或 `name` 引用另一条目；它会被改写
/// 为生成后的 id，无法解析的引用会让整批失败并指出出错条目的 1 基下标。
pub fn to_records(domain_id: &str, inputs: &[IntentJson], now: &str) -> Result<Vec<IntentRecord>> {
    if inputs.is_empty() {
        return Err(AppError::InvalidInput("导入内容不能是空数组".to_string()));
    }

    let mut seen_names: HashMap<String, usize> = HashMap::new();

    for (index, input) in inputs.iter().enumerate() {
        let position = index + 1;
        let name = input.name.trim();

        if name.is_empty() {
            return Err(AppError::InvalidInput(format!(
                "第 {position} 条意图缺少名称"
            )));
        }

        if let Some(first) = seen_names.insert(name.to_string(), position) {
            return Err(AppError::InvalidInput(format!(
                "第 {position} 条意图的名称「{name}」与第 {first} 条重复"
            )));
        }

        let path = input.path.trim();
        if !matches!(path, "fast" | "deep") {
            return Err(AppError::InvalidInput(format!(
                "第 {position} 条意图的 path 必须是 fast 或 deep，实际为「{path}」"
            )));
        }

        let threshold = input.confidence_threshold;
        if !threshold.is_finite() || !(0.0..=1.0).contains(&threshold) {
            return Err(AppError::InvalidInput(format!(
                "第 {position} 条意图的 confidence_threshold 必须在 [0.0, 1.0] 之间，实际为 {threshold}"
            )));
        }
    }

    let generated_ids: Vec<String> = inputs
        .iter()
        .map(|_| uuid::Uuid::new_v4().to_string())
        .collect();

    // 两张查找表都映射到条目下标，使解析结果绑定到生成的 id 而不是借用的
    // 字符串。
    let mut by_id: HashMap<&str, usize> = HashMap::new();
    let mut by_name: HashMap<&str, usize> = HashMap::new();
    for (index, input) in inputs.iter().enumerate() {
        if let Some(incoming_id) = input.id.as_deref() {
            let incoming_id = incoming_id.trim();
            if !incoming_id.is_empty() {
                by_id.entry(incoming_id).or_insert(index);
            }
        }
        by_name.entry(input.name.trim()).or_insert(index);
    }

    let mut records = Vec::with_capacity(inputs.len());
    for (index, input) in inputs.iter().enumerate() {
        let fallback = input.fallback_intent.trim();
        let fallback_intent = if fallback.is_empty() {
            String::new()
        } else {
            match by_id.get(fallback).or_else(|| by_name.get(fallback)) {
                Some(target) => generated_ids[*target].clone(),
                None => {
                    return Err(AppError::InvalidInput(format!(
                        "第 {} 条意图的 fallback_intent「{fallback}」在本批次中找不到对应意图",
                        index + 1
                    )))
                }
            }
        };

        records.push(IntentRecord {
            id: generated_ids[index].clone(),
            domain_id: domain_id.to_string(),
            name: input.name.trim().to_string(),
            description: input.description.clone(),
            examples: input.examples.clone(),
            workflow: input.workflow.clone(),
            allowed_tools: input.allowed_tools.clone(),
            default_agent: input.default_agent.clone(),
            scope: input.scope.clone(),
            enabled: input.enabled,
            version: 1,
            confidence_threshold: input.confidence_threshold,
            fallback_intent,
            path: input.path.trim().to_string(),
            auto_escalate: input.auto_escalate,
            system_prompt_appendix: input.system_prompt_appendix.clone(),
            model_id: input.model_id.clone(),
            max_iterations: input.max_iterations,
            sort_order: index as i64 * 10,
            created_at: now.to_string(),
            updated_at: now.to_string(),
        });
    }

    Ok(records)
}

/// 把记录序列化成可导入的数组格式。
///
/// 内部路由字段一并导出，使往返不丢失业务字段；`sort_order` 省略，因为
/// 数组中的顺序已经表达了它。
pub fn to_json_array(intents: &[IntentRecord]) -> Result<String> {
    let payload: Vec<IntentJson> = intents
        .iter()
        .map(|record| IntentJson {
            id: Some(record.id.clone()),
            name: record.name.clone(),
            description: record.description.clone(),
            examples: record.examples.clone(),
            workflow: record.workflow.clone(),
            allowed_tools: record.allowed_tools.clone(),
            default_agent: record.default_agent.clone(),
            scope: record.scope.clone(),
            enabled: record.enabled,
            confidence_threshold: record.confidence_threshold,
            fallback_intent: record.fallback_intent.clone(),
            path: record.path.clone(),
            auto_escalate: record.auto_escalate,
            system_prompt_appendix: record.system_prompt_appendix.clone(),
            model_id: record.model_id.clone(),
            max_iterations: record.max_iterations,
        })
        .collect();

    Ok(serde_json::to_string_pretty(&payload)?)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::core::error::AppError;
    use crate::features::intent_management::seeds::builtin_software_dev;

    const NOW: &str = "2026-11-04T00:00:00Z";

    /// 取出 `InvalidInput` 错误的信息，使错误的变体不匹配时立刻失败，而不是
    /// 静默按文本匹配。
    fn invalid_message(err: AppError) -> String {
        match err {
            AppError::InvalidInput(msg) => msg,
            other => panic!("期望 AppError::InvalidInput，实际是 {other:?}"),
        }
    }

    fn entry(name: &str, fallback: &str) -> IntentJson {
        IntentJson {
            id: None,
            name: name.to_string(),
            description: String::new(),
            examples: Vec::new(),
            workflow: String::new(),
            allowed_tools: Vec::new(),
            default_agent: String::new(),
            scope: "global".to_string(),
            enabled: true,
            confidence_threshold: 0.7,
            fallback_intent: fallback.to_string(),
            path: "fast".to_string(),
            auto_escalate: false,
            system_prompt_appendix: String::new(),
            model_id: String::new(),
            max_iterations: None,
        }
    }

    #[test]
    fn rejects_non_array_input() {
        let err = parse_intent_array(r#"{"name":"单个对象"}"#).unwrap_err();
        let msg = invalid_message(err);
        assert!(msg.contains("数组"), "错误文案应说明需要数组，实际：{msg}");
    }

    #[test]
    fn rejects_malformed_json() {
        let err = parse_intent_array("{not json").unwrap_err();
        let msg = invalid_message(err);
        assert!(
            msg.contains("JSON 解析失败"),
            "错误文案应说明 JSON 无法解析，实际：{msg}"
        );
    }

    #[test]
    fn minimal_entry_gets_schema_defaults() {
        let inputs = parse_intent_array(r#"[{"name":"最简条目"}]"#).unwrap();
        let records = to_records("domain", &inputs, NOW).unwrap();

        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.name, "最简条目");
        assert!(record.enabled, "缺省 enabled 必须为 true");
        assert_eq!(
            record.confidence_threshold, 0.7,
            "缺省 confidence_threshold 必须取列默认值 0.7"
        );
        assert_eq!(record.scope, "global", "缺省 scope 必须取列默认值");
        assert_eq!(record.path, "fast", "缺省 path 必须为 fast");
    }

    #[test]
    fn rejects_empty_array() {
        let err = parse_intent_array("[]").unwrap_err();
        let msg = invalid_message(err);
        assert!(msg.contains("空"), "错误文案应说明数组为空，实际：{msg}");
    }

    #[test]
    fn rejects_duplicate_names() {
        let inputs = vec![
            entry("重复", ""),
            entry("别的", ""),
            entry("  重复  ", ""),
        ];

        let err = to_records("domain", &inputs, NOW).unwrap_err();
        let msg = invalid_message(err);
        assert!(msg.contains("第 3 条"), "错误文案应指出第 3 条，实际：{msg}");
        assert!(
            msg.contains("重复"),
            "错误文案应说明名称重复，实际：{msg}"
        );
    }

    #[test]
    fn rejects_out_of_range_threshold() {
        let mut too_high = entry("a", "");
        too_high.confidence_threshold = 1.5;
        let msg = invalid_message(to_records("domain", &[too_high], NOW).unwrap_err());
        assert!(msg.contains("第 1 条"), "错误文案应指出第 1 条，实际：{msg}");

        let mut too_low = entry("b", "");
        too_low.confidence_threshold = -0.1;
        let msg = invalid_message(to_records("domain", &[too_low], NOW).unwrap_err());
        assert!(msg.contains("第 1 条"), "错误文案应指出第 1 条，实际：{msg}");

        let mut bad_path = entry("c", "");
        bad_path.path = "slow".to_string();
        let msg = invalid_message(to_records("domain", &[bad_path], NOW).unwrap_err());
        assert!(
            msg.contains("第 1 条") && msg.contains("path"),
            "path 越界必须点名条目与字段，实际：{msg}"
        );

        // 缺省 `path` 回退为 "fast"；显式空字符串不会，从而保证上面的边界
        // 校验仍然有意义。
        let mut empty_path = entry("d", "");
        empty_path.path = String::new();
        assert!(to_records("domain", &[empty_path], NOW).is_err());
    }

    #[test]
    fn unknown_fallback_names_the_offending_index() {
        let inputs = vec![
            entry("a", ""),
            entry("b", "a"),
            entry("c", "不存在的意图"),
        ];

        let err = to_records("domain", &inputs, NOW).unwrap_err();
        let msg = invalid_message(err);
        assert!(msg.contains("第 3 条"), "错误文案应指出第 3 条，实际：{msg}");
        assert!(
            msg.contains("fallback_intent"),
            "错误文案应说明是 fallback_intent 的问题，实际：{msg}"
        );
    }

    #[test]
    fn roundtrip_preserves_business_fields() {
        let (_, originals) = builtin_software_dev();
        let json = to_json_array(&originals).unwrap();
        let inputs = parse_intent_array(&json).unwrap();
        let records = to_records("other-domain", &inputs, NOW).unwrap();

        assert_eq!(records.len(), originals.len(), "往返后条数必须一致");

        for (index, (original, record)) in originals.iter().zip(records.iter()).enumerate() {
            let at = index + 1;
            assert_eq!(record.domain_id, "other-domain", "第 {at} 条领域归属错误");
            assert_eq!(record.version, 1, "第 {at} 条版本必须重置为 1");
            assert_eq!(record.sort_order, index as i64 * 10, "第 {at} 条顺序错误");
            assert_eq!(record.created_at, NOW, "第 {at} 条创建时间错误");
            assert_eq!(record.updated_at, NOW, "第 {at} 条更新时间错误");
            assert_ne!(record.id, original.id, "第 {at} 条必须换用新 id");

            assert_eq!(record.name, original.name, "第 {at} 条 name 丢失");
            assert_eq!(
                record.description, original.description,
                "第 {at} 条 description 丢失"
            );
            assert_eq!(record.examples, original.examples, "第 {at} 条 examples 丢失");
            assert_eq!(record.workflow, original.workflow, "第 {at} 条 workflow 丢失");
            assert_eq!(
                record.allowed_tools, original.allowed_tools,
                "第 {at} 条 allowed_tools 丢失"
            );
            assert_eq!(
                record.default_agent, original.default_agent,
                "第 {at} 条 default_agent 丢失"
            );
            assert_eq!(record.scope, original.scope, "第 {at} 条 scope 丢失");
            assert_eq!(record.enabled, original.enabled, "第 {at} 条 enabled 丢失");
            assert_eq!(
                record.confidence_threshold, original.confidence_threshold,
                "第 {at} 条 confidence_threshold 丢失"
            );
            assert_eq!(record.path, original.path, "第 {at} 条 path 丢失");
            assert_eq!(
                record.auto_escalate, original.auto_escalate,
                "第 {at} 条 auto_escalate 丢失"
            );
            assert_eq!(
                record.system_prompt_appendix, original.system_prompt_appendix,
                "第 {at} 条 system_prompt_appendix 丢失"
            );
            assert_eq!(record.model_id, original.model_id, "第 {at} 条 model_id 丢失");
            assert_eq!(
                record.max_iterations, original.max_iterations,
                "第 {at} 条 max_iterations 丢失"
            );
        }

        // `fallback_intent` 保存的是 id，导入后会变化，因此按相对位置而不是
        // 原始 id 比较目标。
        for (index, (original, record)) in originals.iter().zip(records.iter()).enumerate() {
            let expected = originals
                .iter()
                .position(|candidate| candidate.id == original.fallback_intent);
            let actual = records
                .iter()
                .position(|candidate| candidate.id == record.fallback_intent);

            if original.fallback_intent.is_empty() {
                assert_eq!(actual, None, "第 {} 条空 fallback 必须保持为空", index + 1);
            } else {
                assert_eq!(
                    actual,
                    expected,
                    "第 {} 条 fallback_intent 必须指向同一个意图",
                    index + 1
                );
            }
        }
    }

    #[test]
    fn reimporting_exported_json_does_not_collide() {
        let (_, originals) = builtin_software_dev();
        let json = to_json_array(&originals).unwrap();

        let first = to_records("first", &parse_intent_array(&json).unwrap(), NOW).unwrap();
        let second = to_records("second", &parse_intent_array(&json).unwrap(), NOW).unwrap();

        let first_ids: BTreeSet<&str> = first.iter().map(|record| record.id.as_str()).collect();
        let second_ids: BTreeSet<&str> = second.iter().map(|record| record.id.as_str()).collect();

        assert_eq!(first_ids.len(), originals.len(), "每次导入都必须生成新 id");
        assert!(
            first_ids.is_disjoint(&second_ids),
            "两次导入生成的 id 不得相交"
        );
        assert!(
            first_ids
                .is_disjoint(&originals.iter().map(|record| record.id.as_str()).collect()),
            "导入不得复用导出文件里的 id"
        );
    }
}
