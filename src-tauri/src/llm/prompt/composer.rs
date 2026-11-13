//! 提示词片段组装器。
//!
//! 输入片段列表与预算，输出最终文本以及裁剪轨迹（丢弃了哪些片段、是否截断、
//! 是否超预算），供上层写进 trace 或调试输出。
//!
//! 裁剪规则：
//! 1. 同 key 去重，保留优先级更高的一份。
//! 2. 按优先级升序输出；同优先级保持插入顺序。
//! 3. 超预算时从优先级最高的可裁片段开始丢；只剩一个可裁片段时改为截断。
//! 4. `required` 片段永不丢弃、永不截断；仅剩它们仍超预算时标记 `over_budget`。

/// 默认预算上限（token）。
pub const DEFAULT_MAX_TOKENS: usize = 8000;
/// 默认字符/token 估算系数（中英混合保守取值，不引入 tokenizer 依赖）。
pub const DEFAULT_CHARS_PER_TOKEN: f64 = 2.5;
/// 截断时保留的头部比例，其余留给尾部。
const TRUNCATE_HEAD_RATIO: f64 = 0.7;
/// 截断标记。
const TRUNCATED_MARKER: &str = "[...truncated...]";

/// 一个待组装的提示词片段。
#[derive(Debug, Clone)]
pub struct Fragment {
    /// 去重键，例如 `session`、`intent.<id>`。
    pub key: String,
    /// 数字越大越先被裁掉；同时决定输出顺序（升序）。
    pub priority: i32,
    /// 片段正文。
    pub text: String,
    /// 是否不可裁剪（安全/身份类内容）。
    pub required: bool,
    /// 与前一片段之间的分隔符；首片段忽略。
    pub separator: &'static str,
}

impl Fragment {
    /// 用默认分隔符（空行）构造片段。
    pub fn new(key: impl Into<String>, priority: i32, text: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            priority,
            text: text.into(),
            required: false,
            separator: "\n\n",
        }
    }

    /// 标记为不可裁剪。
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// 指定与前一片段的分隔符。
    pub fn with_separator(mut self, separator: &'static str) -> Self {
        self.separator = separator;
        self
    }
}

/// 组装预算。
#[derive(Debug, Clone, Copy)]
pub struct PromptBudget {
    /// 上限（token）。
    pub max_tokens: usize,
    /// 字符/token 估算系数。
    pub chars_per_token: f64,
}

impl Default for PromptBudget {
    fn default() -> Self {
        Self {
            max_tokens: DEFAULT_MAX_TOKENS,
            chars_per_token: DEFAULT_CHARS_PER_TOKEN,
        }
    }
}

impl PromptBudget {
    /// 以 token 上限构造，系数取默认值。
    pub fn new(max_tokens: usize) -> Self {
        Self {
            max_tokens,
            chars_per_token: DEFAULT_CHARS_PER_TOKEN,
        }
    }

    /// 以字符上限构造；用于沿用既有的字符预算口径。
    pub fn from_chars(max_chars: usize) -> Self {
        let chars_per_token = DEFAULT_CHARS_PER_TOKEN;
        Self {
            max_tokens: ((max_chars as f64) / chars_per_token).ceil() as usize,
            chars_per_token,
        }
    }

    /// 预算折算成字符数。
    fn max_chars(&self) -> usize {
        ((self.max_tokens as f64) * self.chars_per_token).floor() as usize
    }

    /// 字符数折算成 token 数（向上取整）。
    fn tokens(&self, chars: usize) -> usize {
        ((chars as f64) / self.chars_per_token).ceil() as usize
    }
}

/// 字符数折算成 token 数（向上取整），沿用默认系数。
pub fn estimate_tokens(text: &str) -> usize {
    let chars = text.chars().count();
    ((chars as f64) / DEFAULT_CHARS_PER_TOKEN).ceil() as usize
}

/// 组装结果。
#[derive(Debug, Clone, Default)]
pub struct ComposedPrompt {
    /// 拼好的最终文本。
    pub text: String,
    /// 实际占用（token 估算）。
    pub used_tokens: usize,
    /// 被整段丢弃的片段 key，按丢弃顺序。
    pub dropped: Vec<String>,
    /// 是否有片段被截断。
    pub truncated: bool,
    /// 仅剩不可裁片段仍超预算时为 true（安全优先，不静默丢弃）。
    pub over_budget: bool,
}

/// 按预算组装片段。
pub fn compose(fragments: Vec<Fragment>, budget: &PromptBudget) -> ComposedPrompt {
    let mut kept: Vec<Fragment> = Vec::new();
    for fragment in fragments
        .into_iter()
        .filter(|fragment| !fragment.text.trim().is_empty())
    {
        // 同 key 去重：优先级更高者胜出；相同时保留先出现的一份。
        match kept.iter_mut().find(|item| item.key == fragment.key) {
            Some(existing) => {
                if fragment.priority > existing.priority {
                    *existing = fragment;
                }
            }
            None => kept.push(fragment),
        }
    }
    kept.sort_by_key(|fragment| fragment.priority);

    let max_chars = budget.max_chars();
    let mut dropped: Vec<String> = Vec::new();
    let mut truncated = false;

    while rendered_chars(&kept) > max_chars {
        let droppable: Vec<usize> = kept
            .iter()
            .enumerate()
            .filter(|(_, fragment)| !fragment.required)
            .map(|(index, _)| index)
            .collect();

        if droppable.is_empty() {
            break;
        }

        if droppable.len() > 1 {
            let index = *droppable.last().expect("droppable 非空");
            dropped.push(kept.remove(index).key);
            continue;
        }

        // 只剩一个可裁片段：截断它而不是整段丢弃，尽量保住内容。
        let index = droppable[0];
        let others: Vec<&Fragment> = kept
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != index)
            .map(|(_, fragment)| fragment)
            .collect();
        let separator = if others.is_empty() {
            0
        } else {
            kept[index].separator.chars().count()
        };
        let allowed = max_chars.saturating_sub(rendered_chars_refs(&others) + separator);
        let (text, did_truncate) = truncate_middle(&kept[index].text, allowed);
        if text.is_empty() {
            dropped.push(kept.remove(index).key);
        } else {
            kept[index].text = text;
            truncated |= did_truncate;
        }
        break;
    }

    let text = render(&kept);
    let used_chars = text.chars().count();
    ComposedPrompt {
        text,
        used_tokens: budget.tokens(used_chars),
        dropped,
        truncated,
        over_budget: used_chars > max_chars,
    }
}

/// 按分隔符渲染片段。
fn render(fragments: &[Fragment]) -> String {
    let mut text = String::new();
    for (index, fragment) in fragments.iter().enumerate() {
        if index > 0 {
            text.push_str(fragment.separator);
        }
        text.push_str(&fragment.text);
    }
    text
}

/// 计算渲染后的字符数（含分隔符）。
fn rendered_chars(fragments: &[Fragment]) -> usize {
    fragments
        .iter()
        .enumerate()
        .map(|(index, fragment)| {
            let separator = if index == 0 {
                0
            } else {
                fragment.separator.chars().count()
            };
            fragment.text.chars().count() + separator
        })
        .sum()
}

/// [`rendered_chars`] 的引用版本。
fn rendered_chars_refs(fragments: &[&Fragment]) -> usize {
    fragments
        .iter()
        .enumerate()
        .map(|(index, fragment)| {
            let separator = if index == 0 {
                0
            } else {
                fragment.separator.chars().count()
            };
            fragment.text.chars().count() + separator
        })
        .sum()
}

/// 保留头 70%、尾 30%，中间插入截断标记。
fn truncate_middle(text: &str, allowed: usize) -> (String, bool) {
    let total = text.chars().count();
    if total <= allowed {
        return (text.to_string(), false);
    }

    let marker_len = TRUNCATED_MARKER.chars().count();
    if allowed <= marker_len {
        // 预算连标记都放不下：整段留空，由调用方记为丢弃。
        return (String::new(), true);
    }

    let keep = allowed - marker_len;
    let head = ((keep as f64) * TRUNCATE_HEAD_RATIO).round() as usize;
    let tail = keep - head;
    let chars: Vec<char> = text.chars().collect();

    let mut out = String::with_capacity(allowed);
    out.extend(&chars[..head]);
    out.push_str(TRUNCATED_MARKER);
    out.extend(&chars[total - tail..]);
    (out, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fragment(key: &str, priority: i32, text: &str) -> Fragment {
        Fragment::new(key, priority, text)
    }

    #[test]
    fn compose_dedupes_same_key_keeping_highest_priority() {
        let out = compose(
            vec![
                fragment("session", 10, "old"),
                fragment("session", 20, "new"),
            ],
            &PromptBudget::new(1000),
        );

        assert_eq!(out.text, "new");
    }

    #[test]
    fn compose_keeps_everything_within_budget() {
        let out = compose(
            vec![
                fragment("session", 10, "session"),
                fragment("persona", 20, "persona"),
                fragment("intent", 30, "intent"),
            ],
            &PromptBudget::new(1000),
        );

        assert_eq!(out.text, "session\n\npersona\n\nintent");
        assert!(out.dropped.is_empty());
        assert!(!out.truncated);
        assert!(!out.over_budget);
    }

    #[test]
    fn compose_drops_lowest_priority_fragments_first() {
        // 预算 100 token ≈ 250 字符；三段各 120 字符。
        let out = compose(
            vec![
                fragment("session", 10, &"s".repeat(120)),
                fragment("intent", 30, &"i".repeat(120)),
                fragment("memory", 40, &"m".repeat(120)),
            ],
            &PromptBudget::new(100),
        );

        assert_eq!(out.dropped, vec!["memory".to_string()]);
        assert!(!out.text.contains("mmm"));
        assert!(out.text.contains("sss"));
        assert!(out.text.contains("iii"));
        assert!(!out.truncated);
    }

    #[test]
    fn compose_never_drops_required_fragment() {
        let out = compose(
            vec![
                Fragment::new("core", 5, "SAFETY").required(),
                fragment("memory", 40, &"m".repeat(500)),
            ],
            &PromptBudget::new(1),
        );

        assert!(out.text.contains("SAFETY"));
        assert_eq!(out.dropped, vec!["memory".to_string()]);
        assert!(out.over_budget);
    }

    #[test]
    fn compose_truncates_the_last_droppable_fragment() {
        // 预算 40 token = 100 字符；session 片段远超预算，且是唯一可裁项。
        let out = compose(
            vec![
                Fragment::new("core", 5, "CORE").required(),
                fragment("session", 10, &"x".repeat(400)),
            ],
            &PromptBudget::new(40),
        );

        assert!(out.truncated);
        assert!(out.dropped.is_empty());
        assert!(out.text.starts_with("CORE"));
        assert!(out.text.contains(TRUNCATED_MARKER));
        assert!(out.used_tokens <= 40);
    }

    #[test]
    fn compose_reports_over_budget_when_only_required_exceeds() {
        let out = compose(
            vec![Fragment::new("core", 5, "SAFETY_BLOCK").required()],
            &PromptBudget::new(1),
        );

        assert_eq!(out.text, "SAFETY_BLOCK");
        assert!(out.over_budget);
        assert!(out.dropped.is_empty());
        assert!(!out.truncated);
    }
}
