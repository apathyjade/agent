//! 处理 LLM 响应的工具函数。

/// 从 LLM 响应字符串中提取 JSON。
///
/// LLM 经常把 JSON 包在 markdown 代码围栏里。本函数剥掉围栏返回裸 JSON；没有
/// 围栏时退化为 `{ … }` 的启发式扫描。
///
/// # 处理顺序
///
/// 1. ```json … ```  — 显式 JSON 代码围栏
/// 2. ``` … ```      — 通用代码围栏
/// 3. `{ … }`        — 文本中第一个 `{` 到最后一个 `}`
/// 4. 整段输入（兜底）
pub fn extract_json(input: &str) -> String {
    let trimmed = input.trim();

    // 只遍历每个围栏块一次。```json 优先；否则保留第一个非空的通用围栏。
    // 空围栏会被跳过，避免一个孤立的 ```json\n``` 掩盖后面可用的块。
    let mut generic_fence: Option<String> = None;
    let mut rest = trimmed;
    while let Some(start) = rest.find("```") {
        let after_open = &rest[start + 3..];
        let (info, after_line) = match after_open.split_once('\n') {
            Some((info, after)) => (info.trim(), after),
            None => (after_open.trim(), ""),
        };
        let Some(end) = after_line.find("```") else {
            break;
        };

        let block = after_line[..end].trim();
        if !block.is_empty() {
            if info.eq_ignore_ascii_case("json") {
                return block.to_string();
            }
            if generic_fence.is_none() {
                generic_fence = Some(block.to_string());
            }
        }

        // 从闭合围栏之后继续扫描。
        let consumed = start + 3 + (after_open.len() - after_line.len()) + end + 3;
        rest = &rest[consumed..];
    }

    if let Some(block) = generic_fence {
        return block;
    }

    // 寻找裸 JSON 对象 —— 第一个 '{' 与最后一个 '}'
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            if end > start {
                return trimmed[start..=end].to_string();
            }
        }
    }

    // 兜底：返回清理后的整段输入
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_code_block() {
        let input = "Here is the plan:\n```json\n{\"steps\": []}\n```\nEnd.";
        assert_eq!(extract_json(input), "{\"steps\": []}");
    }

    #[test]
    fn test_plain_json() {
        let input = "{\"steps\": []}";
        assert_eq!(extract_json(input), "{\"steps\": []}");
    }

    #[test]
    fn test_generic_fence() {
        let input = "Some text\n```\n{\"decision\": \"go\"}\n```\nmore text";
        assert_eq!(extract_json(input), "{\"decision\": \"go\"}");
    }

    #[test]
    fn test_bare_object() {
        let input = "Some text {\"key\": \"value\"} more text";
        assert_eq!(extract_json(input), "{\"key\": \"value\"}");
    }

    #[test]
    fn test_trailing_text_after_fence() {
        let input = "```json\n{\"a\": 1}\n```\ntrailing";
        assert_eq!(extract_json(input), "{\"a\": 1}");
    }

    #[test]
    fn test_empty_fence_skips_to_next_heuristic() {
        // 空的 ```json 块 —— 应跳到裸对象启发式
        let input = "```json\n```\n{\"fallback\": true}";
        let result = extract_json(input);
        assert!(result.contains("fallback"));
    }

    #[test]
    fn test_empty_fence_then_later_json_fence() {
        let input = "```json\n```\ntext\n```json\n{\"ok\": true}\n```";
        assert_eq!(extract_json(input), "{\"ok\": true}");
    }

    #[test]
    fn test_generic_fence_with_language_tag() {
        // info 字符串不得泄漏进提取结果。
        let input = "```rust\nfn main() {}\n```";
        assert_eq!(extract_json(input), "fn main() {}");
    }
}
