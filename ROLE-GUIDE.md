---
owner: OMP Model Studio maintainers
status: active
updated: 2026-08-16
tags:
  - omp
  - models
  - agents
  - configuration
links:
  - README.md
---

# OMP 模型角色、Agent 与控制面指南

> **适用范围**：OMP Model Studio 所管理的模型角色、Agent 定义与运行时控制语义。
> **核对基线**：OMP v17.3.4；使用当前安装的 `omp config list --json`、`omp models --json` 和对应源码核对。
> **权威边界**：本文解释配置关系，不保存个人配置快照。实际生效值以全局/项目配置、Agent 定义、会话覆盖和当前 OMP 版本为准。

## 1. 先分清七个概念

| 概念 | 是什么 | 不是什么 |
|---|---|---|
| **模型角色**（model role） | `modelRoles` 中的命名模型选择器，例如 `@slow`、`@advisor`。它解决“此用途用哪个模型”。 | 不是一个可执行的 Agent。|
| **Agent 定义** | 一个 `*.md` 文件：任务提示、工具权限、模型引用、思考级别和输出约束。它解决“谁来做、允许做什么”。 | 不是一次正在运行的任务。|
| **Skill** | 可复用的方法、流程和领域规则。它解决“怎么做”，可由主会话或多个 Agent 共同使用。 | 不是模型选择器，也不是独立执行会话。|
| **Agent 实例 / 子 Agent** | 主会话通过 `task` 启动的一次实际工作会话。它有自己的上下文、模型、状态和输出。 | 不是 `/agents` 中的一行配置。|
| **Advisor** | 附着在一个会话上的异步旁路审查者；持续观察主 Agent 的增量并提出意见。 | 不是主 Agent 的审批器，也不是普通可协作子 Agent。|
| **`/agents` Agent Control Center** | 配置和发现 Agent 定义的界面：模型覆盖、禁用、prewalk、子 Agent advisor。 | 不是运行中任务的调度台。|
| **Agent Hub**（`Alt+A` / `Ctrl+S`） | 运行时查看、聚焦、steer、恢复或终止已启动子 Agent 的界面。 | 不修改 Agent 定义和长期策略。|

核心关系：

```text
用户任务
├─ Skill：怎么做（可由主会话或多个 Agent 复用）
└─ Agent 定义：谁独立执行、允许哪些工具、遵守哪些边界
   └─ Role：用哪个模型 / thinking
      └─ 具体 provider/model[:thinking]

主会话
├─ task(...) → Agent 定义 → 一个子 Agent 实例
├─ Advisor（可选）→ 旁路观察当前会话
├─ /agents → 配置未来 spawn
└─ Agent Hub → 管理已经 spawn 的实例
```

**Skill 决定怎么做；Agent 定义决定谁做以及工具和边界；Role 只决定模型；Control Center 配置未来 spawn；Agent Hub 管理已经 spawn 的实例。**

---

## 2. 模型角色：默认规则与使用方式

### 2.1 配置语法

角色在 `config.yml` 的 `modelRoles` 中映射到模型选择器：

```yaml
modelRoles:
  default: openai-codex/gpt-5.6-terra:max
  slow: openai-codex/gpt-5.6-sol:max
  review: anthropic/claude-opus-5:xhigh  # 自定义角色
```

引用时使用 `@角色名`：

```yaml
task:
  agentModelOverrides:
    reviewer: "@review"
```

要点：

- 推荐使用完整 `provider/model-id[:thinking]`，避免同名模型在不同 provider 间歧义。
- 思考级别可附在选择器后，例如 `:minimal`、`:low`、`:medium`、`:high`、`:xhigh`、`:max` 或 `:off`；最终是否可用仍取决于 provider 与模型能力。
- 角色可以引用另一个角色，例如 `task: "@default"`；不要形成循环引用。
- `*` 是当前默认角色的简写；`pi/角色名` 是旧兼容写法。新配置统一使用 `@角色名`。
- 角色选择器只会从已认证、未禁用且可用的模型中解析。显式写了不可用模型，不会神奇地得到等价替代。

### 2.2 一个关键事实：没有跨安装固定的“默认模型”

`modelRoles` 的设置默认值是空对象 `{}`，不是一组固定 provider/model-id。OMP 会依照已配置模型、会话选择、可用 provider 和内建优先级解析角色。因此：

- **想要稳定、可预测的成本和能力，显式配置角色。**
- **角色未配置时，下面的“默认行为”是解析或回退规则，不是某个保证存在的具体模型。**
- 命令行显式选模、项目范围选模和恢复的会话模型可优先于正常初始选择。

### 2.3 内建角色完整表

| 角色 | 主要消费者 | 未显式配置时的默认行为 | 实务建议 |
|---|---|---|---|
| `default` | 主交互会话 | 没有固定内建模型；使用当前会话/已保存选择/可用模型的正常初始解析。 | 日常实现的主模型。显式配置以保证稳定。 |
| `smol` | `scout`、`librarian`、`sonic`、轻量后台工作 | 自身未设时：若已配置 `default`，**只**继承该选择器；仅在 `default` 也未设时才走内建快速/低成本优先级。 | 研究、机械改动、短任务。单独配置，不要误把主模型成本带进所有轻任务。 |
| `slow` | 内建 `reviewer` | 自身未设时：若已配置 `default`，**只**继承该选择器；仅在 `default` 也未设时才走内建高推理优先级。 | diff 审查、困难定位、需要慢推理的只读检查。 |
| `vision` | 图片检查工具 | 没有独立的通用优先级链；图片路径依次尝试 `vision`、`default`、当前活动模型，最后是第一个可用模型。 | 需要视觉输入时单独设更可靠的视觉模型。 |
| `plan` | `/plan` 模式 | 是计划模式的临时切换目标；若无法解析则保持当前模型而不是凭空选一个。退出 plan mode 会恢复原活动模型。 | 用于需要先设计、再执行的任务。 |
| `designer` | 内建 `designer` Agent | 自身未设时：若已配置 `default`，**只**继承该选择器；仅在 `default` 也未设时才走设计/多模态优先级。 | UI/UX、视觉实现与设计审查。 |
| `commit` | `omp commit` | commit 选择依次尝试 `commit`、`smol`、其余可解析角色。 | 单独设一个快速、文字质量稳定的模型。 |
| `tiny` | 标题、memory、自动思考难度分类、异常停止等后台小任务 | 未配置时复用 `smol` 的解析优先级。 | 与 `smol` 同模型通常合理；要压低后台成本时单独设。 |
| `task` | 通用 `task` 子 Agent | 通用 Agent 若没有显式 `@task` 映射，会回退到父会话活动模型、父会话模型字符串，最后才是已配置的 `default`。 | 通用实现子任务的默认模型；与主会话分开可控制并发成本。 |
| `advisor` | 主会话 Advisor；显式为子 Agent 启用的 Advisor | 未配置时使用 `slow` 的优先级，**但不会先继承 `default`**。显式配置却无法解析时，Advisor 不会静默换成别的模型。 | 保持独立于主模型；用于高风险旁路审查。 |

内建快速、慢速和设计优先级列表会随 OMP 版本演进。需要追踪当前精确候选时，查看本机源码中的 `packages/coding-agent/src/config/model-resolver.ts` 与 `packages/coding-agent/src/priority.json`，不要把旧文档中的具体模型列表当作长期配置。

### 2.3.1 未配置时，最终到底 fallback 到哪里？

**结论：没有一个适用于所有角色的全局“最后 fallback 模型”。** OMP 先解析角色本身；角色解析失败后，由每个功能的调用点决定继续使用当前模型、尝试别的角色、选第一个可用模型，还是直接不运行/报错。

“未配置”还必须区分两种情况：

1. **只省略某个角色，但已配置 `modelRoles.default`**；
2. **`modelRoles` 整体为空**，即没有为任何角色指定模型。

#### 角色解析层：省略角色时会产生什么候选

| 省略的角色 | `default` 已配置时 | `modelRoles` 整体为空时 | 最后一个内建优先级模式 | 若所有候选都不可用 |
|---|---|---|---|---|
| `default` | 不适用：它本身就是显式主模型选择。 | 没有角色优先级模式。 | 无 | 由主会话初始选模流程决定，见下表。 |
| `smol` | **只**使用 `default` 的选择器；不会在该选择器失败后继续 `smol` 优先级。 | 使用 `smol` 优先级列表。 | `mini` | 角色解析为无模型。 |
| `slow` | **只**使用 `default` 的选择器；不会在该选择器失败后继续 `slow` 优先级。 | 使用 `slow` 优先级列表。 | `pro` | 角色解析为无模型。 |
| `designer` | **只**使用 `default` 的选择器；不会在该选择器失败后继续 `designer` 优先级。 | 使用 `designer` 优先级列表。 | `gemini-3-5-flash` | 角色解析为无模型。 |
| `tiny` | 不继承 `default`；使用 `smol` 优先级列表。 | 使用 `smol` 优先级列表。 | `mini` | 角色解析为无模型。 |
| `advisor` | 不继承 `default`；使用 `slow` 优先级列表。 | 使用 `slow` 优先级列表。 | `pro` | 角色解析为无模型。 |
| `vision` | 没有角色自身优先级。 | 没有角色自身优先级。 | 无 | 由图片检查工具的调用点继续 fallback，见下表。 |
| `plan` | 没有角色自身优先级。 | 没有角色自身优先级。 | 无 | plan mode 不切换模型，保留当前活动模型。 |
| `commit` | 没有角色自身优先级。 | 没有角色自身优先级。 | 无 | `omp commit` 会继续尝试其他角色，见下表。 |
| `task` | 没有角色自身优先级。 | 没有角色自身优先级。 | 无 | 通用子 Agent 使用父会话 fallback，见下表。 |

`smol` 当前优先级从 Cerebras GLM、Gemini Flash Lite/Flash、Haiku、Flash 逐步尝试到 `mini`；`slow` 从 GPT/Codex、Opus 逐步尝试到 `pro`；`designer` 从 Gemini Pro 逐步尝试到 `gemini-3-5-flash`。这些是**模式匹配顺序**，不是保证可调用的模型清单；解析只会选中当前已发现、可用的模型。

`default: "@smol"` 这类自引用是特例：为避免循环，OMP 会改用 `smol` 的内建优先级。不要依赖这种间接写法表达普通默认策略。

#### 消费者层：角色没有解析出模型后的最终结果

| 使用位置 | 最终 fallback / 结果 |
|---|---|
| 新主会话的 `default` | 依次考虑显式 CLI 模型、非恢复会话的 scoped model、保存的默认模型；之后选已发现模型中的已知 provider 默认模型；最后选 `availableModels[0]`。没有可用模型则无法得到主模型。 |
| `scout` / `librarian` / `sonic`（`@smol`）、`reviewer`（`@slow`）、`designer`（`@designer`） | 若对应角色优先级没有匹配到模型，**不会因为“没配角色”自动改用父会话或第一个可用模型**；该 Agent 无法以该选择器启动。仅当已解析出模型、但该模型缺少凭据时，子 Agent 的独立认证 fallback 才可能转用父会话活动模型。 |
| `tiny` 后台功能 | 没有统一终点。标题生成会尝试 `tiny → commit → smol → 当前活动模型`；其他调用点可跳过在线处理、报“没有 tiny/smol 模型”，或使用自己的 session/registry fallback。不要把 `tiny` 当作必然回落到 `default`。 |
| `inspect_image` 的 `vision` | 精确顺序为 `@vision → @default → 当前活动模型 → availableModels[0]`。最终模型仍须支持 image input 且有凭据，否则报错；“选到了模型”不等于图片检查一定可用。 |
| `/plan` 的 `plan` | 没有 `plan` 映射时不发生模型切换，继续用进入 plan mode 前的活动模型。 |
| `omp commit` 的 `commit` | 尝试 `commit → smol → 全部内建角色`；仍无模型则报 `No model available for commit generation`。 |
| 通用 `task` Agent | `@task` 未配置时，依次使用父会话当前活动模型、父会话的 `getModelString()` fallback、再到已配置的 `default`。三者都没有时无法 spawn；这里没有“第一个可用模型”的裸回退。 |
| Advisor | `advisor` 未配置且慢速优先级没有可用模型时，不启动 Advisor。显式配置了无效 `advisor` 模型时同样不静默换模型。 |

因此，想避免不同功能落到不同终点，最稳妥的做法不是依赖 fallback，而是至少明确配置 `default`、`smol`、`slow`、`designer`、`task` 和 `advisor`。

### 2.4 常见误解

1. **`reviewer` 不是内建模型角色。** 它是一个内建 Agent 定义，其 frontmatter 默认引用 `@slow`。写 `modelRoles.reviewer` 不会让它自动改用该角色。
2. **`tester` 不是 OMP 内建角色。** 如果配置中出现它，它只是自定义角色，只有显式引用 `@tester` 的 Agent 或流程才会使用。
3. **模型角色不创建 Agent。** `modelRoles.design` 或 `modelRoles.review` 只是别名；必须由某个 Agent 定义、命令或显式选择去引用它。
4. **Agent 名称和角色名称可相同，但语义不同。** `task` 既是内建角色名，也是通用 Agent 的名称；配置时要看它出现在哪个键下。

### 2.5 查看当前有效角色

本文不复制某台机器的 `modelRoles` 快照；该事实只由配置文件维护，避免文档与运行配置漂移。使用：

```bash
omp config get modelRoles --json
omp models --json
```

第一条显示当前配置范围内的角色映射；第二条显示当前可用模型及其输入、thinking、上下文、输出上限和成本元数据。命令行模型、项目级配置和恢复会话仍可能改变一次会话的最终选择。

---

## 3. Agent：定义、发现、有效模型

### 3.1 内建 Agent 与本机自定义 Agent

| Agent 名称 | 工作定位 | 定义中的模型倾向 | 变更权限/边界 |
|---|---|---|---|
| `scout` | 快速只读代码库勘察、模式搜索、影响面收集 | `@smol`，中等思考 | 只读。用于先找证据，不做实现。 |
| `librarian` | 外部库/API/源码研究 | `@smol`，minimal | 研究导向，不修改项目。 |
| `sonic` | 严格机械、低推理的数据收集或小更新 | `@smol`，medium | 仅限可廉价验证的机械任务。 |
| `task` | 通用实现、修复、整合子任务 | `@task` | 默认通用工作者；可编辑、可执行受任务允许的工具。 |
| `designer` | UI/UX 与视觉实现、视觉审查 | `@designer` | 面向设计实施，不是通用审查器。 |
| `reviewer` | 已形成 diff 的代码审查 | `@slow` | 只读；报告可定位的问题，不替代测试。 |
| `security-reviewer` | 证据驱动的安全审查 | 定义未固定具体角色，可由 `task.agentModelOverrides` 覆盖 | 只读；检查权限、输入、数据、网络和敏感边界。 |

使用建议：

- 需要发现信息：先 `scout`；需要第三方实现事实：`librarian`。
- 有清晰、可独立执行的实现切片：`task`。
- UI 设计或视觉实现：`designer`；真实界面/设备验证：使用当前环境中已配置的专用测试 Agent。
- diff 稳定后找回归：`reviewer`；安全边界、输入、权限、敏感数据：`security-reviewer`。
- `sonic` 只用于严格机械的任务；不要把模糊设计或重要修复委托给它。

### 3.2 Agent 从哪里发现，谁覆盖谁

同名 Agent 的发现优先级从高到低：

1. 当前项目最近的 `.omp/agents/`
2. 用户目录 `~/.omp/agent/agents/`
3. 已启用的 OMP extension package
4. 兼容的 Claude marketplace plugin roots
5. OMP bundled Agents

先发现的同名 Agent 覆盖后发现的版本。因此，项目可以用同名 `.omp/agents/<name>.md` 对本机或内建定义做有意识的项目级替换。直接放入 `.claude/agents/`、`.codex/agents/` 或 `.gemini/agents/` 并不是 OMP 的普通 Agent 发现路径。

一个 Agent 定义通常包含：

```md
---
name: example-agent
description: 何时应派遣它
model: "@task"
thinkingLevel: high
tools: [read, grep, glob]
prewalk: false
advisor: false
---

给该 Agent 的专用任务约束、输入和输出契约。
```

`name` 是派遣和覆盖键；文件名不是唯一身份。定义中的 `tools`、`spawns`、`blocking`、`autoloadSkills`、`readSummarize`、`prewalk`、`advisor` 和 `output` 等字段构成执行策略。

### 3.3 一个 Agent 到底用哪个模型

对于通过 `task(agent: "<name>")` 启动的普通子 Agent，按以下优先级确定有效模型：

```text
`task` 调用显式传入的内部 `requestModel`
    > Agent Control Center 的 `task.agentModelOverrides.<name>`
    > Agent frontmatter 中的 `model`
    > 父会话 `getActiveModelString()`（当前活动模型）
    > 父会话 `getModelString()`（会话 fallback）
    > 已配置的 `default` 角色
```

第一级是调用方传入的内部 `requestModel` 覆盖。普通 `task` wire schema 通常不暴露这个字段；它可由 eval bridge 或其他调用方提供，不能靠在常规 task item 中随意新增 `model` 字段替代。倒数第二级是父会话的 `getModelString()`；普通 spawn 通常与当前活动模型相同，但解析器将它作为独立 fallback 保留。

因此：

- 要改内建 `reviewer` 的模型，正确做法是给 `task.agentModelOverrides.reviewer` 设为 `@review` 或具体模型；**不是**新增 `modelRoles.reviewer`。
- 要保持可维护性，先定义语义角色，再让 Agent 覆盖引用它：

  ```yaml
  modelRoles:
    review: anthropic/claude-opus-5:xhigh
  task:
    agentModelOverrides:
      reviewer: "@review"
  ```

- 直接模型覆盖完全有效；多个 Agent 需要统一切换时，改为引用一个自定义语义角色更易维护。
- 父 Agent 与子 Agent 不共享完整聊天历史。派遣时必须给子 Agent 自足的目标、边界、输入、输出契约和验收条件；父 Agent 负责整合。

### 3.4 Skill、Agent 与 Role 的关系

三者是正交维度，不应因为名称相近而绑定：

| 需求 | 使用 |
|---|---|
| 只需要一套可复用方法 | Skill |
| 需要可独立调用的执行者 | Agent |
| 需要独立的模型、成本或延迟边界 | Role |
| 执行者需要稳定的模型边界 | Agent + Role |
| 执行者需要复用领域流程 | Agent + Skill |

决策顺序：

1. **先看 Skill。** 任务只是“用某种方法做事”时，由主会话加载 Skill 即可。
2. **稳定且需要独立调用时才建 Agent。** Agent 承担独立上下文、工具权限、任务提示和输出契约。
3. **只有需要独立模型预算时才建 Role。** Role 只做 `modelRoles.<role> → provider/model[:thinking]` 路由；自定义 Role 不要求同名 Agent Markdown。
4. Agent 可以引用任何已存在 Role，例如 `model: "@slow"`；不要求 Agent 名称与 Role 名称一致。Skill 也可由多个 Agent 复用。

典型组合：

- 写 PRD：主会话使用 `@slow` 并加载 `to-prd`；默认不需要 `product` Role 或 `product` Agent。
- 关键决策：主会话使用 `@slow` 并加载 `expert-lens`。
- 设备测试：专用 Role 提供模型边界，已安装的设备测试 Agent 提供独立执行者，设备或移动测试 Skill 提供方法。

因此，Model Studio 中的“自定义模型 Role”和“自定义 Agent”必须分开管理；路由预设不应复制 Agent Markdown 或 Skill 内容。

### 3.5 如何实际触发 `scout`

`/agents` 只配置 Agent，**不会启动** `scout`。目前没有内建的 `/scout` slash command。

在普通 OMP 对话中，直接要求主 Agent 使用指定 Agent，并给出只读范围和期望输出。例如：

```text
请使用 scout 只读勘察当前登录流程：
1. 找出入口、状态持有者和 API 调用点；
2. 列出可能受“增加 OAuth provider”影响的文件；
3. 不要修改任何文件；
4. 返回文件路径、关键符号和风险。
```

若必须使用它，写明“**必须使用 scout**”，不要只说“查一下”；主 Agent 随后会以 `task(agent: "scout")` 启动一个子 Agent。启动成功后，工具反馈会给出 Agent id；可用 `Alt+A` / `Ctrl+S` 在 Agent Hub 查看进度、聚焦或补充 steer。

对拥有 `task` 工具调用权的 Agent 或 SDK，实际派遣形态是：

```ts
task({
  context: "项目背景、约束和输出格式",
  tasks: [{
    name: "AuthScout",
    agent: "scout",
    task: "只读梳理登录流程；不编辑文件；返回入口、状态和 API 调用点。",
  }],
})
```

`scout` 是只读勘察者。让它实现、修改文件或运行重型验证是不匹配的；发现信息后，交给 `task`、`designer` 或 `device-tester` 做后续实际工作。

---

## 4. `/agents`：Agent Control Center

`/agents` 打开的是 Agent 定义的控制面。界面可能使用 “agents hub” 名称，但本文件称它为 **Agent Control Center**，以避免与运行时 Agent Hub 混淆。

它做的是：列出项目、用户和 bundled Agent 的来源；创建/选择定义；为**以后启动**的某个 Agent 设置策略。它不等于一个正在运行的子 Agent 列表。

| 控件 | 持久化键 | 作用 | 使用时机 |
|---|---|---|---|
| Enable / Disable | `task.disabledAgents` | 按 Agent 名称禁用或恢复派遣；不删除定义文件。 | 暂时禁止不应在当前环境使用的 Agent。 |
| Model | `task.agentModelOverrides.<agent>` | 覆盖 Agent frontmatter 的 `model`，优先级最高。清除后回到 Agent 默认策略。 | 对某个 Agent 单独控制成本、能力或 provider。 |
| Prewalk | `task.agentPrewalk.<agent>` | 设置 `agent default`、`on`、`off` 或模型/角色目标。开启时，Agent 先按正常模型开始；在形成计划待办后的第一次 edit/write 附近，转交给默认 `@smol` 或指定目标。 | 明确的“强模型先规划，快模型后执行”实现任务。 |
| Advisor | `task.agentAdvisor.<agent>` | 设置 `agent default`、`on`、`off` 或 Advisor 模型/角色。 | 只给高风险子任务增加旁路审查。 |
| New Agent | 新建用户/项目 Agent 定义 | 创建可复用的自定义 Agent，而不是一个临时实例。 | 有稳定、重复的角色、工具边界或输出契约时。 |

控制原则：

- 先在 Control Center 配好，再 spawn 子 Agent。运行中的实例已经在启动时解析了模型和策略。
- `agent default` 的意思是尊重 Agent 定义文件中的 frontmatter；不是“强制使用 `@default`”。
- 不要为 `scout`、`reviewer`、`security-reviewer`、`device-tester` 开启 prewalk：它们的价值不是“规划后编辑实现”。
- `task.agentAdvisor` 是**子 Agent opt-in**；主会话开启的 Advisor 不会自动递归附着到所有子 Agent。
- Control Center 只调模型/派遣策略。要改变 Agent 的任务规则、工具许可或提示词，编辑相应 `agents/*.md` 定义。

### 4.1 推荐的 Control Center 配置形态

将“哪个模型更适合什么”放在 `modelRoles`，将“哪个 Agent 用哪个角色”放在 `task.agentModelOverrides`：

```yaml
modelRoles:
  implement: openai-codex/gpt-5.6-terra:xhigh
  review: openai-codex/gpt-5.6-sol:max
  visual-qa: openai-codex/gpt-5.6-luna:max

task:
  agentModelOverrides:
    task: "@implement"
    reviewer: "@review"
    device-tester: "@visual-qa"
```

这样替换模型只改一处角色映射；Agent 定义和工作流名称保持稳定。

---

## 5. Agent Hub：运行时实例控制

**Agent Hub** 通过 `Alt+A`（或 `Ctrl+S`）打开。它管理的是已经由 `task` 启动的实际 Agent 实例，而不是定义配置。

可查看的信息包括：实例状态、任务/活动、实际模型、耗时、成本、工具调用和 token。常用操作包括：

- 聚焦到某个子 Agent；
- 发送 steer 消息，补充边界或纠正方向；
- 恢复 parked Agent；
- 终止错误、重复或不再需要的实例；
- 查看父子树。

主 Agent 不作为普通子 Agent 出现在这个 roster 中。Advisor 若出现，属于只读的旁路条目：不能像普通子 Agent 一样向其发消息、恢复或终止。

运行时协作关系：

```text
Agent Control Center (/agents)
      │ 配置未来 spawn 的策略
      ▼
主 Agent 以 task 启动子 Agent ─────────────► Agent Hub (Alt+A / Ctrl+S)
      │                                       │ 管理正在运行的实例
      │                                       └─ steer / focus / revive / stop
      └─ 主 Agent 通过 hub 与普通子 Agent 通信

Advisor：附着于某个会话，读取其增量；不作为普通协作 peer。
```

---

## 6. Advisor、reviewer 与 security-reviewer：三个不同的审查层

| 机制 | 何时运行 | 作用范围 | 默认模型/策略 | 能否修改主任务状态 |
|---|---|---|---|---|
| **Advisor** | 主会话进行时，异步持续运行 | 当前主会话；子 Agent 需单独 opt-in | `@advisor`；未配置时按 slow 优先级，但不继承 `default` | 不能。它只能给建议和中断信号。 |
| **reviewer** | diff 已形成、适合审查时 | 一个明确的 diff/变更范围 | 内建 Agent 使用 `@slow` | 不能。只读报告发现。 |
| **security-reviewer** | 安全敏感的设计、变更或审计时 | 权限、输入、数据、网络、边界等证据范围 | 定义未固定模型；本机被直接覆盖为 Sol | 不能。只读报告安全问题。 |

### 6.1 Advisor 的默认配置和语义

OMP 的安装默认值：

```yaml
advisor:
  enabled: false
  syncBacklog: "off"
  immuneTurns: 3
```

检查全局配置中的当前值：

```bash
omp config get advisor.enabled --json
```

`advisor.enabled: true` 会为新主会话启用 Advisor；当前交互会话仍可用 `/advisor on` 或 `/advisor off` 临时切换，无交互/headless 场景可使用 `--advisor`。

Advisor 的工作方式：

- 异步接收主会话的增量、工具意图/结果和项目上下文，可进行只读工作区检查。
- 默认工具是只读检查工具加 `advise`；它不会批准外部动作、修改主 Agent 的状态或自动替主 Agent 写入结果。
- 建议有 `nit`、`concern`、`blocker` 等严重度。`concern`/`blocker` 可能中断主 Agent；一次中断后，`immuneTurns` 默认 3 个主回合会将后续同类提示改为非中断式，避免循环打断。
- 没有 roster 时，主 Advisor 使用 `modelRoles.advisor`。有 `WATCHDOG.yml` roster 时，可以按条目为多个 Advisor 指定模型、启用状态和职责。
- `WATCHDOG.md` 为 Advisor 追加项目审查指令；`WATCHDOG.yml` 定义 roster。它们可放在用户配置或项目根/祖先 `.omp` 位置，项目级规则用于覆盖通用规则。
- 旧的 `advisor.subagents: true` 只会以旧语义覆盖通用 task；新配置应使用 `task.agentAdvisor.<agent>` 精确 opt-in。

### 6.2 `syncBacklog` 的 YAML 类型陷阱

`advisor.syncBacklog` 是**字符串 enum**，合法值严格为 `"off"`、`"1"`、`"3"`、`"5"`，默认 `"off"`。它的作用是：Advisor 比主会话落后指定回合数时，主会话最多等 30 秒让 Advisor 追上。

YAML 中必须加引号：

```yaml
advisor:
  # 默认就是 off；可直接省略这一项。
  syncBacklog: "off"

  # 需要更强同步时：
  # syncBacklog: "1"
```

不要写裸 `off` 或裸 `1`：YAML 可能把它们解析为 boolean 或 number，而设置 schema 要求 string。这是配置错误，不是关闭同步的可靠写法。

### 6.3 何时用哪个机制

- **常规小变更**：不需要 Advisor 或 reviewer；主 Agent + 针对变更的验证即可。
- **跨模块、权限、数据迁移、外部副作用或高回归风险**：在主会话启用 Advisor；仅在确实需要逐回合追上时才设 `syncBacklog: "1"`。
- **变更完成且 diff 稳定**：运行 `/review`，让 `reviewer` 检查回归、契约和可维护性；随后由主 Agent 判断、修复并验证。
- **认证、授权、Intent、文件路径、网络请求、密钥、隐私、敏感数据**：追加 `security-reviewer`，并将审查范围写清楚。
- **不要把 Advisor 当作测试，也不要把 reviewer 当作运行时保护。** 两者都补充而不替代针对真实行为的测试/烟测。

如果希望 Advisor 只用于高风险会话，保留本机映射但将全局 `advisor.enabled` 改为 `false`，然后在需要的会话执行 `/advisor on`。如果保留当前 `true`，日常低风险会话可用 `/advisor off` 临时关闭。此处是工作流选择，不要求改变当前配置。

---

## 7. 一个完整、可复用的协作工作流

### 7.1 普通代码变更

```text
1. 主 Agent（@default）确认目标、现有模式和最小验证路径。
2. 范围未知时并行委派 scout / librarian 只读收集证据。
3. 独立实现切片交给 task；每个任务都写明目标、非目标、接口、验收。
4. 主 Agent 整合结果，运行受影响的真实行为验证。
5. diff 稳定且风险值得时，再 /review；主 Agent 处理有证据的发现。
```

### 7.2 高风险变更

```text
1. /plan 使用 @plan 明确不变量、数据边界和回滚/验证路径。
2. 退出 plan mode，模型恢复到原主会话选择。
3. /advisor on；必要时配置 syncBacklog: "1"，避免 Advisor 落后太多。
4. task 做实现；security-reviewer 做独立只读安全检查。
5. 主 Agent 整合，跑受影响测试和真实烟测。
6. /review 审查最终稳定 diff；修复后重跑相关验证。
```

### 7.3 UI 或设备路径

```text
1. designer：信息层级、交互、视觉实现。
2. task：需要的非视觉实现切片。
3. 主 Agent 合并并启动实际目标。
4. device-tester：真实浏览器/设备/UI 流程验证，不改应用源代码。
5. reviewer：仅在稳定 diff 需要额外回归审查时运行。
```

### 7.4 并行的边界

可并行的是互不依赖的研究、独立实现切片或独立审查；不是把同一段未定义的任务复制给多个 Agent。父 Agent 应预先定义跨切片接口和文件所有权，最终由一个主 Agent 统一整合和验证。

子 Agent 之间可由主 Agent 使用运行时 hub 发送补充说明；Advisor 不是普通 peer，不能这样协调。Advisor 默认也不会自动覆盖子 Agent，避免把每个并发子任务都扩大成多模型审查成本。

---

## 8. 配置操作手册

### 8.1 只想替换一个 Agent 的模型

不要改它的 prompt，不要新造同名角色。通过 `/agents` 的 **Model** 控件，或等价地写：

```yaml
task:
  agentModelOverrides:
    reviewer: "@review"
```

再在 `modelRoles` 中维护 `review` 的具体模型。清除覆盖即可回到 Agent frontmatter 的默认模型。

### 8.2 想新增一个稳定职责

1. 在 `~/.omp/agent/agents/` 创建用户 Agent，或在项目 `.omp/agents/` 创建项目专用 Agent。
2. 定义清晰的 `name`、`description`、`model`、工具边界和输出契约。
3. 如果模型语义值得复用，先创建 `modelRoles.<semantic-name>`，再在 frontmatter 中引用 `@<semantic-name>`。
4. 从 `/agents` 检查其来源和有效模型；之后用 `task(agent: "<name>")` 启动。

只有重复出现、工具/提示/输出约束确实不同的工作，才值得新建 Agent。一次性任务直接用 `task` 并写好任务契约即可。

### 8.3 想只对一个子 Agent 开启 Advisor

在 `/agents` 选择该 Agent 的 **Advisor** 控件，设为 `on` 或明确模型/角色。等价配置示例：

```yaml
task:
  agentAdvisor:
    task: "@advisor"
    security-reviewer: "on"
```

这会为未来的对应子 Agent 实例附着 Advisor；它与主会话的 `advisor.enabled` 是两条独立策略。

### 8.4 想使用 prewalk

只对“先规划、后实施”的 `task` 类工作开启。`on` 时先由正常模型形成计划，再在第一次编辑/写入附近交给默认 `@smol`；如果低成本执行模型不合适，设置明确的目标角色，而不是依赖隐式回退。

审查、研究、设备测试和短机械任务通常不该 prewalk：它们不是长实现链，切换模型只会增加状态转换成本和判断误差。

### 8.5 想配置模型 fallback

`retry.fallbackChains` 的 key 可以是 Role、具体模型或 provider wildcard，值是按顺序尝试的模型选择器：

```yaml
retry:
  modelFallback: true
  fallbackChains:
    default:
      - openai-codex/gpt-5.6-luna:xhigh
    openai-codex/*:
      - openai/*
  fallbackRevertPolicy: cooldown-expiry
```

Fallback 是请求失败或额度恢复策略，不是日常按成本自动降级。候选不能回到 target 自身，也不应指向 shadowed provider。Model Studio 将这些值保存在草稿中，并在 Apply 前检查空值、重复、自循环和 provider 冲突。

### 8.6 想删除自定义 Role

先清除所有 `@role` 引用，包括其他 Role、Agent frontmatter 和 fallback chain；再在 Model Studio 中连续两次点击“移除”，最后 Apply。删除 Role 只删除 `modelRoles.<role>`，不会删除 Agent Markdown 或 Skill。未 Apply 前只是草稿，重新读取会恢复磁盘状态。

---

## 9. 故障定位清单

| 现象 | 最可能原因 | 先检查什么 |
|---|---|---|
| `/agents` 里没有预期 Agent | 放错目录、同名定义被更高优先级覆盖，或被禁用 | 项目 `.omp/agents/`、`~/.omp/agent/agents/`、来源标签、`task.disabledAgents`。 |
| 改了 `modelRoles.reviewer`，reviewer 仍是旧模型 | `reviewer` Agent 默认引用的是 `@slow` | 设置 `task.agentModelOverrides.reviewer: "@review"`，或改 `slow`。 |
| 子 Agent 没用预期模型 | Control Center 覆盖、frontmatter 和父会话回退的优先级被误解 | 在 `/agents` 查看有效模型，检查 `task.agentModelOverrides.<name>`。 |
| Advisor 没出现 | 主会话关闭、`@advisor` 无法解析，或只配置了子 Agent advisor | 检查当前会话 `/advisor` 状态、`advisor.enabled` 和 `modelRoles.advisor`。 |
| Advisor 配置报类型错或不起作用 | `syncBacklog` 被 YAML 解析成非字符串 | 使用 `"off"`、`"1"`、`"3"` 或 `"5"`，或省略以采用默认。 |
| 想控制运行中的 Agent，却在 `/agents` 操作 | `/agents` 只配未来策略 | 用 `Alt+A` / `Ctrl+S` 打开 Agent Hub，聚焦、steer、恢复或终止实例。 |
| Advisor 无法被 steer/kill | 它不是普通子 Agent | 改变主会话 Advisor 状态或配置；不要把它当协作 peer。 |

---

## 10. 维护边界与依据

- **上游源码地址、`pi` 与 `omp` 配置对照、Tier 术语表**：见 [`CLAUDE.md`](CLAUDE.md)（owner 文档，本节不重复维护）。
- **全局配置**：`~/.omp/agent/config.yml`
- **用户 Agent 定义**：`~/.omp/agent/agents/*.md`
- **项目 Agent 定义**：`<project>/.omp/agents/*.md`
- **项目级模型/策略配置**：`<project>/.omp/config.yml`
- **Advisor 专用规则**：用户或项目范围的 `WATCHDOG.md`、`WATCHDOG.yml`
- **本指南**：项目根目录 [`ROLE-GUIDE.md`](ROLE-GUIDE.md)

核对本文时优先使用当前安装版本提供的：

- `omp --help`、`omp config list --json` 和 `omp models --json`；
- `src/config/model-roles.ts`、`model-resolver.ts` 与 `settings-schema.ts`：角色、fallback 和设置行为；
- `src/task/agents.ts`、`types.ts` 与 `src/discovery/helpers.ts`：Agent 定义、frontmatter 和派遣行为；
- Advisor、Agent Hub 与 task 工具的当前源码和上游文档。

升级 OMP 后若语义或字段发生变化，以当前安装版本为准，更新本文的 `updated` 与核对基线；不要在本文维护个人配置快照。
