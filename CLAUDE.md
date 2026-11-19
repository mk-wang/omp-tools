---
owner: OMP Model Studio maintainers
status: active
updated: 2026-08-21
tags:
  - oh-my-pi
  - upstream
  - configuration
links:
  - AGENTS.md
  - ROLE-GUIDE.md
---

# CLAUDE.md — 上游产品与配置参考

> **定位**：本文件维护 OMP 的上游源码位置与配置格式。

## 1. 上游源码（本机克隆，只读参考）

上游 [Oh My Pi](https://github.com/can1357/oh-my-pi) 的本机只读克隆位于 `/opt/workspace/opensource/oh-my-pi`，不参与 Studio 构建。查证配置字段时先核对该仓库的源码和 `docs/`。

## 2. OMP 配置文件

| 用途 | 路径 |
|---|---|
| 全局配置 | `~/.omp/agent/config.yml` |
| 自定义 Provider / Model catalog | `~/.omp/agent/models.yml` |
| 自定义 Agent | `~/.omp/agent/agents/*.md`（YAML frontmatter 和提示词正文） |
| 项目级配置 | `<project>/.omp/config.yml`、`<project>/.omp/agents/*.md`（项目优先于用户） |

### 2.1 `disabledProviders` 与 `shadowedProviders` 的边界

在 OMP Model Studio 中，这两个名称必须保持为两个独立概念：

| 名称 | Owner 与持久化 | 运行时语义 | Model Studio 行为 |
|---|---|---|---|
| `disabledProviders` | OMP 的 `~/.omp/agent/config.yml`；由 OMP 维护和解释 | 禁止指定 provider 提供的模型参与 OMP 的可用模型与模型解析；因此 `omp models --json` 不会返回这些模型 | Studio 不把它加载成 shadow，不用它补回左侧 Provider List，也不在 Apply 时修改它；外部已有值原样保留 |
| `shadowedProviders` | Studio React 草稿状态；可随 WebView 本地预设保存，不写入 OMP config | 仅在当前 Studio 编辑会话中把一个**当前可用** provider 视为暂时不可选 | 左侧列表只显示 `omp models --json` 返回的 provider；取消 Use 只更新这个草稿 overlay，并触发 Role/fallback 重分配与校验 |

因此，OMP 左侧 Provider List 的唯一模型来源是 `omp models --json`。不要把 `disabledProviders` 中的 provider 再 union 回列表，也不要把 `shadowedProviders` 序列化成 OMP 的 `disabledProviders`。Apply 只提交模型 Role、路由策略和 Agent 文件；shadow 是编辑辅助状态，不是 OMP provider 禁用配置。

## 3. 进一步查证

- 模型角色、Agent 发现与模型选择器格式见 [`ROLE-GUIDE.md`](ROLE-GUIDE.md)。相应的上游文档是 `docs/task-agent-discovery.md`、`docs/models.md` 和 `docs/settings.md`。
- 本机安装的 CLI 可能与克隆版本不同。核对当前行为时优先使用 `omp config list --json` 和 `omp models --json`，再用上游源码解释结果。
