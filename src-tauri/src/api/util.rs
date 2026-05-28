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

    // Try to find a ```json ... ``` block
    if let Some(start) = trimmed.find("```json") {
        let after_fence = &trimmed[start + 7..];
        if let Some(end) = after_fence.find("```") {
            let json_block = after_fence[..end].trim();
            if !json_block.is_empty() {
                return json_block.to_string();
            }
        }
    }

    // Try to find a ``` ... ``` block (generic code fence)
    if let Some(start) = trimmed.find("```") {
        let after_fence = &trimmed[start + 3..];
        if let Some(end) = after_fence.find("```") {
            let json_block = after_fence[..end].trim();
            if !json_block.is_empty() {
                return json_block.to_string();
            }
        }
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
}
