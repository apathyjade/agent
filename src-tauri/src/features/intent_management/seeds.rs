use chrono::Utc;

use crate::core::db::models::{IntentDomainRecord, IntentRecord};
use crate::core::db::repository::Database;
use crate::core::error::Result;

const DOMAIN_ID: &str = "builtin.software_dev";

/// `settings` key recording that the built-in domain has been seeded.
/// Seeding happens at most once per database, so a domain the user deleted
/// is never resurrected.
const SEEDED_SETTING: &str = "intent.seeded";

/// The built-in 「程序开发」 domain and its 12 intents, transcribed verbatim
/// from `docs/design/2026-11-04-intent-management-design.md` §6.3.
///
/// The descriptions are written as "how it differs from its neighbour" on
/// purpose: the classifier prompt is generated from them, so they must not be
/// softened into marketing copy.
pub fn builtin_software_dev() -> (IntentDomainRecord, Vec<IntentRecord>) {
    let now = Utc::now().to_rfc3339();

    let domain = IntentDomainRecord {
        id: DOMAIN_ID.to_string(),
        name: "程序开发".to_string(),
        description: "围绕代码的规划、实现、调试与审查。".to_string(),
        is_active: false,
        source: "builtin".to_string(),
        version: 1,
        created_at: now.clone(),
        updated_at: now.clone(),
    };

    // Fields shared by every seed intent (§6.3: scope is always global,
    // every intent ships enabled at version 1, and the optional tuning knobs
    // are left untouched).
    let base = IntentRecord {
        id: String::new(),
        domain_id: DOMAIN_ID.to_string(),
        name: String::new(),
        description: String::new(),
        examples: Vec::new(),
        workflow: String::new(),
        allowed_tools: Vec::new(),
        default_agent: String::new(),
        scope: "global".to_string(),
        enabled: true,
        version: 1,
        confidence_threshold: 0.0,
        fallback_intent: String::new(),
        path: String::new(),
        auto_escalate: false,
        system_prompt_appendix: String::new(),
        model_id: String::new(),
        max_iterations: None,
        sort_order: 0,
        created_at: now.clone(),
        updated_at: now.clone(),
    };

    let intents = vec![
        IntentRecord {
            id: "builtin.software_dev.plan".to_string(),
            name: "规划方案".to_string(),
            description: "要方案、拆解或步骤，且明确不是现在改代码。与 spec 的区别：产出是口头/对话形式的计划，不是可存档的文档。".to_string(),
            examples: vec![
                "先别动代码，帮我把这个需求拆一下".to_string(),
                "这个功能大概怎么落地".to_string(),
                "给我一个实施计划".to_string(),
                "分几步做比较好".to_string(),
                "先把思路理一下再动手".to_string(),
            ],
            workflow: "planning".to_string(),
            allowed_tools: vec!["file_system".to_string()],
            default_agent: "thinker".to_string(),
            path: "deep".to_string(),
            auto_escalate: true,
            confidence_threshold: 0.75,
            fallback_intent: "builtin.software_dev.spec".to_string(),
            sort_order: 0,
            ..base.clone()
        },
        IntentRecord {
            id: "builtin.software_dev.spec".to_string(),
            name: "编写规格".to_string(),
            description: "要一份可存档的需求或技术文档。与 plan 的区别：成果是文档，要落盘。".to_string(),
            examples: vec![
                "写一份登录模块的规格".to_string(),
                "把这个需求整理成设计文档".to_string(),
                "补一份技术方案".to_string(),
                "生成需求文档存到 docs 下".to_string(),
                "把刚才讨论的写成 spec".to_string(),
            ],
            workflow: "spec_generation".to_string(),
            allowed_tools: vec!["file_system".to_string()],
            default_agent: "thinker".to_string(),
            path: "deep".to_string(),
            auto_escalate: true,
            confidence_threshold: 0.75,
            fallback_intent: "builtin.software_dev.plan".to_string(),
            sort_order: 10,
            ..base.clone()
        },
        IntentRecord {
            id: "builtin.software_dev.implement".to_string(),
            name: "实现功能".to_string(),
            description: "明确要新增或修改代码行为。没有失败现象、也不是纯结构调整时选它。".to_string(),
            examples: vec![
                "加一个登出接口".to_string(),
                "把这段改成异步".to_string(),
                "实现一下这个需求".to_string(),
                "给设置页加个开关".to_string(),
                "按这个设计写代码".to_string(),
            ],
            workflow: "code_change".to_string(),
            allowed_tools: vec!["file_system".to_string(), "code_executor".to_string()],
            default_agent: "code_editor".to_string(),
            path: "deep".to_string(),
            auto_escalate: true,
            confidence_threshold: 0.7,
            fallback_intent: "builtin.software_dev.plan".to_string(),
            sort_order: 20,
            ..base.clone()
        },
        IntentRecord {
            id: "builtin.software_dev.debug".to_string(),
            name: "排查修复".to_string(),
            description: "有具体失败现象——报错、测试挂、行为不对——要先定位根因再修。与 implement 的区别：先有症状。".to_string(),
            examples: vec![
                "这个接口 500 了".to_string(),
                "为什么这条测试过不了".to_string(),
                "登录之后页面白屏".to_string(),
                "这里偶尔 panic".to_string(),
                "同样的输入两次结果不一样".to_string(),
            ],
            workflow: "bug_fix".to_string(),
            allowed_tools: vec!["file_system".to_string(), "code_executor".to_string()],
            default_agent: "code_explorer".to_string(),
            path: "deep".to_string(),
            auto_escalate: true,
            confidence_threshold: 0.75,
            fallback_intent: "builtin.software_dev.implement".to_string(),
            sort_order: 30,
            ..base.clone()
        },
        IntentRecord {
            id: "builtin.software_dev.test".to_string(),
            name: "测试相关".to_string(),
            description: "围绕测试本身：补用例、修用例、跑用例、看覆盖。改的是测试而不是被测代码时选它。".to_string(),
            examples: vec![
                "给这个函数补几个单测".to_string(),
                "跑一下测试看看".to_string(),
                "这个用例为什么挂了".to_string(),
                "补一下边界情况".to_string(),
                "覆盖率现在多少".to_string(),
            ],
            workflow: "testing".to_string(),
            allowed_tools: vec!["file_system".to_string(), "code_executor".to_string()],
            default_agent: "code_editor".to_string(),
            path: "deep".to_string(),
            auto_escalate: true,
            confidence_threshold: 0.7,
            fallback_intent: "builtin.software_dev.implement".to_string(),
            sort_order: 40,
            ..base.clone()
        },
        IntentRecord {
            id: "builtin.software_dev.review".to_string(),
            name: "审查改动".to_string(),
            description: "对已有改动或代码做质量判断，本身不改代码。要有对象——一个 diff、一个 PR、一段实现。".to_string(),
            examples: vec![
                "帮我 review 一下这次改动".to_string(),
                "这段代码有什么问题".to_string(),
                "这个 PR 能合吗".to_string(),
                "看看有没有隐患".to_string(),
                "这么写符合项目规范吗".to_string(),
            ],
            workflow: "review".to_string(),
            allowed_tools: vec!["file_system".to_string()],
            default_agent: "code_explorer".to_string(),
            path: "deep".to_string(),
            auto_escalate: true,
            confidence_threshold: 0.75,
            fallback_intent: "builtin.software_dev.explain".to_string(),
            sort_order: 50,
            ..base.clone()
        },
        IntentRecord {
            id: "builtin.software_dev.refactor".to_string(),
            name: "重构".to_string(),
            description: "改结构不改外部行为。明确说了「行为不变」「整理结构」「去重」时选它。".to_string(),
            examples: vec![
                "把这个模块拆一下".to_string(),
                "提取公共逻辑".to_string(),
                "这段太长了，整理一下".to_string(),
                "去掉重复代码".to_string(),
                "换个更清楚的组织方式".to_string(),
            ],
            workflow: "refactor".to_string(),
            allowed_tools: vec!["file_system".to_string(), "code_executor".to_string()],
            default_agent: "code_editor".to_string(),
            path: "deep".to_string(),
            auto_escalate: true,
            confidence_threshold: 0.7,
            fallback_intent: "builtin.software_dev.implement".to_string(),
            sort_order: 60,
            ..base.clone()
        },
        IntentRecord {
            id: "builtin.software_dev.explain".to_string(),
            name: "解释代码".to_string(),
            description: "要理解现有实现或报错的含义，不要改动。与 lookup 的区别：要的是「为什么」而不是「在哪」。".to_string(),
            examples: vec![
                "这段代码在干什么".to_string(),
                "为什么这里要加锁".to_string(),
                "这个报错什么意思".to_string(),
                "解释一下这个函数".to_string(),
                "这个设计为什么这么做".to_string(),
            ],
            workflow: "explain".to_string(),
            allowed_tools: vec!["file_system".to_string()],
            default_agent: "thinker".to_string(),
            path: "fast".to_string(),
            auto_escalate: false,
            confidence_threshold: 0.65,
            fallback_intent: "builtin.software_dev.lookup".to_string(),
            sort_order: 70,
            ..base.clone()
        },
        IntentRecord {
            id: "builtin.software_dev.lookup".to_string(),
            name: "查询项目".to_string(),
            description: "查事实：项目约定、文件位置、配置项、历史写法。要的是「在哪」「是什么」，不是「为什么」。".to_string(),
            examples: vec![
                "项目里怎么跑测试".to_string(),
                "AGENTS.md 怎么规定的".to_string(),
                "登录相关代码在哪".to_string(),
                "之前有没有类似实现".to_string(),
                "这个配置项在哪定义".to_string(),
            ],
            workflow: "knowledge_query".to_string(),
            allowed_tools: vec!["file_system".to_string(), "web_search".to_string()],
            default_agent: "code_explorer".to_string(),
            path: "fast".to_string(),
            auto_escalate: false,
            confidence_threshold: 0.65,
            fallback_intent: "builtin.software_dev.explain".to_string(),
            sort_order: 80,
            ..base.clone()
        },
        IntentRecord {
            id: "builtin.software_dev.execute".to_string(),
            name: "执行命令".to_string(),
            description: "直接要跑命令、构建、装依赖、起服务。诉求是「执行」而不是「改代码」。".to_string(),
            examples: vec![
                "跑一下 build".to_string(),
                "装一下依赖".to_string(),
                "启动开发服务器".to_string(),
                "执行这个脚本".to_string(),
                "跑一下 lint".to_string(),
            ],
            workflow: "tool_execute".to_string(),
            allowed_tools: vec!["code_executor".to_string(), "file_system".to_string()],
            default_agent: "shell".to_string(),
            path: "fast".to_string(),
            auto_escalate: false,
            confidence_threshold: 0.7,
            fallback_intent: "builtin.software_dev.implement".to_string(),
            sort_order: 90,
            ..base.clone()
        },
        IntentRecord {
            id: "builtin.software_dev.clarify".to_string(),
            name: "需要澄清".to_string(),
            description: "信息不足或指代不明，无法安全路由；正确反应是先反问，而不是猜着做。".to_string(),
            examples: vec![
                "帮我弄一下登录".to_string(),
                "改一下".to_string(),
                "看看这个".to_string(),
                "这个有问题".to_string(),
                "帮我处理一下".to_string(),
            ],
            workflow: "clarify".to_string(),
            allowed_tools: Vec::new(),
            default_agent: String::new(),
            path: "fast".to_string(),
            auto_escalate: false,
            confidence_threshold: 0.6,
            // Empty fallback: terminal, so it cannot loop back to itself.
            fallback_intent: String::new(),
            sort_order: 100,
            ..base.clone()
        },
        IntentRecord {
            id: "builtin.software_dev.chat".to_string(),
            name: "闲聊对话".to_string(),
            description: "寒暄、感谢、与项目无关的日常对话。不需要工具，也不应触发任何工程动作。".to_string(),
            examples: vec![
                "你好".to_string(),
                "谢谢".to_string(),
                "今天有点累".to_string(),
                "在吗".to_string(),
                "随便聊聊".to_string(),
            ],
            workflow: "chat".to_string(),
            allowed_tools: Vec::new(),
            default_agent: String::new(),
            path: "fast".to_string(),
            auto_escalate: false,
            confidence_threshold: 0.6,
            // Empty fallback: terminal, so it cannot loop back to itself.
            fallback_intent: String::new(),
            sort_order: 110,
            ..base.clone()
        },
    ];

    (domain, intents)
}

/// Seeds the built-in domain at most once per database.
///
/// The "already seeded" marker is the `intent.seeded` row in the `settings`
/// table, not the domain's existence: users may delete a built-in domain
/// (§7), and that must stay deleted across restarts.
///
/// The domain, its 12 intents, the activation and the marker all land in one
/// transaction ([`Database::seed_intent_domain_atomic`]), so an interrupted or
/// failed seed leaves nothing behind and the next launch simply retries.
pub fn seed_builtin_intents(db: &Database) -> Result<()> {
    if db.get_setting(SEEDED_SETTING)?.is_some() {
        return Ok(());
    }

    let (domain, intents) = builtin_software_dev();
    db.seed_intent_domain_atomic(&domain, &intents, SEEDED_SETTING, "1")?;

    log::info!("Seeded built-in intent domain {DOMAIN_ID} with {} intents", intents.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    const DOMAIN: &str = "builtin.software_dev";

    const EXPECTED_IDS: [&str; 12] = [
        "builtin.software_dev.plan",
        "builtin.software_dev.spec",
        "builtin.software_dev.implement",
        "builtin.software_dev.debug",
        "builtin.software_dev.test",
        "builtin.software_dev.review",
        "builtin.software_dev.refactor",
        "builtin.software_dev.explain",
        "builtin.software_dev.lookup",
        "builtin.software_dev.execute",
        "builtin.software_dev.clarify",
        "builtin.software_dev.chat",
    ];

    const KNOWN_TOOLS: [&str; 5] = [
        "file_system",
        "code_executor",
        "web_search",
        "calculator",
        "run_workflow",
    ];

    const KNOWN_AGENTS: [&str; 8] = [
        "",
        "thinker",
        "code_explorer",
        "code_editor",
        "shell",
        "web",
        "memory",
        "mcp_bridge",
    ];

    const EXPECTED_NAMES: [&str; 12] = [
        "规划方案",
        "编写规格",
        "实现功能",
        "排查修复",
        "测试相关",
        "审查改动",
        "重构",
        "解释代码",
        "查询项目",
        "执行命令",
        "需要澄清",
        "闲聊对话",
    ];

    const EXPECTED_DESCRIPTIONS: [&str; 12] = [
        "要方案、拆解或步骤，且明确不是现在改代码。与 spec 的区别：产出是口头/对话形式的计划，不是可存档的文档。",
        "要一份可存档的需求或技术文档。与 plan 的区别：成果是文档，要落盘。",
        "明确要新增或修改代码行为。没有失败现象、也不是纯结构调整时选它。",
        "有具体失败现象——报错、测试挂、行为不对——要先定位根因再修。与 implement 的区别：先有症状。",
        "围绕测试本身：补用例、修用例、跑用例、看覆盖。改的是测试而不是被测代码时选它。",
        "对已有改动或代码做质量判断，本身不改代码。要有对象——一个 diff、一个 PR、一段实现。",
        "改结构不改外部行为。明确说了「行为不变」「整理结构」「去重」时选它。",
        "要理解现有实现或报错的含义，不要改动。与 lookup 的区别：要的是「为什么」而不是「在哪」。",
        "查事实：项目约定、文件位置、配置项、历史写法。要的是「在哪」「是什么」，不是「为什么」。",
        "直接要跑命令、构建、装依赖、起服务。诉求是「执行」而不是「改代码」。",
        "信息不足或指代不明，无法安全路由；正确反应是先反问，而不是猜着做。",
        "寒暄、感谢、与项目无关的日常对话。不需要工具，也不应触发任何工程动作。",
    ];

    const EXPECTED_WORKFLOWS: [&str; 12] = [
        "planning",
        "spec_generation",
        "code_change",
        "bug_fix",
        "testing",
        "review",
        "refactor",
        "explain",
        "knowledge_query",
        "tool_execute",
        "clarify",
        "chat",
    ];

    /// (id suffix, path, auto_escalate, confidence_threshold, fallback suffix)
    /// straight from design §6.3.
    const EXPECTED_ROUTING: [(&str, &str, bool, f64, &str); 12] = [
        ("plan", "deep", true, 0.75, "spec"),
        ("spec", "deep", true, 0.75, "plan"),
        ("implement", "deep", true, 0.7, "plan"),
        ("debug", "deep", true, 0.75, "implement"),
        ("test", "deep", true, 0.7, "implement"),
        ("review", "deep", true, 0.75, "explain"),
        ("refactor", "deep", true, 0.7, "implement"),
        ("explain", "fast", false, 0.65, "lookup"),
        ("lookup", "fast", false, 0.65, "explain"),
        ("execute", "fast", false, 0.7, "implement"),
        ("clarify", "fast", false, 0.6, ""),
        ("chat", "fast", false, 0.6, ""),
    ];

    /// Minimal enabled intent record with a caller-chosen id/domain.
    fn mk_intent(id: &str, domain_id: &str) -> IntentRecord {
        let ts = Utc::now().to_rfc3339();
        IntentRecord {
            id: id.to_string(),
            domain_id: domain_id.to_string(),
            name: id.to_string(),
            description: String::new(),
            examples: Vec::new(),
            workflow: "chat".to_string(),
            allowed_tools: Vec::new(),
            default_agent: String::new(),
            scope: "global".to_string(),
            enabled: true,
            version: 1,
            confidence_threshold: 0.5,
            fallback_intent: String::new(),
            path: "fast".to_string(),
            auto_escalate: false,
            system_prompt_appendix: String::new(),
            model_id: String::new(),
            max_iterations: None,
            sort_order: 0,
            created_at: ts.clone(),
            updated_at: ts,
        }
    }

    /// Creates and activates a minimal user domain, so the built-in one can be
    /// deleted (deleting the active domain is rejected by the repo guard).
    fn create_user_domain(db: &Database) {
        let ts = Utc::now().to_rfc3339();
        db.insert_intent_domain(&IntentDomainRecord {
            id: "user-domain".to_string(),
            name: "我的领域".to_string(),
            description: String::new(),
            is_active: false,
            source: "user".to_string(),
            version: 1,
            created_at: ts.clone(),
            updated_at: ts,
        })
        .unwrap();
        db.insert_intent(&mk_intent("user-intent", "user-domain"))
            .unwrap();
        db.set_active_intent_domain("user-domain").unwrap();
    }

    #[test]
    fn seeding_is_idempotent() {
        let db = Database::new_test().unwrap();
        seed_builtin_intents(&db).unwrap();
        seed_builtin_intents(&db).unwrap();
        assert_eq!(db.list_intent_domains().unwrap().len(), 1);
        assert_eq!(db.count_intents(DOMAIN), 12);
    }

    /// The atomic seed must roll back writes that already landed, not merely
    /// return `Err`. Here the first intent insert hits a primary-key conflict
    /// from an intent parked in another domain, i.e. the failure happens after
    /// the built-in domain row was inserted — exactly the round-1 shape where
    /// a partially seeded domain used to survive.
    #[test]
    fn atomic_seed_rolls_back_a_partially_applied_attempt() {
        let db = Database::new_test().unwrap();
        create_user_domain(&db);
        // Foreign keys are OFF, so this intent may live in the user domain
        // while squatting on an id the built-in seed needs. Primary keys are
        // global, so the seed's first intent insert must conflict.
        db.insert_intent(&mk_intent("builtin.software_dev.plan", "user-domain"))
            .unwrap();

        let (domain, intents) = builtin_software_dev();
        let err = db.seed_intent_domain_atomic(&domain, &intents, SEEDED_SETTING, "1");

        assert!(err.is_err(), "意图主键冲突必须让整个播种失败");
        assert!(
            db.get_intent_domain(DOMAIN).unwrap().is_none(),
            "已写入的领域行必须被回滚"
        );
        assert_eq!(db.count_intents(DOMAIN), 0, "回滚后不得留下任何内置意图");
        assert_eq!(
            db.get_active_intent_domain().unwrap().map(|d| d.id),
            Some("user-domain".to_string()),
            "用户领域必须仍然是活跃领域"
        );
        assert_eq!(db.count_intents("user-domain"), 2, "用户领域的数据不得被动到");
        assert!(
            db.get_setting(SEEDED_SETTING).unwrap().is_none(),
            "失败的事务不得写入标记"
        );
    }

    /// A failed seed must not write the marker, otherwise a legitimate retry
    /// would be suppressed forever. (This failure happens on the first write,
    /// so it does not exercise rollback — see the test above for that.)
    #[test]
    fn failed_atomic_seed_leaves_no_marker() {
        let db = Database::new_test().unwrap();
        // A pre-existing row with the same id makes the insert conflict, which
        // is the same shape as a leftover from an interrupted seed.
        db.insert_intent_domain(&IntentDomainRecord {
            id: DOMAIN.to_string(),
            name: "程序开发".to_string(),
            description: String::new(),
            is_active: false,
            source: "user".to_string(),
            version: 1,
            created_at: "2026-11-04T00:00:00Z".to_string(),
            updated_at: "2026-11-04T00:00:00Z".to_string(),
        })
        .unwrap();

        let (domain, intents) = builtin_software_dev();
        assert!(db
            .seed_intent_domain_atomic(&domain, &intents, SEEDED_SETTING, "1")
            .is_err());

        assert!(
            db.get_setting(SEEDED_SETTING).unwrap().is_none(),
            "失败的播种不得写入标记"
        );
        assert_eq!(db.count_intents(DOMAIN), 0, "失败的事务不得留下意图");
        assert!(
            db.get_active_intent_domain().unwrap().is_none(),
            "失败的事务不得激活领域"
        );

        // With the conflicting row gone, the retry is not blocked.
        db.delete_intent_domain(DOMAIN).unwrap();
        seed_builtin_intents(&db).unwrap();
        assert_eq!(db.count_intents(DOMAIN), 12);
    }

    /// Deletes are permanent: once the marker is set, a deleted built-in
    /// domain must not come back on the next launch, and it must not steal
    /// activation back from the user's own domain.
    #[test]
    fn deleted_builtin_domain_is_not_resurrected() {
        let db = Database::new_test().unwrap();
        seed_builtin_intents(&db).unwrap();
        create_user_domain(&db);
        db.delete_intent_domain(DOMAIN).unwrap();

        seed_builtin_intents(&db).unwrap();

        let domains = db.list_intent_domains().unwrap();
        assert_eq!(domains.len(), 1, "被删除的内置领域不得复活");
        assert_eq!(domains[0].id, "user-domain");
        assert_eq!(db.count_intents(DOMAIN), 0);
        assert_eq!(
            db.get_active_intent_domain().unwrap().map(|d| d.id),
            Some("user-domain".to_string()),
            "再次播种不得抢走用户的活跃领域"
        );
        assert_eq!(
            db.get_setting(SEEDED_SETTING).unwrap().as_deref(),
            Some("1")
        );
    }

    #[test]
    fn seeding_preserves_user_edits() {
        let db = Database::new_test().unwrap();
        seed_builtin_intents(&db).unwrap();

        let mut intent = db.get_intent("builtin.software_dev.plan").unwrap().unwrap();
        intent.name = "我的规划".to_string();
        db.update_intent(&intent).unwrap();

        seed_builtin_intents(&db).unwrap();

        let stored = db.get_intent("builtin.software_dev.plan").unwrap().unwrap();
        assert_eq!(stored.name, "我的规划", "再次播种不得覆盖用户改动");
        assert_eq!(stored.version, 2, "再次播种不得重置版本");
        assert_eq!(db.count_intents(DOMAIN), 12);
    }

    #[test]
    fn seeded_domain_is_active() {
        let db = Database::new_test().unwrap();
        seed_builtin_intents(&db).unwrap();

        let active = db.get_active_intent_domain().unwrap().unwrap();
        assert_eq!(active.id, DOMAIN);
        assert_eq!(active.source, "builtin");
        assert_eq!(active.name, "程序开发");
        assert_eq!(active.version, 1);
        assert!(!active.description.trim().is_empty());
    }

    #[test]
    fn seed_matches_design_document() {
        let db = Database::new_test().unwrap();
        seed_builtin_intents(&db).unwrap();

        let intents = db.list_intents(DOMAIN).unwrap();
        let ids: Vec<&str> = intents.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(
            ids,
            EXPECTED_IDS.to_vec(),
            "意图顺序与数量必须与设计文档 §6.3 一致"
        );

        let expected: BTreeSet<&str> = EXPECTED_IDS.iter().copied().collect();
        let actual: BTreeSet<&str> = ids.iter().copied().collect();
        assert_eq!(actual, expected);

        assert_eq!(
            intents
                .iter()
                .map(|i| i.name.as_str())
                .collect::<Vec<_>>(),
            EXPECTED_NAMES.to_vec(),
            "意图名字必须按 §6.3 顺序逐一对应"
        );
        assert_eq!(
            intents
                .iter()
                .map(|i| i.workflow.as_str())
                .collect::<Vec<_>>(),
            EXPECTED_WORKFLOWS.to_vec(),
            "workflow 必须按 §6.3 顺序逐一对应"
        );
        assert_eq!(
            intents
                .iter()
                .map(|i| i.description.as_str())
                .collect::<Vec<_>>(),
            EXPECTED_DESCRIPTIONS.to_vec(),
            "描述是分类提示词的来源，必须按 §6.3 顺序逐字对应"
        );

        for intent in &intents {
            assert!(!intent.name.trim().is_empty(), "{} 缺少名字", intent.id);
            assert!(
                !intent.description.trim().is_empty(),
                "{} 缺少描述",
                intent.id
            );
            assert_eq!(intent.examples.len(), 5, "{} 必须有 5 个示例", intent.id);
            assert_eq!(intent.domain_id, DOMAIN, "{} 领域归属错误", intent.id);
            assert_eq!(intent.scope, "global", "{} scope 错误", intent.id);
            assert!(intent.enabled, "{} 必须启用", intent.id);
            assert_eq!(intent.version, 1, "{} 初始版本应为 1", intent.id);

            for tool in &intent.allowed_tools {
                assert!(
                    KNOWN_TOOLS.contains(&tool.as_str()),
                    "{} 引用了不存在的工具 {}",
                    intent.id,
                    tool
                );
            }
            assert!(
                KNOWN_AGENTS.contains(&intent.default_agent.as_str()),
                "{} 引用了不存在的 worker {}",
                intent.id,
                intent.default_agent
            );
        }

        // Seed order must be materialized as sort_order = index * 10.
        assert_eq!(
            intents.iter().map(|i| i.sort_order).collect::<Vec<_>>(),
            (0..12).map(|i| i * 10).collect::<Vec<_>>()
        );

        // Routing fields must match §6.3 exactly.
        for (intent, (slug, path, auto_escalate, threshold, fallback)) in
            intents.iter().zip(EXPECTED_ROUTING.iter())
        {
            assert_eq!(intent.id, format!("{DOMAIN}.{slug}"), "id 顺序错位");
            let expected_fallback = if fallback.is_empty() {
                String::new()
            } else {
                format!("{DOMAIN}.{fallback}")
            };
            assert_eq!(
                (
                    intent.path.as_str(),
                    intent.auto_escalate,
                    intent.confidence_threshold,
                    intent.fallback_intent.as_str(),
                ),
                (*path, *auto_escalate, *threshold, expected_fallback.as_str()),
                "{} 的路由字段与 §6.3 不一致",
                intent.id
            );
        }
    }
}
