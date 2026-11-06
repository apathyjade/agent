//! 通用工具函数。
//!
//! 这里放与具体业务无关、但被多层共享的小工具。
//! 约束：`core` 层不得引用 `llm|agent|features|app|runtime`。

/// 按字符数截断字符串，保证不会切断 UTF-8 多字节字符。
///
/// 与 `&s[..s.len().min(n)]` 的区别：后者按**字节**切片，当切口落在多字节字符
/// （例如中文，一个字符 3 字节）中间时会 panic。本应用的大模型输出经常包含中文，
/// 因此所有「截取预览」类场景都必须走这个函数。
///
/// # 参数
///
/// * `s` —— 待截断的字符串
/// * `max_chars` —— 最多保留的字符数（不是字节数）
///
/// # 示例
///
/// ```
/// use agent_lib::core::util::truncate_chars;
///
/// // 中文按字符计数，不会 panic
/// assert_eq!(truncate_chars("你好世界", 2), "你好");
/// // 字符串比上限短时原样返回
/// assert_eq!(truncate_chars("ab", 10), "ab");
/// ```
pub fn truncate_chars(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => &s[..byte_idx],
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_by_characters_not_bytes() {
        // "你好" 占 6 字节，按字节切会切出半个字符
        assert_eq!(truncate_chars("你好世界", 2), "你好");
        assert_eq!(truncate_chars("你好世界", 2).len(), 6);
    }

    #[test]
    fn returns_whole_string_when_shorter_than_limit() {
        assert_eq!(truncate_chars("abc", 10), "abc");
        assert_eq!(truncate_chars("", 5), "");
    }

    #[test]
    fn handles_limit_at_exact_boundary() {
        assert_eq!(truncate_chars("abc", 3), "abc");
        assert_eq!(truncate_chars("abc", 2), "ab");
    }

    #[test]
    fn zero_limit_yields_empty_string() {
        assert_eq!(truncate_chars("你好", 0), "");
    }

    #[test]
    fn mixed_ascii_and_chinese() {
        assert_eq!(truncate_chars("a你b好c", 3), "a你b");
    }
}
