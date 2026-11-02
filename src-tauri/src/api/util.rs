//! Utilities for processing LLM responses.

/// Extract JSON from an LLM response string.
///
/// LLMs often wrap JSON output in markdown code fences.  This function
/// strips those fences and returns the bare JSON, falling back to a
/// heuristic scan for `{ … }` when no fence is found.
///
/// # Handling order
///
/// 1. ```json … ```  — explicit JSON code fence
/// 2. ``` … ```      — generic code fence
/// 3. `{ … }`        — first `{` to last `}` in the text
/// 4. Full trimmed input (fallback)
pub fn extract_json(input: &str) -> String {
    let trimmed = input.trim();

    // Walk every fenced block once. A ```json fence wins; otherwise keep the
    // first non-empty generic fence. Empty fences are skipped so a stray
    // ```json\n``` cannot mask a later usable block.
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

        // Continue after the closing fence.
        let consumed = start + 3 + (after_open.len() - after_line.len()) + end + 3;
        rest = &rest[consumed..];
    }

    if let Some(block) = generic_fence {
        return block;
    }

    // Look for a bare JSON object — find first '{' and last '}'
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            if end > start {
                return trimmed[start..=end].to_string();
            }
        }
    }

    // Fallback: return the whole input cleaned up
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
        // Empty ```json block — should skip to bare-object heuristic
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
        // The info string must not leak into the extracted content.
        let input = "```rust\nfn main() {}\n```";
        assert_eq!(extract_json(input), "fn main() {}");
    }
}
