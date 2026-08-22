---
owner: OMP Model Studio maintainers
status: active
updated: 2026-08-21
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
| 模型路由 | `modelRoles.<role>`：命名选择器别名（如 `@slow`、`@advisor`），供 `@role` 引用；`task.agentModelOverrides.<agent>` 覆盖固定的内置 agent（如 `reviewer`、`security-reviewer`） | `settings.json` 的 `"pi-roster"` 块：`defaultModelProfile`、`modelProfiles.<name>:{model,fallbackModels}`、`assignments`；每个 model/fallback 都是自带 thinking 后缀的完整 spec；另有 `disabledAgents` 与 `maxRecursionDepth` |
| Provider 禁用 | 顶层 `disabledProviders`（`config.yml`）；OMP 用它排除该 provider 提供的模型，`omp models --json` 返回的可用模型已经应用这一过滤。它是 OMP 运行时配置，不是 Model Studio 的 shadow 状态 | 顶层 `disabledProviders`（`settings.json`）；Pi Studio 的 shadow 仍对应 Pi 的真实 provider 禁用 |
| 扩展机制 | OMP extension roots：原生 `.omp/agents`、OMP 扩展包、Claude marketplace 插件（见 `docs/task-agent-discovery.md`） | `settings.json` 的 `packages` 数组列出 npm 包（如 `pi-roster`、`pi-computer-use`），实装安装在 `~/.pi/agent/npm/node_modules/`，扩展脚本挂在 `~/.pi/agent/extensions/*.ts` |

### 3.1 `disabledProviders` 与 `shadowedProviders` 的边界

在 OMP Model Studio 中，这两个名称必须保持为两个独立概念：

| 名称 | Owner 与持久化 | 运行时语义 | Model Studio 行为 |
|---|---|---|---|
| `disabledProviders` | OMP 的 `~/.omp/agent/config.yml`；由 OMP 维护和解释 | 禁止指定 provider 提供的模型参与 OMP 的可用模型与模型解析；因此 `omp models --json` 不会返回这些模型 | Studio 不把它加载成 shadow，不用它补回左侧 Provider List，也不在 Apply 时修改它；外部已有值原样保留 |
| `shadowedProviders` | Studio React 草稿状态；可随 WebView 本地预设保存，不写入 OMP config | 仅在当前 Studio 编辑会话中把一个**当前可用** provider 视为暂时不可选 | 左侧列表只显示 `omp models --json` 返回的 provider；取消 Use 只更新这个草稿 overlay，并触发 Role/fallback 重分配与校验 |

因此，OMP 左侧 Provider List 的唯一模型来源是 `omp models --json`。不要把 `disabledProviders` 中的 provider 再 union 回列表，也不要把 `shadowedProviders` 序列化成 OMP 的 `disabledProviders`。Apply 只提交模型 Role、路由策略和 Agent 文件；shadow 是编辑辅助状态，不是 OMP provider 禁用配置。

两边的模型选择器格式一致：`provider/model-id[:thinking]`（如 `openai-codex/gpt-5.6-terra:high`）；pi-roster 的 primary `model` 与每个 `fallbackModels` 条目都必须带 thinking 后缀，thinking 不再是 profile 字段。

## 4. "pi agent"（subagent）从哪来

`pi` 核心本身**不内置** subagent 派遣系统；本仓库当前对接的是独立扩展
[`pi-roster`](https://github.com/tmustier/pi-roster)，通过 `settings.json` 的 `packages` 数组启用。

- pi-roster 不提供固定的内置 agent；它从 `~/.pi/agent/agents/*.md` 发现用户自定义 agent。
- 项目级 agent 需要显式启用并经过信任检查，不会自动并入用户级花名册。
- 本项目 Rust 后端的 `discover_pi_agent_names` 与上述发现边界保持一致，不再维护一份固定 builtin 列表。

本仓库语境下，“pi agent”就是这份 `.md` 花名册中的一个具名条目。它的默认模型由
`pi-roster.assignments` 决定；未分配的 agent 使用 `defaultModelProfile`。

## 5. pi-roster 的 model profile

pi-roster 只读写 `~/.pi/agent/settings.json` 顶层的 `"pi-roster"` 配置块，不读取
`tiers.json`、`apply-tiers.mjs` 或 `subagents.agentOverrides`。当前 Studio UI 为保持既有编辑体验，
仍把 profile 卡片称为 Tier；Rust 在 settings 边界将其转换为下面的正式 schema：

```json
{
  "pi-roster": {
    "defaultModelProfile": "standard",
    "modelProfiles": {
      "standard": {
        "model": "kimi-coding/k3-256k:medium",
        "fallbackModels": ["opencode-go/glm-5.3:high"]
      }
    },
    "assignments": {"standard": ["reviewer"]},
    "disabledAgents": [],
    "maxRecursionDepth": 2
  }
}
```

`modelProfiles` 与 `assignments` 是开放 map；profile 名、agent 名和数量由使用方决定。每个
primary/fallback model spec 必须是 `provider/model-id:thinking`，profile-level `thinking` 与其它未知
profile 字段都不被 pi-roster 接受。Provider shadow 仍写入 settings 顶层真实的 `disabledProviders`，对 Pi 全局模型选择生效。

## 6. 需要深挖时去哪查

- `oh-my-pi` 侧模型角色/Agent 发现/解析的完整语义：先看仓库内 [`ROLE-GUIDE.md`](ROLE-GUIDE.md)（已系统梳理，仅覆盖
  `omp`），核对源码时用 `oh-my-pi/docs/task-agent-discovery.md`、`docs/models.md`、`docs/settings.md`。
- pi-roster 侧以 `/Volumes/CaseSenHD/WorkSpace/mk/pi-roster/docs/DESIGN.md` 及其
  `src/model-profiles.ts`、`src/roster-config-store.ts` 为当前本机版本的配置 schema 与持久化实现依据。
- `pi` 核心的 settings、models 与扩展加载行为仍以本机安装版本和 `/opt/workspace/opensource/pi` 为参考；
  不要把 pi-subagents 的旧 `subagents.*` schema 套到 pi-roster 上。
- 两个源码仓库都可能领先或落后于本机安装的可执行文件版本；精确核对时优先信任已安装 CLI 的自省输出
  （`omp config list --json`、`omp models --json`、`pi --list-models`），源码只用来解释"为什么是这样"。
