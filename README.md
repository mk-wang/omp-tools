# OMP Model Studio

OMP Model Studio 是一个基于 Tauri 2、React 19 和 Rust 的桌面控制面，用可视化方式管理 [Oh My Pi（OMP）](https://github.com/can1357/oh-my-pi) 的模型 Role、内建 Agent 模型覆盖、自定义 Agent、custom provider 和 provider 状态。

项目直接读写本机 OMP 配置，不代替 OMP 运行时；它只管理显式的模型路由和 `models.yml` custom provider 配置，不管理 OMP OAuth 登录流程。

## 功能

- **全部内建模型 Role**：配置 `default`、`smol`、`slow`、`plan`、`task`、`vision`、`designer`、`commit`、`tiny` 和 `advisor`。
- **内建 Agent 模型覆盖**：分别配置 `reviewer` 与 `security-reviewer` 使用的模型；两者不是模型 Role。
- **自定义模型 Role**：为独立的模型、成本或延迟边界增加自定义 Role；移除采用两步确认，并阻止删除仍被引用的 Role。
- **自定义 Agent**：创建和编辑 `~/.omp/agent/agents/*.md`，使用 YAML frontmatter 描述名称、用途和模型 Role。
- **模型能力与价格**：从 `omp models --json` 展示输入能力、thinking、上下文、输出上限及 input/output/cache 价格。
- **路由策略**：配置全局 thinking、auto thinking 上限、Advisor runtime、Prewalk 和 retry model fallback。
- **Fallback chain**：按 Role、具体模型或 provider wildcard 维护有序候选，并校验空值、重复、自循环和 shadowed provider。
- **Provider 管理与一键切换**：Provider 列表严格来自 `omp models --json` 的当前可用结果（OMP 已按 `disabledProviders` 过滤模型）；勾掉某个 provider 的 Use 只在当前草稿中 shadow 它，并立即把每个受影响 Role 切换到其 fallback chain 中下一个可用候选（没有可用候选的 Role 会保持标红，需要手动选择）。Apply 不会读取或修改 OMP 真实的 `disabledProviders`。
- **Custom provider**：填写 provider ID、API key、Base URL 和总 API type；总 Type 作为默认值，单个 model 可额外覆盖为另一种 API，通过 provider 的 `/models` endpoint 获取可用 model list，并自动尝试兼容的 rich catalog 读取 thinking effort；再勾选模型并立即独立写入 `~/.omp/agent/models.yml`；不依赖全局 Apply。
- **直接编辑配置文件**：Header 可打开 `config.yml`，Custom Provider 区域可打开 `models.yml`，外部保存后点击“重新读取”加载最新内容。
- **完整路由预设与主题**：在 WebView `localStorage` 中保存模型、路由策略和 provider 状态；Agent Markdown 保持独立。
- **安全写入**：配置文件、`models.yml` 和 Agent Markdown 使用同目录临时文件写入后原子替换。

## Role 与 Agent 的边界

| 概念 | OMP 配置位置 | 作用 |
| --- | --- | --- |
| 模型 Role | `modelRoles.<role>` | 只决定模型选择，不定义执行权限、工具或任务 |
| 内建 Agent 覆盖 | `task.agentModelOverrides.<agent>` | 覆盖 OMP 内建 Agent 使用的模型 |
| 自定义 Agent | `~/.omp/agent/agents/<name>.md` | 定义执行者、提示词、工具边界和输出契约，并可引用任意模型 Role |

模型选择器支持以下形式：

```text
provider/model-id[:effort]
@slow
@task
```

例如：

```text
openai-codex/gpt-5.6-terra:max
google-antigravity/gemini-3.7-flash:high
```


完整的 Role、Agent、Advisor 和控制面语义见 [`ROLE-GUIDE.md`](ROLE-GUIDE.md)。该指南不复制个人配置快照；实际值始终以当前 OMP 配置和安装版本为准。

## 环境要求

- 已安装并能从 `PATH` 找到 `omp`。请按照 [OMP 官方仓库](https://github.com/can1357/oh-my-pi) 的说明安装。
- [Bun](https://bun.sh/)：推荐且为 Tauri 配置的 `beforeDevCommand` / `beforeBuildCommand` 所需运行时。
- [Rust](https://www.rust-lang.org/tools/install) 和 Cargo；项目声明的最低 Rust 版本为 `1.77.2`。
- Tauri 所需的操作系统开发依赖。请参考 [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) 完成 macOS、Windows 或 Linux 配置。

`npm` 可以安装依赖并运行前端脚本，但 `tauri.conf.json` 中的桌面开发和构建钩子会调用 Bun。

## 从源码运行

```bash
# 安装前端依赖
bun install

# 启动 Tauri 桌面开发环境
bun run tauri dev
```

启动后，应用会读取以下本地数据：

- 默认配置：`~/.omp/agent/config.yml`
- 如果已存在，也会识别：`~/.omp/agent/config.yaml`
- Agent 定义：`~/.omp/agent/agents/*.md`
- 模型目录回退缓存：`~/.omp/agent/models.yml` 或 `~/.omp/agent/models.yaml`

首次使用前请先确认 OMP 能正常运行：

```bash
omp models --json
```

如果实时模型发现失败，应用会显示 warning，并尝试使用本地模型目录或当前配置中的 selector。

## 使用流程

1. 安装 OMP，并确认 `omp models --json` 可以执行。
2. 启动 `bun run tauri dev`。
3. 在全部内建模型 Role 卡片中选择模型和 thinking effort。
4. 按需新增 custom provider，填写 API key、Base URL 和总 Type；点击获取完整 model list 后，应用会叠加可用的 thinking effort 元数据，并在每个 model 行单独覆盖 Type；点击 **保存 Provider** 后立即写入 `models.yml`。
5. 配置全局 thinking、Advisor、Prewalk 与 fallback chain。
6. 按需新增自定义 Role 或自定义 Agent，并调整 provider 状态。
7. 确认没有非法选择后点击 **Apply**，将 Role、路由策略和 Agent 草稿写入 `config.yml` 与 Agent 文件；Custom Provider 不参与此操作。

模型 Role、Agent 和路由策略编辑先保存在应用草稿中，不会在点击 **Apply** 前写入磁盘。Custom Provider 是例外：点击 **保存 Provider** 或确认删除后，会独立写入 `models.yml`。

- Custom provider ID 只能包含字母、数字、`-`、`_`、`.`；Base URL 必须是 `http://` 或 `https://`；总 Type 和每个 model 的最终 Type 必须是 OMP 支持的 API；必须成功获取 model list，并至少勾选一个 model。
- Model list 请求：Base URL 已带 `/vN` 时请求 `<baseUrl>/models`，否则依次尝试 `<baseUrl>/v1/models` 与 `<baseUrl>/v3/models`；请求发送 `Authorization: Bearer <apiKey>`，`anthropic-messages` 额外发送 `x-api-key` 和 `anthropic-version: 2023-06-01`。普通 model list 是可用模型的权威来源；若 endpoint 接受 `?client_version=0.144.0`，应用会将 `slug`、`supported_reasoning_levels[].effort` 和 `default_reasoning_level` 作为可选能力 overlay，忽略 `visibility: hide` 的能力及 OMP/Pi 当前不支持的 `ultra`。
- 自定义 Role 名称不能覆盖 OMP 系统 Role，且必须以小写字母开头，只能包含小写字母、数字、`-`、`_`。
- 自定义 Role 必须选择模型。
- Agent Markdown 必须包含 `name`、`description`、`model` 三个 frontmatter 字段，并包含非空正文。
- shadowed provider 不能继续被模型 Role 使用。
- 已配置但不在当前 `omp models --json` 返回列表中的直接模型 selector 会被标记为非法。
- Fallback target 必须唯一且至少有一个候选；候选不能留空、重复、回退到自身或继续使用 shadowed provider。
- 空白的全局策略控件表示继承 OMP 默认，不会强行写入一个覆盖值。

### Agent Markdown 示例

```markdown
---
name: release-coordinator
description: Release verification agent.
model: "@slow"
---

Run the release verification workflow and report concrete findings.
```

## 配置写入与数据边界

- 后端只更新受管理的 YAML 映射：`config.yml` 中的 `modelRoles`、`task.agentModelOverrides`、thinking/Advisor/Prewalk 设置和 `retry.fallbackChains`；`disabledProviders` 不属于本工具的 OMP shadow 状态，Apply 会保留其现有值；其余未识别的 config 键也会保留。
- `models.yml` 中只 upsert 当前 custom provider 明确管理的 `baseUrl`、`apiKey` 和勾选的 `models`；每个保存的 model 都带自己的 `api`，托管 provider 不再写 provider-level `api`。已有 model 元数据及 provider headers 等未管理字段保留。只有用户明确删除的 provider 才会移除；override-only/discovery provider 不会被编辑器接管。
- Provider shadow 状态是 WebView 本地草稿：只针对当前 `omp models --json` 返回的 provider 生效，不从 `disabledProviders` 载入，也不会写回该字段；预设可以保存这份本地 shadow 状态。
- Custom provider 的 API key 只会由 **保存 Provider** 写入本机 `models.yml`；读取已有配置时只返回“已配置”状态，不把密钥返回到 WebView。
- YAML 会经过 `serde_yaml` 重新序列化，因此格式和注释可能被重写；如需保留原始格式，请在写入前备份文件。
- 配置、`models.yml` 和 Agent 文件先写入同目录隐藏临时文件，再通过 rename 原子替换，避免留下半写入文件。
- 应用界面显示路径时会将用户主目录压缩为 `~/`。
- 本工具的范围不包括 Skill、OMP OAuth 登录流程、运行权限或其他非模型路由配置。

## 开发命令

```bash
# 前端开发服务器（仅 Vite）
bun run dev

# 前端类型检查与生产构建
bun run build

# Rust 后端单元测试
cargo test --manifest-path src-tauri/Cargo.toml

# 构建桌面发布包
bun run tauri build
```


## 项目结构

```text
ROLE-GUIDE.md  # Role、Agent、Advisor 与运行时控制语义

src/
├── App.tsx       # 主界面、草稿状态、验证、预设和主题
├── api.ts        # Tauri IPC 调用
├── types.ts      # 前后端数据契约
└── styles.css    # 单一 vanilla CSS 设计系统

src-tauri/
├── src/lib.rs    # 配置发现、模型元数据、路由策略、YAML 与 Agent 持久化
├── src/main.rs   # Tauri 二进制入口
├── capabilities/ # Tauri 权限配置
└── tauri.conf.json
```

工程约束、目录职责和实现细节以仓库根目录的 [`AGENTS.md`](AGENTS.md) 为工程协作 SSOT；README 只维护面向使用者和贡献者的入口信息。

## 贡献

欢迎通过 Issue 或 Pull Request 报告问题、改进文档和提交代码。提交前请：

1. 阅读 [`AGENTS.md`](AGENTS.md)。
2. 保持 Role、内建 Agent 覆盖和自定义 Agent 的边界清晰。
3. 如果修改 IPC 数据契约，同时检查 `src/types.ts`、`src/api.ts` 和 `src-tauri/src/lib.rs`。
4. 运行前端构建和 Rust 测试，并在 PR 中说明验证命令及结果。
5. 不要提交个人 OMP 配置、provider 凭据或生成的 `dist/`、`target/` 文件。

## License

本项目采用 [MIT License](LICENSE)。完整许可条款见仓库根目录的 [`LICENSE`](LICENSE) 文件。
