# claude 目录与模型窗口（Catalog 同构）

> 状态：v1 **已实施**（commit `89954de` / apmux v0.1.11+），等待评审。
> 背景是 Claude Code 对非常规模型 ID（如网关别名 `hilinkup/z-ai/glm-5.3-flash`）
> 的未知模型提示，要求手动设 `CLAUDE_CODE_MAX_CONTEXT_TOKENS` 或在
> `modelOverrides` 中映射。apmux v0.1.10 之前只把 `ANTHROPIC_MODEL` 写进
> `settings.json`，代理行必然触发该提示。v1 在每个 claude provider 上引入
> `catalog: Vec<ModelEntry>`，apply 阶段写 `modelOverrides` + `MAX_CONTEXT_TOKENS`。
> 仍有 §8 列出的几个跟踪项。

---

## 1. 背景：触发与目标

**触发**：hilinkup 等代理把 Claude Code 的 `ANTHROPIC_BASE_URL` 指向网关、`ANTHROPIC_MODEL` 指向网关模型别名（非 `claude-*`）。Claude Code 不识别该 ID，启动时打印：

> `"<id>" is not a model this version of Claude Code recognizes, so auto-compact
> will keep this session within 200k tokens (the context window it assumes). If
> the model accepts more, append [1m] to the model name for 1M, or set
> CLAUDE_CODE_MAX_CONTEXT_TOKENS to its real window; to make it recognized, map
> it in the modelOverrides setting or update Claude Code;
> CLAUDE_CODE_DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT=1 restores the previous
> wait-for-the-API behavior.`

**目标**：让 apmux 代理行自带这些旋钮，且数据模型与 codex/pi 一致（catalog 同构，
context_tokens 在每行里直接编辑），live `settings.json` 完全由 catalog + slots + quick
items 推出。

---

## 2. Claude Code 侧已核实事实

> 源：https://code.claude.com/docs/en/model-config、env-vars、settings；GitHub #33316。

| 机制 | 位置 | 语义要点 |
|---|---|---|
| `CLAUDE_CODE_MAX_CONTEXT_TOKENS` | `env` | 对**不以 `claude-` 开头**、**不含 `[1m]`**、**无法解析为 Claude 模型** 的 ID 直接生效。声明后按该窗口做主动压缩。 |
| `CLAUDE_CODE_DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT=1` | `env` | 关闭主动压缩；只在 API 报"prompt is too long"后被动恢复。**需 Claude Code v2.1.223+**。 |
| `modelOverrides` | `settings.json` 顶层键 | **key 必须是已识别的 Anthropic 模型 ID**（未知 key 被忽略）；**value 才是发往 API 的名字**。"a value you configured yourself in `modelOverrides`" 被识别为自定义模型（诊断静默）。`ANTHROPIC_MODEL`/`ANTHROPIC_DEFAULT_*_MODEL`/`--model` 都会被该 map 改写。**v2.1.73+**。 |
| `[1m]` 后缀 | 模型名 | 对固定第三方/已识别 ID：先剥离再发送。对**不可识别** ID 含 `[1m]`：直接假设 1M 窗口，且**`MAX_CONTEXT_TOKENS` 失效**（需叠加 `CLAUDE_CODE_DISABLE_1M_CONTEXT=1`）。不可识别 ID 是否剥离文档未明示。 |
| `CLAUDE_CODE_DISABLE_1M_CONTEXT=1` | `env` | 配合 `[1m]` 修正窗口。 |
| `CLAUDE_CODE_AUTO_COMPACT_WINDOW` / `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` | `env` | 全局压缩阈值/百分比，与本设计无直接关系。 |

**对本场景的推论**：hilinkup 别名（非 `claude-*`、无 `[1m]`）→ `CLAUDE_CODE_MAX_CONTEXT_TOKENS` 是最干净的主路径；`modelOverrides` 作为识别+改写补强。

---

## 3. apmux 侧已核实事实

- **`Provider.snippet`**（JSON SSOT）→ `apply` 时 deep-merge 进 `~/.claude/settings.json`；apmux 自有 env 键（`ANTHROPIC_BASE_URL`/auth/model/5 slots）由 `patch_claude_env` 写入，**后于 snippet 写入，优先**（merge → override）。
- **`quick.rs::CLAUDE`** 已有 5 项 `QuickItem`（如 `teammates` → `env.CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`），通过 `apply_snippet` / `remove_snippet` 在 `Provider.snippet` 上 merge/unmerge。TUI 通过 form.rs 的 `models_summary` 与 models 页的 `ModelPicker` 渲染。
- **`ModelUi` 枚举**：`Catalog { fields }`（codex/pi/opencode）vs `Slots { slots }`（claude 唯一）。catalog 编辑器 `CatalogEditor` 已支持 Id/Label/ContextWindow/MaxTokens 增删行与字段编辑。
- **`ModelEntry`**（`store.rs`）：`{ id, label, context_window, max_tokens }`。codex adapter 已在消费 `context_window` 写 `model_providers` 的 `context_window`/`max_context_window`。
- **`Provider`**：`model: Option<String>`（Default）、`slots: BTreeMap<String,String>`（5 slot 落点）、`catalog: Vec<ModelEntry>`、`extras: BTreeMap<String,String>`、`snippet`/`apply_snippet`/`official`。
- **`claude.rs` 现状**：
  - `FIELDS` 含 5 个 + `api_key_field`（Extra）。
  - `model_ui() = ModelUi::Slots { slots: CLAUDE_SLOTS }`（5 项）。
  - `patch_claude_env` 写入 `ANTHROPIC_BASE_URL` / auth / `ANTHROPIC_MODEL` / 5 slot env；`official=true` 时全清。

---

## 4. 设计方案

### 4.1 数据模型

**`ModelEntry` 扩一列**（serde 默认 + skip，存量零迁移）：

```rust
pub struct ModelEntry {
    pub id: String,
    pub label: Option<String>,
    pub context_window: Option<u64>,
    pub max_tokens: Option<u64>,
    /// claude 专用：行代理的 Anthropic 模型 ID（精确、带日期后缀），
    /// 取自 `KNOWN_CLAUDE_MODEL_IDS`（从本地 Claude Code 二进制提取）。
    /// 决定了 env 中 `ANTHROPIC_*_MODEL` 的写法与 `modelOverrides` 的参与。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_model_id: Option<String>,
}
```

**Provider 不动**：`provider.model`（Default）和 `provider.slots`（slot 落点）保持 SSOT；catalog
编辑器把这两处的写入渲染到行网格（Default 用 radio、其它 slot 用多选），存储结构不变
→ `patch_claude_env` 现有 slot 消费逻辑保持。单落点约束由 `BTreeMap` 天然保证。

### 4.2 Catalog 编辑器（claude 的 models 页）

```
┌──────────────────────────┬───────┬──────────────┬──────────────┬────────────────────────┐
│ id / label               │ ctx   │ slots (多选) │ Default      │ target model id (下拉)│
├──────────────────────────┼───────┼──────────────┼──────────────┼────────────────────────┤
│ hilinkup/z-ai/glm-5.3    │ 200k  │ sonnet, sub  │ ●            │ claude-sonnet-4-6      │
│ hilinkup/z-ai/glm-5.3-x  │ 1m    │ haiku, opus  │              │ claude-opus-4-7        │
└──────────────────────────┴───────┴──────────────┴──────────────┴────────────────────────┘
```

- **slots 列**：行内多选弹层，逐 slot 指派；指派即"搬家"（旧行的该 slot 失位）。
- **Default 列**：radio 单选，写入 `provider.model`。
- **target model id 列**：下拉，候选 = `KNOWN_CLAUDE_MODEL_IDS`（从本地 Claude Code 二进制提取，**只含未带日期的别名**——带日期的快照 ID 会过时，未带日期的别名永远指向当前版本），首项 `(none)` 表示清空。下拉里只能选**精确 Anthropic 模型 ID**（如 `claude-opus-4-8`）—— 短别名（`sonnet-4-5` 等）不能作 `modelOverrides` key（按官方文档：未知的 key 被忽略）。
- `ModelUi::Catalog` 切换（claude 不再走 `Slots` 变体）→ `form.rs::models_summary` 与 models 页统一走 catalog 分支。

### 4.3 live 生成规则（`patch_claude_env` 重构）

| live 键 | 来源 |
|---|---|
| `ANTHROPIC_BASE_URL` / auth | 现状 |
| `ANTHROPIC_MODEL` | Default 落点行的 `target_model_id`（若在 `KNOWN_CLAUDE_MODEL_IDS` 内）否则 `.id`（= `provider.model`） |
| `ANTHROPIC_DEFAULT_{HAIKU,SONNET,OPUS,FABLE}_MODEL` / `CLAUDE_CODE_SUBAGENT_MODEL` | 各 slot 落点行：同上规则——`target_model_id`（若在 `KNOWN` 内）否则 `.id` |
| **`CLAUDE_CODE_MAX_CONTEXT_TOKENS`** | **min(catalog 中所有非空 `context_window`)**；全空或无行 → 不写 |
| **`modelOverrides`** | 对每个 catalog 行：若 `target_model_id` 在 `KNOWN` 内，写一条 `{ "<target>": "<行 .id>" }`；否则跳过 |

**`modelOverrides` 生成示例**（对应 §4.2 的两行；两行都选了 `target_model_id`）：

```json
"modelOverrides": {
  "claude-sonnet-4-6": "hilinkup/z-ai/glm-5.3",
  "claude-opus-4-7":   "hilinkup/z-ai/glm-5.3-x"
}
```

对应的 env 同步写入已知 ID：

```json
"env": {
  "ANTHROPIC_MODEL":                "claude-sonnet-4-6",
  "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-4-6",
  "ANTHROPIC_DEFAULT_SUBAGENT_MODEL": "claude-sonnet-4-6",
  "ANTHROPIC_DEFAULT_HAIKU_MODEL":  "claude-opus-4-7",
  "ANTHROPIC_DEFAULT_OPUS_MODEL":   "claude-opus-4-7"
}
```

效果：Claude Code 看到的所有 `ANTHROPIC_*_MODEL` 都是已知 ID（`claude-sonnet-4-6`、`claude-opus-4-7`）—— 窗口、auto-compact、诊断都按已知模型处理；请求实际发出前，`modelOverrides` 把已知 ID 翻译成网关别名（`hilinkup/...`）。未选 `target_model_id` 的行：env 值就是行 `.id`（网关别名），`modelOverrides` 不写该行（未知 key 会被忽略，写了也没用）。

**`CLAUDE_CODE_MAX_CONTEXT_TOKENS` 推导示例**：3 行 catalog，窗口分别为 200000 / 1000000 / None → min(200000, 1000000) = **200000**（None 跳过）。全空 → 不写该键。

**`modelOverrides` 重复 target 行为**：若 catalog 中两行都映射到同一 Anthropic ID（如都填 `claude-sonnet-4-6`），按 `provider.catalog` Vec 的迭代顺序后者胜出（last-wins）。TUI 当前没有 visual warning——属于 §4.2 "搬家"的同族 UX 问题，未来加行重排能力时一起处理。

### 4.4 QuickItem

```rust
// quick.rs::CLAUDE 追加
QuickItem {
    id: "unknown_model_reactive",
    label: "quick.unknown_model_reactive",
    snippet: Some(
        r#"{"env":{"CLAUDE_CODE_DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT":"1"}}"#
    ),
    extra_key: None,
},
```

一键切"被动压缩"。label 文案需提示：网关若改写溢出报错文案（agate 有错误归一化
层，详见 §6 疑问 2），该模式会失效。

### 4.5 兼容与迁移

- 加载时若 claude provider `catalog` 为空但 `provider.model` 非空 → **播种一行** `{ id: model, context_window: None, target_model_id: None }`，避免空网格；apply 时若无窗口则不写 `MAX_CONTEXT_TOKENS`，行为等同现状。
- `official=true` 行：照旧全清，不消费 catalog。
- 不动 `Provider` 结构；`patch_claude_env` 现有 slot 消费保留，仅追加 2 段（MAX_CONTEXT_TOKENS、modelOverrides）。

---

## 5. 工作量拆解

| 文件 | 内容 | 量级 |
|---|---|---|
| `store.rs` | `ModelEntry.target_model_id` + 播种迁移 | 小 |
| `adapter/models.rs` | `CLAUDE_FIELDS`（含新 CatalogField 变体或独立编辑态） | 中 |
| `claude.rs` | `model_ui()` 切 Catalog + apply 生成规则 + 测试 | 中 |
| `tui/pages/models.rs` | CatalogEditor 扩展：slots 多选弹层、Default radio、target model id 下拉 | 大头 |
| `quick.rs` + `i18n.rs` | 新 QuickItem + 中英文 label | 小 |

---

## 6. 开放问题（请其它 agent 协助分析）

### 疑问 1：catalog 行的 `target_model_id` 是否需要绑定 slot 落点
**现象**：用户期望同一行既能"代理"某个 Claude 已知 ID（出现在 `modelOverrides` / 5 个 env 路径上），也要被指派到至少一个 slot 才有实际作用（否则写在 env 里但没人调用）。

**当前默认设计**：`target_model_id` 下拉对**任何行**开放，不强制绑定 slot。未被指派 slot 的行选了 `target_model_id` 也无害——apply 阶段 `modelOverrides` 仍会写入对应映射（`claude-sonnet-4-6 → <行 id>`），只是 `ANTHROPIC_DEFAULT_*_MODEL` 的 5 个变量不会引用该行。

**决策**：不做 UI 门禁；`target_model_id` 是下拉，"(none)" 是首项以清空。

**Why**：与方案 b 不同的设计理由——`modelOverrides` 文档说"unknown keys are ignored"，所以 `target_model_id` 不在 `KNOWN_CLAUDE_MODEL_IDS` 的行被 `patch_claude_env` 自动跳过（不写入）。剩余的"行选 target 但没 slot"情况：env 不会引用该行，`modelOverrides` 仍会生成该映射但不被任何 env 触发（等价于 dead config，不破坏既有行为）。这与之前的"无 slot 也允许"决策一致——slot 是 slot 维度的 SSOT，target 是 target 维度的 SSOT，两者独立。

### 疑问 2：agate 错误归一化是否破坏 `CLAUDE_CODE_DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT=1` 的被动恢复
**现象**：该 quickitem 依赖 API 报"prompt is too long"等可识别文案触发被动恢复。agate 在
`workers/agate/worker/` 有 `errorLayer` / 上游错误归一化层（cf-workers 仓库），可能改写
溢出报错文案，导致 Claude Code 识别不到而不恢复。

**决策**：agate 不改写 Anthropic wire 错误文案，被动恢复（`CLAUDE_CODE_DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT=1`）可用。

**Why**：三处证据一致——
1. `cf-workers/workers/agate/worker/main.ts:191` 的 `v1Error(... 'anthropic' ...)` 路径直接调用 `createAnthropicErrorBody(message, type)`，原 message 透传；
2. `cf-workers/workers/agate/worker/providers/endpoints/index.ts:407` `markPassthrough` 标记的透传路径只重置错误归一化层，不重写 body；
3. `cf-workers/packages/ai-protocol/src/runtime/errors.ts` 仅覆盖 OpenAI 侧（"OpenAI 错误归一化层"），无 Anthropic 分支。

quickitem label 可保留 §4.4 现有的"网关若改写溢出报错文案..."的兜底说明，但风险点已消除，无需在 label 里加精确警告。

### 疑问 3：`modelOverrides` 的 v2.1.200 改写行为
**现象**：docs 明说"Overrides also apply when an Anthropic model ID is passed directly via
`--model`, `ANTHROPIC_MODEL`, or an `ANTHROPIC_DEFAULT_*_MODEL` variable (before v2.1.200,
`--model`/env values bypassed the override map)"。当前 Claude Code 版本需确认 ≥ v2.1.200，
否则 `ANTHROPIC_DEFAULT_SONNET_MODEL="gateway-id"` 不会被改写。

**决策**：当前环境 Claude Code 为 v2.1.251（≥ v2.1.200），覆盖 `ANTHROPIC_DEFAULT_*_MODEL` 经 `modelOverrides` 改写的行为；无需版本探测，quickitem label 也不必附加版本要求。

**Why**：`/home/razo/.local/share/claude/versions/2.1.251` 二进制已安装（ELF 格式、可执行）。v2.1.200 改写行为在该版本之后，2.1.251 自然覆盖——二进制中 `ef`（key→value）与 `yMe`（value→key）改写函数均为 alive 状态。无需运行时 probe，因为本机即可假定 ≥ 2.1.200；若用户用更老版本，问题在"该用户应升级"而非 apmux 兼容。

### 疑问 4：`CLAUDE_CODE_MAX_CONTEXT_TOKENS` 对识别后的 modelOverrides 值是否仍生效
**现象**：docs 说"id doesn't start with `claude-` (any casing) and can't be resolved to a
Claude model → the variable **applies directly**"。当 ID 出现在 `modelOverrides` value 后，
"被识别为自定义模型"——但它**不是 Claude 模型**，文档未明示窗口假设。

**决策**：`CLAUDE_CODE_MAX_CONTEXT_TOKENS` 对 `modelOverrides` 的 value 仍生效。两条机制独立：value 仍是 ID，仍走"不以 `claude-` 开头、无法解析为 Claude 模型"的判定（§2 表第 1 行 case 1），`MAX_CONTEXT_TOKENS` 直接生效。

**Why**：文档原文 "applies directly" 是按"该 ID 在运行时是否被识别"判定，不按"它来自 env 还是 modelOverrides value"。value 被识别为"自定义模型"只关闭 Claude Code 的诊断告警，不改变窗口假设路径。`patch_claude_env` 同时写两条互不冲突。

### 疑问 5：Default 行同时承载其它 slot 是否要警告
**现象**：设计上允许多 slot 落同一行（自然约束）。但若 glm-5.3-x 同时被 Default + haiku +
opus 选中，单一 `CLAUDE_CODE_MAX_CONTEXT_TOKENS`（取 Default）会与 haiku/opus 实际窗口冲突。

**决策**：§4.3 表行"Default 落点行的 `.context_window`"已重写——`MAX_CONTEXT_TOKENS` 取 **catalog 全表非空 `context_window` 的 min**，不再绑定 Default 行。无需 UI 警告。

**Why**：原方案 (b) 编辑器加 ⚠ 提示偏离 SSOT（`context_window` 已逐行编辑），且 (c) "min(Default, 各 slot)" 在 Default 窗口 ≥ slot 窗口时已等价于 (b) 的提示但更早触发。`min(全部非空窗口)` 是更保守的通用解：(i) Default 窗口可能小于 slot 实际窗口，max 才错；(ii) 与 Q1 决策"slot 多选、Default radio"独立，编辑态不需要特殊联动；(iii) 一次 apply 一锤定音，不依赖 Default 与 slot 的耦合。

### 疑问 6：catalog 播种策略
**现象**：存量 claude provider `catalog=[]` + `model=Some("x")` → 播种 `{ id: "x" }` 一行。
但若 `model=None`（历史或非默认）则不播种 → 编辑器空网格，UX 困惑。

**决策**：store load 时一次性迁移——claude provider `catalog` 为空时，按 (a)+(b) 复合策略播种：
1. 若 `provider.model` 是 `Some(id)` → 播种一行 `{ id, context_window: None, target_model_id: None }`，不指派 slot（保留原状，作为 Default 行的可编辑入口）；
2. 遍历 `provider.slots`，每个非空 value 若不在 catalog 中 → 播种一行 `{ id: slot_value, context_window: None, target_model_id: None }`；
3. 若 (1)(2) 都无播种结果 → 编辑器渲染"添加一个模型"占位行（id 为空、可编辑），不预填 id。

**Why**：方案 (a) 解决"model 已配但 catalog 空"（多数存量 case），方案 (b) 解决"model 为 None 但 slots 已配"的边角 case。占位行仅渲染层，不入 SSOT，避免空 id 污染 catalog。一次性 store-load 迁移优于每次渲染时种行（避免脏写）。

### 疑问 7：跟 cc-switch-cli 经验的对照
**现象**：项目 `~/Workspace/cc-switch-cli/src-tauri/src/proxy/providers/transform_codex_chat.rs`
的 `append_responses_input_as_chat_messages` 在最近修复中也合并了 assistant 文本与
`function_call`、清掉 parts-array 残留，是个"模型层有偏好 → 协议层兜底"的成功例子。

**决策**：Claude Code 侧**不存在**"模型自报窗口"的协议扩展；`CLAUDE_CODE_MAX_CONTEXT_TOKENS` + `modelOverrides` 是仅有的两条机制。apmux 无需在 `claude.rs` 中追加兼容性拼接。

**Why**：`code.claude.com/docs/en/model-config` / `env-vars` / `settings`（§2 源）已枚举所有相关 env 与 settings 键；§2 表的 6 行穷尽 `MAX_CONTEXT_TOKENS` / `DISABLE_UNKNOWN_*` / `modelOverrides` / `[1m]` / `DISABLE_1M_*` / `AUTO_COMPACT_WINDOW` / `AUTOCOMPACT_PCT_OVERRIDE`，无任何"模型在请求中声明窗口"的 client 端钩子。cc-switch-cli 的 `transform_codex_chat.rs` 解决的是 OpenAI Responses ↔ Chat Completions 协议差异，与"模型窗口声明"不同维度，不可类比。

---

## 7. 实施时间线

- **2026-08-29 commit `89954de` / apmux v0.1.11** — §1–§6 全部落地，§4.3 表的 live 生成规则实现。CI 在 Windows 上因 pre-existing `try_launch::end_to_end_launch_uses_isolated_env`（与本设计无关）失败，不影响 release 流程。

## 8. Open Risks

1. **Q5 min() 过度保守**：若某行 `context_window` 误填小值（如占位 1）会被全表 min 拉低，污染 `MAX_CONTEXT_TOKENS`。**已落 mitigation**：`CatalogEditor::commit_edit` 在 apmux v0.1.12 起对 `ContextWindow` 解析后过滤 `>= 1000`，小于阈值视为 None 不写入。重复 target 行为在 `claude.rs::patch_claude_env` 里**last-wins**（按 `provider.catalog` Vec 顺序）—— 用户两行映射到 `claude-sonnet-4-6` 时后者胜出；TUI 当前没有 visual warning（属于 §4.2 "搬家"的同族 UX 问题）。
2. **slot 重新指派的撤销语义**：取消 slot 落点后旧行直接清空（in-editor `delete_row` 也清 `slot_owner`），状态栏会提示"已清 N 个 slot"；但若用户希望"回滚到上一个拥有者"——见 §4.2 "搬家"措辞——需要单独设计。**待建 issue**：TUI 重做行重排时一并处理。
3. **`modelOverrides` 多版本兼容性**：v2.1.200 之前的 Claude Code 仍会旁路 env 改写。Q3 决策只覆盖本机 2.1.251，跨用户安装版本无法保证。**待建 issue**：README 加 `requires Claude Code >= v2.1.200` 提示。
4. **agate 未来若新增 Anthropic 错误改写**：Q2 结论依赖"agate 不动 Anthropic wire 错误"的现状。agate 升级时需保留 `createAnthropicErrorBody(message, type)` 透传契约——可考虑在 agate 测试里加一条"Anthropic 4xx 文案保真"用例，但属于跨仓工作。**待建 issue**。
5. **Q6 占位行的"添加一个模型"渲染**：实现期需明确占位行的编辑提交时机（首次失焦即物化 / 显式按钮），本设计未定。**待建 issue**。
