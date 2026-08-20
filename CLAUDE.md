---
owner: OMP Model Studio maintainers
status: active
updated: 2026-08-20
tags:
  - pi
  - oh-my-pi
  - upstream
  - configuration
links:
  - AGENTS.md
  - ROLE-GUIDE.md
---

# CLAUDE.md — 上游产品与配置参考

> **定位**：本文件是 OMP Model Studio 所管理的两个上游产品（`pi`、`oh-my-pi`/`omp`）及其配置格式的
> **owner 文档**——源码地址、目录/配置文件对照表、术语表都以本文为准。仓库内其它文档（`AGENTS.md`、
> `ROLE-GUIDE.md`）引用本文，不重复维护这些事实；发现不一致时以本文为准并回来更新本文。

## 1. 上游源码（本机克隆，只读参考）

| 产品 | 源码路径 | 说明 |
|---|---|---|
| `pi` | `/opt/workspace/opensource/pi` | 独立的 coding-agent CLI 核心（`packages/agent`、`packages/ai`、`packages/coding-agent`、`packages/server` 等）。 |
| `oh-my-pi`（对外命令 `omp`） | `/opt/workspace/opensource/oh-my-pi` | 另一个 coding-agent CLI 核心，本仓库 README 中链接的 [Oh My Pi](https://github.com/can1357/oh-my-pi)。同样有 `packages/coding-agent`，另含 Bazel/Rust 构建面。 |

这两个仓库不被本项目（Tauri 桌面壳）构建或引用，只用于在读不懂某个配置字段/行为时，离线核对上游源码与 `docs/` 目录，而不是靠猜或靠网络搜索。两个仓库会随上游演进；核对时以本机克隆当前内容为准，必要时用 `git -C <path> log -1` 确认版本再下结论。

## 2. `pi` 与 `oh-my-pi` 是两个不同的产品

它们是**同一谱系、彼此独立的两套代码**（各自的 git 仓库、各自的内置 agent 清单、各自的扩展机制），只是共享了相似的配置概念（命名模型角色、按 agent 覆盖模型、按 provider 禁用）。**从一个仓库读到的事实不能想当然套用到另一个**——本项目里 `App.tsx`/`api.ts`/`lib.rs` 顶层命令管 `omp` 一侧，`PiPanel.tsx`/`piApi.ts`/`lib.rs` 里 `*_pi_*` 命令管 `pi` 一侧，核对时要去对应的源码仓库，不要混用。

## 3. 配置文件对照表

| | `oh-my-pi`（`omp`） | `pi` |
|---|---|---|
| 配置根目录 | `~/.omp/agent/` | `~/.pi/agent/` |
| 全局配置文件 | `config.yml`（YAML） | `settings.json`（JSON） |
| 自定义 Provider / Model catalog | `models.yml`（YAML） | `models.json`（JSON/JSONC） |
| 自定义 Agent 定义 | `agents/*.md`（frontmatter + 提示词正文） | `agents/*.md`（同形状：`name`/`description`/`model`/...） |
| 项目级配置 | `<project>/.omp/config.yml`、`<project>/.omp/agents/*.md` | `<project>/.pi/settings.json`、`<project>/.pi/agents/*.md`（项目优先于用户） |
| 模型路由 | `modelRoles.<role>`：命名选择器别名（如 `@slow`、`@advisor`），供 `@role` 引用；`task.agentModelOverrides.<agent>` 覆盖固定的内置 agent（如 `reviewer`、`security-reviewer`） | `subagents.defaultModel` / `subagents.defaultThinking` 兜底；`subagents.agentOverrides.<agent>` 是扁平 map，一个 agent 一条 `{model, thinking, fallbackModels}` |
| Provider 禁用 | 顶层 `disabledProviders`（`config.yml`） | 顶层 `disabledProviders`（`settings.json`）；Studio UI 里称为 "shadow" |
| 扩展机制 | OMP extension roots：原生 `.omp/agents`、OMP 扩展包、Claude marketplace 插件（见 `docs/task-agent-discovery.md`） | `settings.json` 的 `packages` 数组列出 npm 包（如 `pi-subagents`、`pi-computer-use`），实装安装在 `~/.pi/agent/npm/node_modules/`，扩展脚本挂在 `~/.pi/agent/extensions/*.ts` |

两边的模型选择器格式一致：`provider/model-id[:thinking]`（如 `openai-codex/gpt-5.6-terra:high`），或角色别名（`@slow`）。

## 4. "pi agent"（subagent）从哪来

`pi` 核心本身**不内置** subagent 派遣系统——那是一个独立维护的第三方 pi 扩展包：
**[`pi-subagents`](https://github.com/nicobailon/pi-subagents)**（本机装在
`~/.pi/agent/npm/node_modules/pi-subagents`，由 `settings.json` 的 `packages` 列表引入）。

- 它自带的内置 agent（`agents/*.md`）：`delegate`、`oracle`、`researcher`、`reviewer`、`scout`、`worker`。
- 本项目 Rust 后端的 `PI_BUILTIN_AGENTS`（`src-tauri/src/lib.rs`）在此基础上额外列了 `gpt-pro`。
- 自定义的（本机可见：`code-mapper`、`product-designer`、`agent-tooling-engineer` 等）从
  `~/.pi/agent/agents/*.md` 的 frontmatter 发现，`name:` 字段即 agent 名。

**"pi agent"** 在本仓库语境下 = 这份花名册里的一个具名条目（内置或自定义），每个都可以通过
`subagents.agentOverrides.<name>` 独立路由到某个模型选择器。这与 `oh-my-pi` 侧的
"Agent 定义"（见 `ROLE-GUIDE.md` §3）是同一层概念，但派遣/发现机制和内置清单完全不同，不要互相套用。

## 5. "Tier" 是什么

`pi-subagents` 自己的文档（`docs/models.md`，"Recommended model tiering" 一节）里，Tier 只是一种
**非强制的分组建议**，按任务形状分四档：

1. **Fast workhorse** —— 最便宜的可用模型 + 低思考量，用于侦察/查找/机械式编辑（例：`scout`）。
2. **Standard well-scoped** —— 中档模型 + 中等思考量，用于大多数边界清晰的委派任务（例：`worker`、`reviewer`）。
3. **Deep but bounded** —— 顶级推理模型 + 高思考量，只用于目标和完成标准都写清楚的难任务（例：oracle 类 agent）；目标模糊时这类模型容易空转，不要用在开放式任务上。
4. **Taste and intent** —— 擅长读懂人类意图、能做判断而不空转的模型，用于 UX/产品取舍、模糊需求下的规划等开放式工作。

上游文档里这只是指导性建议，pi-subagents 实际读取的仍然是扁平的
`subagents.agentOverrides.<agent>` map，并没有 "Tier" 这个配置原语。

`~/.pi/agent/tiers.json` 把这份建议**变成了真正可编辑的一等数据结构**：

```json
{
  "default": "<tier 名>",
  "tiers": { "<tier 名>": { "model": "...", "thinking": "...", "fallbackModels": [...] } },
  "assignments": { "<tier 名>": ["agent 名", ...] }
}
```

`pi` 和 `pi-subagents` 都不会直接读这个文件——它是一份"单一事实来源"，需要被**展开**成
`settings.json` 里 `subagents.agentOverrides` 那个扁平 map，这样改一个 tier 就能同时改完所有分配到
它名下的 agent，而不必逐个 agent 改。做这件"展开"的实现有两处，语义必须保持一致：

- `~/.pi/agent/apply-tiers.mjs`——早于本项目就存在的独立脚本，支持 `apply`/`show`/`panic`/`set` 子命令。
- `src-tauri/src/lib.rs` 的 `save_pi_tiers`/`expand_pi_agent_overrides`——本项目对同一逻辑的原生 Rust
  重新实现，由 `src/PiPanel.tsx` 驱动。它对 "shadow 一个 provider" 的处理（`PiPanel.tsx` 里
  `reassignTiersAwayFromShadowedProviders` 上方的注释）是照着 `apply-tiers.mjs panic` 的行为特意对齐的：
  立刻换用 fallback 列表里下一个候选，而不是把受影响的 tier 都标红等人工修。

`PiTier.useCase`（UI 里的"使用场景"输入框）是 Studio 独有的元数据，只存在于 `tiers.json`，从不展开进
`agentOverrides`——`pi-subagents` 不认识这个字段。

## 6. 需要深挖时去哪查

- `oh-my-pi` 侧模型角色/Agent 发现/解析的完整语义：先看仓库内 [`ROLE-GUIDE.md`](ROLE-GUIDE.md)（已系统梳理，仅覆盖
  `omp`），核对源码时用 `oh-my-pi/docs/task-agent-discovery.md`、`docs/models.md`、`docs/settings.md`。
- `pi`/`pi-subagents` 侧模型路由、Tier、fallback、missions 等：
  `~/.pi/agent/npm/node_modules/pi-subagents/docs/`（`models.md`、`agents.md`、`configuration.md`、
  `workflows.md`）——这是本机实际安装的插件版本，比 `/opt/workspace/opensource/pi` 仓库本身更贴近真实
  运行行为（`pi-subagents` 是独立发布的第三方包，不随 `pi` 核心仓库一起改动）。
- 两个源码仓库都可能领先或落后于本机安装的可执行文件版本；精确核对时优先信任已安装 CLI 的自省输出
  （`omp config list --json`、`omp models --json`、`pi --list-models`），源码只用来解释"为什么是这样"。
