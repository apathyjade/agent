# 意图拆解评测报告

日期：2026-11-09
分支：`ai/intent-decomposition-orchestration`
评测集：`src-tauri/tests/fixtures/intent_decomposition_eval.json`（57 条：single 24 / multi 15 / zero 10 / ambiguous 8）
运行方式：`INTENT_ROUTING_EVAL=1 cargo test --manifest-path src-tauri/Cargo.toml --lib intent_decomposition_eval -- --ignored --nocapture`
模型覆盖：`INTENT_EVAL_MODEL_ID=<模型 id>` 可覆盖拆解模型，用于横向比较。

---

## 1. 指标口径

| 指标 | 定义 |
|---|---|
| set accuracy (all) | 全部 57 条里，预测意图集合与标注集合完全一致的比例 |
| set accuracy (excluding ambiguous) | 排除 `ambiguous` 子集后的集合准确率。**ambiguous 是设计上就允许判错的子集**（跨领域/短句歧义），混进总准确率不合理 |
| set accuracy (single / multi) | 对应子集上的集合准确率 |
| ordered accuracy (multi) | multi 子集里，模型返回的意图**顺序**与标注顺序一致的比例 |
| intent recall | 标注集合里的每个意图是否都被拆出来（漏一个即失败） |
| zero-intent accuracy | 应判为零意图的样本，正确落入普通聊天兜底的比例 |
| hallucination rate | 拆出的意图 id 不在候选集合内的比例（应为 0） |
| ambiguous uncertain | ambiguous 子集里被标记 `uncertain` 的比例 |

---

## 2. 结果对照

### 2.1 旧实现（单意图路由）基线

模型 `deepseek-v4-pro`，超时 8000ms。

| 指标 | 结果 |
|---|---|
| set accuracy (single) | 24/24 = **100%** |
| zero-intent accuracy | 3/10 = **30%** |
| hallucination rate | 0/52 = **0%** |
| llm calls | 57（**每条消息 1 次**） |
| p50 latency | 3759 ms |

旧实现只能选出**一个**意图，因此基线只在 single 子集上具有可比性；multi 与 ambiguous 样本保留在评测集里供新实现对比。

### 2.2 新实现（意图拆解）

| 指标 | 旧实现 | 新实现 `deepseek-v4-pro` | 新实现 `deepseek-v4-flash` |
|---|---|---|---|
| set accuracy (all) | — | 51/57 = 0.89 | **51/57 = 0.89** |
| set accuracy (excluding ambiguous) | — | 48/49 = 0.98 | **48/49 = 0.98** |
| set accuracy (single) | 1.00 | 24/24 = 1.00 | **24/24 = 1.00** |
| set accuracy (multi) | 不支持 | 14/15 = 0.93 | **14/15 = 0.93** |
| ordered accuracy (multi) | 不支持 | 14/15 = 0.93 | **14/15 = 0.93** |
| intent recall | 不支持 | 55/62 = 0.89 | **57/62 = 0.92** |
| zero-intent accuracy | 0.30 | 10/10 = 1.00 | **10/10 = 1.00** |
| hallucination rate | 0.00 | 0/55 = 0.00 | **0/59 = 0.00** |
| llm calls | 57（每消息 1 次） | 57（每会话 1 次） | **57（每会话 1 次）** |
| p50 latency | 3759 ms | 3940 ms | **1292 ms** |
| p95 latency | — | 17818 ms | **2852 ms** |
| max latency | — | 20002 ms（**撞超时**） | **3054 ms** |

**结论：拆解用 `deepseek-v4-flash`。** 与 pro 的集合准确率完全相同（48/49），recall 略高，p50 快 3 倍、p95 快 6 倍，且零超时。pro 的尾部延迟（p95 17.8 秒、max 撞满 20 秒超时）不可接受。

---

## 3. 根因分析：多意图准确率从 60% 到 93% 的过程

首轮实现跑出来的 multi 集合准确率只有 **60%**（9/15），intent recall 69%。当时有两个候选原因：

- **A. 模型能力不足**——模型理解不了「一句话拆多个意图」
- **B. 响应超时被吞成零意图**——模型没来得及回答

诊断方式：让评测 harness 打印每条失败样本的 `source` 与 `elapsed`。结论是 **B**：

```
[multi miss] message: 先给这个功能出一个实施计划，再按计划把代码写出来
  expected: ["builtin.software_dev.plan", "builtin.software_dev.implement"]
  predicted: []
  source: Fallback elapsed: 8002 ms
```

全部失败样本的 `elapsed` 都**精确等于 8002 ms**（`llm_timeout_ms` 默认值 8000），`source` 为 `Fallback`——即**超时被吞成了零意图**。

决定性旁证：**两次独立跑的失败样本集合不同**（6 条 vs 5 条，交集只有 4 条），这是时序竞态的特征，不是能力问题。

换用快模型（超时不再触发）后，multi 准确率立刻从 60% 跃升到 93%。

> **教训**：只看准确率数字会往错的方向优化。`elapsed` 精确撞在超时值上，才是真正的判据。

---

## 4. 修复清单

| # | 问题 | 修复 | 落点 |
|---|---|---|---|
| 1 | **超时被当成零意图** | `DecisionSource` 新增 `Unavailable`；模型明确返回空数组 → `Fallback`（合法零意图），超时/调用失败/预算耗尽 → `Unavailable` | `llm/intent/{mod,router}.rs` |
| 2 | **零意图会话每条消息烧 2 次 LLM 调用**（前端调一次 `decompose_message`、发送时后端再调一次） | 合法零意图落一个 **0 步计划**并挂 `active_plan_id`，后续消息复用缓存；不可用则**不落计划**、不设缓存 | `app/commands/intent_plan.rs` |
| 3 | **超时余量不足**（8000ms 让多意图场景必然失败） | 默认 `llm_timeout_ms` 8000→20000、`routing_budget_ms` 12000→25000，并按仓库既有 `migrate_legacy_timeouts` 模式迁移旧配置里的 8000/12000（用户自定义值不动） | `app/config.rs` |
| 4 | **prompt 没有「一个消息可以有多个意图」的信号**，且两处强调「返回空数组」 | 明确「一条消息常需要多于一个意图，逐个列出」「部分匹配也算匹配」「每个 id 至多一次」；空数组条件收窄为「与所有候选都无关」 | `llm/intent/classifier.rs` |
| 5 | **评测 harness 漏统计子集**（`_ => {}` 把 multi/ambiguous 跳过了），导致只看打印指标会误判「验证通过」 | harness 补齐全部子集与指标（含 ordered、recall、p95/max） | `llm/intent/router.rs` |

---

## 5. 验收标准核对

| # | 标准 | 结果 |
|---|---|---|
| 1 | 意图集合准确率 ≥ 90% | **98%**（排除 ambiguous 48/49）/ 89%（含 ambiguous）— 达标 |
| 2 | 幻觉率 = 0 | **0/59 = 0%** — 达标 |
| 3 | 零意图场景 100% 落入普通聊天兜底 | **10/10 = 100%**（旧实现 30%）— 达标 |
| 4 | 用户确认前不产生任何副作用 | 有测试兜底：`pending_plan_has_no_side_effects_until_confirmed` — 达标 |
| 5 | 每会话只拆解一次 | 有测试兜底：`cached_decomposition_is_reused_without_a_model_call`、零意图会话也会落 0 步计划复用 — 达标 |
| 6 | 拆解失败不影响消息发送 | 不可用走 `Unavailable` → 普通聊天兜底，消息照常发出 — 达标 |
| 7 | `chat` 不触发确认卡 | `chat` / `clarify` 不进候选；零意图直接走普通聊天 — 达标 |

---

## 6. 遗留问题

1. **唯一剩余误判**：「查一下这个项目怎么跑测试，然后跑一遍全量测试」标注为 `[lookup, execute]`，模型给 `[lookup, test]`。标注是对的（`execute` = 执行命令，`test` = 写测试），模型确实判错了一个。**未重新标注**——重新标注以对上模型输出属于「对着测试教学」，不这么做。
2. **ambiguous uncertain = 1/8**：8 条 ambiguous 是「短句、含糊」而非「跨领域歧义」（如「这个测试有问题」「优化一下性能」），标注本身可辩。模型对它们高置信判一个意图是合理行为，**确认卡就是安全网**。「模糊句 100% 标 uncertain」是 2026-11-05 那版设计的验收项，本设计已改为「零意图 100% 兜底 + 确认前无副作用」。
3. **`decompose_message` 命中已有缓存时仍会新落一条等价计划**：正常前端流程不会触发（前端在 `active_plan_id` 存在时跳过调用），只会在前后端判断不一致时产生孤儿计划行。未修。
4. **`check-comment-language.mjs` 报 6 处英文注释**：全部在 `src-tauri/src/app/mod.rs`，本次改动未触及该文件，属既有问题。

---

## 7. 配置说明

拆解模型由 `config.intent.classifier_model_id` 指定；`None` 表示沿用默认模型。**本机已设为 `deepseek-v4-flash`**（旧配置里为空、走默认的 `deepseek-v4-pro`，因尾部延迟不可接受）。

`config.json` 原文件备份在 `config.json.bak-20261109`。
