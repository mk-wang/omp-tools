import { useEffect, useMemo, useState } from "react";
import { loadSnapshot, saveSnapshot } from "./api";
import type { ConfigSnapshot, ModelOption, SaveRequest } from "./types";
import "./styles.css";

type SlotKey =
  | "default"
  | "smol"
  | "slow"
  | "vision"
  | "advisor"
  | "reviewer"
  | "securityReviewer";
type SlotValues = Record<SlotKey, string>;
type Theme = "light" | "dark";

type SlotDefinition = {
  key: SlotKey;
  title: string;
  eyebrow: string;
  path: string;
  description: string;
  color: string;
};

type CustomRole = {
  role: string;
  value: string;
};

type CustomRoleEditor = CustomRole;

type CustomAgent = {
  fileName: string;
  path: string;
  name: string;
  description: string;
  model: string;
  agentMarkdown: string;
  dirty: boolean;
};

type AgentEditor = CustomAgent & {
  mode: "create" | "edit";
  originalName: string;
};

type Preset = {
  id: string;
  name: string;
  values: SlotValues;
  customRoleValues: Record<string, string>;
  reviewerFollowsSlow: boolean;
  securityReviewerFollowsSlow: boolean;
  updatedAt: string;
};

const SLOT_DEFINITIONS: SlotDefinition[] = [
  {
    key: "default",
    title: "Default",
    eyebrow: "主会话",
    path: "modelRoles.default",
    description: "日常对话、实现任务和默认模型。",
    color: "mint"
  },
  {
    key: "smol",
    title: "Small",
    eyebrow: "OMP smol",
    path: "modelRoles.smol",
    description: "快速研究、轻量任务和低成本后台工作。",
    color: "amber"
  },
  {
    key: "slow",
    title: "Slow",
    eyebrow: "深度推理",
    path: "modelRoles.slow",
    description: "复杂定位、回归检查和高质量只读审查。",
    color: "blue"
  },
  {
    key: "vision",
    title: "Vision",
    eyebrow: "视觉输入",
    path: "modelRoles.vision",
    description: "图片、截图和视觉输入任务。",
    color: "indigo"
  },
  {
    key: "advisor",
    title: "Advisor",
    eyebrow: "旁路审查",
    path: "modelRoles.advisor",
    description: "持续观察主会话并提出 concern 或 blocker。",
    color: "violet"
  },
  {
    key: "reviewer",
    title: "Reviewer",
    eyebrow: "Agent · 代码审查",
    path: "task.agentModelOverrides.reviewer",
    description: "内建 reviewer Agent 的模型覆盖；默认引用 @slow。",
    color: "rose"
  },
  {
    key: "securityReviewer",
    title: "Security Reviewer",
    eyebrow: "Agent · 安全审查",
    path: "task.agentModelOverrides.security-reviewer",
    description: "security-reviewer Agent 的模型覆盖；不是模型 Role。",
    color: "red"
  }
];
const AGENT_OVERRIDE_KEYS = new Set<SlotKey>(["reviewer", "securityReviewer"]);
const MODEL_ROLE_DEFINITIONS = SLOT_DEFINITIONS.filter(definition => !AGENT_OVERRIDE_KEYS.has(definition.key));
const BUILTIN_AGENT_OVERRIDE_DEFINITIONS = SLOT_DEFINITIONS.filter(
  definition => AGENT_OVERRIDE_KEYS.has(definition.key)
);


const EFFORT_LABELS: Record<string, string> = {
  auto: "Auto",
  minimal: "Minimal",
  low: "Low",
  medium: "Medium",
  high: "High",
  xhigh: "XHigh",
  max: "Max",
  off: "Off"
};

const KNOWN_EFFORTS = ["minimal", "low", "medium", "high", "xhigh", "max", "off"];
const PRESETS_STORAGE_KEY = "omp-model-studio.presets";
const THEME_STORAGE_KEY = "omp-model-studio.theme";
const SYSTEM_MODEL_ROLES = new Set([
  "default",
  "smol",
  "slow",
  "vision",
  "plan",
  "designer",
  "commit",
  "tiny",
  "task",
  "advisor"
]);
const CUSTOM_ROLE_PATTERN = /^[a-z][a-z0-9_-]*$/;

const EMPTY_VALUES: SlotValues = {
  default: "",
  smol: "",
  slow: "",
  vision: "",
  advisor: "",
  reviewer: "",
  securityReviewer: ""
};

function readableError(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}

function splitSelector(value: string): { base: string; effort: string; alias: boolean } {
  const trimmed = value.trim();
  if (trimmed.startsWith("@")) return { base: trimmed, effort: "auto", alias: true };
  const match = trimmed.match(/^(.*):(minimal|low|medium|high|xhigh|max|off)$/);
  if (!match) return { base: trimmed, effort: "auto", alias: false };
  return { base: match[1], effort: match[2], alias: false };
}

function withEffort(base: string, effort: string): string {
  if (!base) return "";
  return effort === "auto" ? base : `${base}:${effort}`;
}

function providerForSelector(value: string): string {
  const base = splitSelector(value).base;
  return base.includes("/") ? base.split("/")[0] : "";
}

function createPresetId(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) return crypto.randomUUID();
  return `preset-${Date.now()}`;
}

function recordOfStrings(value: unknown): Record<string, string> {
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};
  return Object.fromEntries(
    Object.entries(value).filter((entry): entry is [string, string] => typeof entry[1] === "string")
  );
}

function parsePreset(value: unknown): Preset | null {
  if (!value || typeof value !== "object") return null;
  const record = value as Record<string, unknown>;
  if (typeof record.id !== "string" || typeof record.name !== "string") return null;
  const storedValues = recordOfStrings(record.values);
  const customRoleValues = recordOfStrings(record.customRoleValues);
  if (!Object.hasOwn(customRoleValues, "tester") && storedValues.tester) {
    customRoleValues.tester = storedValues.tester;
  }
  return {
    id: record.id,
    name: record.name,
    values: {
      default: storedValues.default ?? "",
      smol: storedValues.smol ?? "",
      slow: storedValues.slow ?? "",
      vision: storedValues.vision ?? "",
      advisor: storedValues.advisor ?? "",
      reviewer: storedValues.reviewer ?? "",
      securityReviewer: storedValues.securityReviewer ?? ""
    },
    customRoleValues,
    reviewerFollowsSlow: record.reviewerFollowsSlow !== false,
    securityReviewerFollowsSlow: record.securityReviewerFollowsSlow !== false,
    updatedAt: typeof record.updatedAt === "string" ? record.updatedAt : new Date(0).toISOString()
  };
}

function readPresets(): Preset[] {
  if (typeof window === "undefined") return [];
  try {
    const parsed: unknown = JSON.parse(window.localStorage.getItem(PRESETS_STORAGE_KEY) ?? "[]");
    return Array.isArray(parsed)
      ? parsed.map(parsePreset).filter((preset): preset is Preset => preset !== null)
      : [];
  } catch {
    return [];
  }
}

function createAgentMarkdown(name: string, model: string): string {
  if (!CUSTOM_ROLE_PATTERN.test(name) || !model.trim()) return "";
  return `---\nname: ${name}\ndescription: 执行分配给 ${name} 的独立任务。\nmodel: "${model}"\n---\n\n执行父任务分配的工作，并遵循仓库中的指令、工具边界与输出契约。\n`;
}

function frontmatterField(markdown: string, field: string): string {
  const block = markdown.match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n/);
  if (!block) return "";
  const escaped = field.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const direct = block[1].match(new RegExp(`^${escaped}:\\s*(.+?)\\s*$`, "m"))?.[1];
  if (direct) return direct.replace(/^["']|["']$/g, "").trim();
  const sequence = block[1].match(new RegExp(`^${escaped}:\\s*\\r?\\n\\s*-\\s*(.+?)\\s*$`, "m"))?.[1];
  return sequence?.replace(/^["']|["']$/g, "").trim() ?? "";
}

function replaceFrontmatterField(markdown: string, field: string, value: string): string {
  const escaped = field.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const pattern = new RegExp(`^${escaped}:[^\\n]*(?:\\r?\\n[ \\t]+-[^\\n]*)*`, "m");
  return pattern.test(markdown) ? markdown.replace(pattern, `${field}: "${value}"`) : markdown;
}

function validateAgentMarkdown(markdown: string): string {
  if (!/^---\r?\n/.test(markdown) || !/\r?\n---\r?\n/.test(markdown)) return "Markdown 必须包含 YAML frontmatter。";
  if (!frontmatterField(markdown, "name")) return "frontmatter 缺少 name。";
  if (!frontmatterField(markdown, "description")) return "frontmatter 缺少 description。";
  if (!frontmatterField(markdown, "model")) return "frontmatter 缺少有效 model。";
  const body = markdown.split(/\r?\n---\r?\n/, 2)[1] ?? "";
  if (!body.trim()) return "Markdown 正文不能为空。";
  return "";
}

function readTheme(): Theme {
  if (typeof window === "undefined") return "light";
  return window.localStorage.getItem(THEME_STORAGE_KEY) === "dark" ? "dark" : "light";
}

function ModelPicker({
  value,
  models,
  filter,
  disabledProviders,
  onChange,
  disabled = false
}: {
  value: string;
  models: ModelOption[];
  filter: string;
  disabledProviders: string[];
  onChange: (value: string) => void;
  disabled?: boolean;
}) {
  const parsed = splitSelector(value);
  const currentProvider = providerForSelector(value);
  const disabledProviderSet = new Set(disabledProviders);
  const providers = useMemo(() => {
    const names = new Set(
      models
        .filter(model => !disabledProviderSet.has(model.provider) || model.provider === currentProvider)
        .map(model => model.provider)
    );
    if (currentProvider) names.add(currentProvider);
    return [...names].sort((left, right) => left.localeCompare(right));
  }, [currentProvider, models, disabledProviders]);

  const currentModel = models.find(model => `${model.provider}/${model.id}` === parsed.base);
  const selectedProvider = currentProvider || providers[0] || "";
  const providerModels = models.filter(
    model => model.provider === selectedProvider && (!disabledProviderSet.has(selectedProvider) || selectedProvider === currentProvider)
  );
  const normalizedFilter = filter.trim().toLowerCase();
  const filteredModels = providerModels.filter(model => {
    if (!normalizedFilter) return true;
    return `${model.id} ${model.name} ${model.provider}`.toLowerCase().includes(normalizedFilter);
  });
  const availableModels = filteredModels.length > 0 ? filteredModels : providerModels;
  const selectedModel = currentModel ?? availableModels[0];
  const selectedBase = parsed.base || selectedModel?.selector || "";
  const effortOptions = useMemo(() => {
    const supported = currentModel
      ? currentModel.thinking.length
        ? currentModel.thinking
        : currentModel.reasoning
          ? KNOWN_EFFORTS
          : ["off"]
      : KNOWN_EFFORTS;
    const options = ["auto", ...supported];
    if (parsed.effort !== "auto" && !options.includes(parsed.effort)) options.push(parsed.effort);
    return [...new Set(options)];
  }, [currentModel, parsed.effort]);

  if (parsed.alias) {
    return (
      <div className="alias-picker">
        <span className="alias-badge">ROLE ALIAS</span>
        <input
          value={value}
          disabled={disabled}
          onChange={event => onChange(event.target.value)}
          aria-label="Role alias"
        />
      </div>
    );
  }

  const changeProvider = (provider: string) => {
    const next = models.find(model => model.provider === provider);
    onChange(withEffort(next?.selector ?? `${provider}/`, parsed.effort));
  };

  return (
    <div className="picker-grid">
      <label className="field-group">
        <span>Provider</span>
        <select
          value={selectedProvider}
          disabled={disabled || providers.length === 0}
          onChange={event => changeProvider(event.target.value)}
        >
          {providers.length === 0 && <option value="">No providers</option>}
          {providers.map(provider => (
            <option value={provider} key={provider}>
              {provider}
            </option>
          ))}
        </select>
      </label>
      <label className="field-group model-field">
        <span>Model</span>
        <select
          value={selectedBase}
          disabled={disabled || availableModels.length === 0}
          onChange={event => onChange(withEffort(event.target.value, parsed.effort))}
        >
          {availableModels.length === 0 && <option value="">No models</option>}
          {selectedBase && !availableModels.some(model => model.selector === selectedBase) && (
            <option value={selectedBase}>{selectedBase.split("/").slice(1).join("/")}</option>
          )}
          {availableModels.map(model => (
            <option value={model.selector} key={model.selector}>
              {model.id}
            </option>
          ))}
        </select>
      </label>
      <label className="field-group effort-field">
        <span>Thinking</span>
        <select
          value={parsed.effort}
          disabled={disabled || !selectedBase}
          onChange={event => onChange(withEffort(selectedBase, event.target.value))}
        >
          {effortOptions.map(effort => (
            <option value={effort} key={effort}>
              {EFFORT_LABELS[effort] ?? effort}
            </option>
          ))}
        </select>
      </label>
    </div>
  );
}

function SlotCard({
  definition,
  value,
  models,
  filter,
  disabledProviders,
  reviewerFollowsSlow,
  securityReviewerFollowsSlow,
  slowValue,
  onChange,
  onFollowChange
}: {
  definition: SlotDefinition;
  value: string;
  models: ModelOption[];
  filter: string;
  disabledProviders: string[];
  reviewerFollowsSlow: boolean;
  securityReviewerFollowsSlow: boolean;
  slowValue: string;
  onChange: (value: string) => void;
  onFollowChange: (key: "reviewer" | "securityReviewer", follow: boolean) => void;
}) {
  const isReviewer = definition.key === "reviewer";
  const isSecurityReviewer = definition.key === "securityReviewer";
  const followsSlow = isReviewer ? reviewerFollowsSlow : isSecurityReviewer ? securityReviewerFollowsSlow : false;
  const effectiveValue = followsSlow ? slowValue : value;
  const effectiveProvider = providerForSelector(effectiveValue);
  const isInvalid = Boolean(effectiveProvider && disabledProviders.includes(effectiveProvider));
  const followKey = isSecurityReviewer ? "securityReviewer" : "reviewer";
  const isFollowable = isReviewer || isSecurityReviewer;

  return (
    <article className={`slot-card ${definition.color}${isInvalid ? " invalid" : ""}`}>
      <div className="slot-header">
        <div className="slot-heading">
          <span className="slot-dot" />
          <div>
            <div className="slot-title-line">
              <h2>{definition.title}</h2>
              <span className="eyebrow">{definition.eyebrow}</span>
              {definition.path && !AGENT_OVERRIDE_KEYS.has(definition.key) && (
                <code className="config-path">{definition.path}</code>
              )}
            </div>
            <p>{definition.description}</p>
          </div>
        </div>
        <div className="slot-header-meta">
          {isInvalid && <span className="invalid-badge">非法</span>}
        </div>
      </div>

      {isFollowable && (
        <div className="reviewer-mode">
          <label className="switch-row">
            <input
              type="checkbox"
              checked={followsSlow}
              onChange={event => onFollowChange(followKey, event.target.checked)}
            />
            <span className="switch" aria-hidden="true" />
            <span>跟随 Slow 角色</span>
          </label>
        </div>
      )}

      {followsSlow && (
        <code className="inherited-model">{effectiveValue || "尚未选择"}</code>
      )}
      {(!isFollowable || !followsSlow) && (
        <ModelPicker
          value={value}
          models={models}
          filter={filter}
          disabledProviders={disabledProviders}
          onChange={onChange}
        />
      )}

    </article>
  );
}

function CustomRoleCard({
  customRole,
  models,
  filter,
  disabledProviders,
  onChange
}: {
  customRole: CustomRole;
  models: ModelOption[];
  filter: string;
  disabledProviders: string[];
  onChange: (value: string) => void;
}) {
  const provider = providerForSelector(customRole.value);
  const isInvalid = Boolean(provider && disabledProviders.includes(provider));
  return (
    <article className={`slot-card cyan custom-role-card${isInvalid ? " invalid" : ""}`}>
      <div className="slot-header">
        <div className="slot-heading">
          <span className="slot-dot" />
          <div>
            <div className="slot-title-line">
              <h2>{customRole.role}</h2>
              <span className="eyebrow">模型 Role</span>
              <code className="config-path">modelRoles.{customRole.role}</code>
            </div>
            <p>只定义模型与 thinking 路由；不创建或绑定 Agent。</p>
          </div>
        </div>
        {isInvalid && <span className="invalid-badge">非法</span>}
      </div>
      <ModelPicker
        value={customRole.value}
        models={models}
        filter={filter}
        disabledProviders={disabledProviders}
        onChange={onChange}
      />
    </article>
  );
}

function CustomAgentCard({
  agent,
  onEdit
}: {
  agent: CustomAgent;
  onEdit: () => void;
}) {
  const validationError = validateAgentMarkdown(agent.agentMarkdown);
  return (
    <article className={`agent-card${validationError ? " invalid" : ""}`}>
      <div className="agent-card-main">
        <div className="slot-title-line">
          <h2>{agent.name}</h2>
          <span className="eyebrow">Agent</span>
          {agent.dirty && <span className="draft-badge">草稿</span>}
        </div>
        <p>{agent.description}</p>
        <code className="agent-path">{agent.path}</code>
      </div>
      <div className="agent-card-actions">
        <span>MODEL / ROLE</span>
        <code>{agent.model}</code>
        {validationError && <span className="invalid-badge">非法</span>}
        <button className="button compact" onClick={onEdit}>编辑 Markdown</button>
      </div>
    </article>
  );
}

function CustomRoleModal({
  editor,
  models,
  filter,
  disabledProviders,
  existingRoles,
  onChange,
  onCancel,
  onSave
}: {
  editor: CustomRoleEditor;
  models: ModelOption[];
  filter: string;
  disabledProviders: string[];
  existingRoles: string[];
  onChange: (editor: CustomRoleEditor) => void;
  onCancel: () => void;
  onSave: () => void;
}) {
  const roleError = !editor.role
    ? "请输入 Role key。"
    : !CUSTOM_ROLE_PATTERN.test(editor.role)
      ? "Role key 需以小写字母开头，只能包含小写字母、数字、-、_。"
      : SYSTEM_MODEL_ROLES.has(editor.role)
        ? "该名称属于系统 Role。"
        : existingRoles.includes(editor.role)
          ? "该自定义 Role 已存在。"
          : "";
  const error = roleError || (editor.value.trim() ? "" : "请选择模型。");

  return (
    <div
      className="modal-backdrop"
      role="presentation"
      onMouseDown={event => {
        if (event.target === event.currentTarget) onCancel();
      }}
    >
      <section className="role-modal compact-modal" role="dialog" aria-modal="true" aria-labelledby="custom-role-title">
        <div className="role-modal-header">
          <div>
            <span className="section-kicker">CUSTOM MODEL ROLE</span>
            <h2 id="custom-role-title">新增自定义 Role</h2>
          </div>
          <button className="modal-close" onClick={onCancel} aria-label="关闭">×</button>
        </div>
        <div className="role-modal-body">
          <label className="modal-field">
            <span>Role key</span>
            <input
              value={editor.role}
              onChange={event => onChange({ ...editor, role: event.target.value.toLowerCase() })}
              placeholder="例如 release-budget"
              autoFocus
            />
          </label>
          <div className="modal-field">
            <span>Role model</span>
            <ModelPicker
              value={editor.value}
              models={models}
              filter={filter}
              disabledProviders={disabledProviders}
              onChange={value => onChange({ ...editor, value })}
            />
          </div>
          <div className={`modal-validation${error ? " is-error" : ""}`}>
            {error || `将写入 modelRoles.${editor.role}；不会创建 Agent Markdown。`}
          </div>
        </div>
        <div className="role-modal-footer">
          <span>先加入模型草稿；Apply 才写入 config.yml。</span>
          <div>
            <button className="button quiet" onClick={onCancel}>取消</button>
            <button className="button" onClick={onSave} disabled={Boolean(error)}>加入草稿</button>
          </div>
        </div>
      </section>
    </div>
  );
}

function AgentModal({
  editor,
  roles,
  existingAgents,
  onChange,
  onCancel,
  onSave
}: {
  editor: AgentEditor;
  roles: string[];
  existingAgents: string[];
  onChange: (editor: AgentEditor) => void;
  onCancel: () => void;
  onSave: () => void;
}) {
  const duplicateName = editor.mode === "create" && existingAgents.includes(editor.name);
  const nameError = !editor.name
    ? "请输入 Agent name。"
    : !CUSTOM_ROLE_PATTERN.test(editor.name)
      ? "Agent name 需以小写字母开头，只能包含小写字母、数字、-、_。"
      : duplicateName
        ? "该 Agent 已存在。"
        : "";
  const markdownName = frontmatterField(editor.agentMarkdown, "name");
  const identityError = markdownName && markdownName !== editor.name
    ? `frontmatter name 必须保持为 “${editor.name}”。`
    : "";
  const error = nameError || validateAgentMarkdown(editor.agentMarkdown) || identityError;
  const updateName = (name: string) => {
    const normalized = name.toLowerCase();
    const markdown = editor.agentMarkdown
      ? replaceFrontmatterField(editor.agentMarkdown, "name", normalized)
      : createAgentMarkdown(normalized, editor.model);
    onChange({
      ...editor,
      name: normalized,
      fileName: normalized ? `${normalized}.md` : "",
      path: normalized ? `~/.omp/agent/agents/${normalized}.md` : "",
      agentMarkdown: markdown
    });
  };
  const updateModel = (model: string) => {
    const markdown = editor.agentMarkdown
      ? replaceFrontmatterField(editor.agentMarkdown, "model", model)
      : createAgentMarkdown(editor.name, model);
    onChange({ ...editor, model, agentMarkdown: markdown });
  };

  return (
    <div
      className="modal-backdrop"
      role="presentation"
      onMouseDown={event => {
        if (event.target === event.currentTarget) onCancel();
      }}
    >
      <section className="role-modal" role="dialog" aria-modal="true" aria-labelledby="agent-title">
        <div className="role-modal-header">
          <div>
            <span className="section-kicker">CUSTOM AGENT</span>
            <h2 id="agent-title">{editor.mode === "create" ? "新增自定义 Agent" : `编辑 ${editor.name}`}</h2>
          </div>
          <button className="modal-close" onClick={onCancel} aria-label="关闭">×</button>
        </div>
        <div className="role-modal-body">
          <label className="modal-field">
            <span>Agent name</span>
            {editor.mode === "create" ? (
              <input
                value={editor.name}
                onChange={event => updateName(event.target.value)}
                placeholder="例如 release-coordinator"
                autoFocus
              />
            ) : (
              <code className="locked-role">{editor.name}</code>
            )}
          </label>
          <label className="modal-field">
            <span>Model Role</span>
            <select value={editor.model} onChange={event => updateModel(event.target.value)}>
              {!roles.includes(editor.model) && <option value={editor.model}>{editor.model}</option>}
              {roles.map(role => <option value={role} key={role}>{role}</option>)}
            </select>
          </label>
          <div className="markdown-heading">
            <div>
              <span>Agent Markdown</span>
              <code>{editor.path || "~/.omp/agent/agents/<agent>.md"}</code>
            </div>
            {editor.name && editor.model && (
              <button
                className="button compact"
                onClick={() => onChange({ ...editor, agentMarkdown: createAgentMarkdown(editor.name, editor.model) })}
              >
                重置模板
              </button>
            )}
          </div>
          <textarea
            className="markdown-editor"
            value={editor.agentMarkdown}
            onChange={event => {
              const agentMarkdown = event.target.value;
              onChange({
                ...editor,
                agentMarkdown,
                description: frontmatterField(agentMarkdown, "description") || editor.description,
                model: frontmatterField(agentMarkdown, "model") || editor.model
              });
            }}
            placeholder="输入包含 name、description、model frontmatter 与正文的 Agent Markdown"
            spellCheck={false}
          />
          <div className={`modal-validation${error ? " is-error" : ""}`}>
            {error || `${editor.name} 将使用 ${editor.model}；Agent 与模型 Role 分别命名。`}
          </div>
        </div>
        <div className="role-modal-footer">
          <span>先加入草稿；Apply 才写入 Agent Markdown。</span>
          <div>
            <button className="button quiet" onClick={onCancel}>取消</button>
            <button className="button" onClick={onSave} disabled={Boolean(error)}>
              {editor.mode === "create" ? "加入草稿" : "更新草稿"}
            </button>
          </div>
        </div>
      </section>
    </div>
  );
}

export default function App() {
  const [snapshot, setSnapshot] = useState<ConfigSnapshot | null>(null);
  const [values, setValues] = useState<SlotValues>(EMPTY_VALUES);
  const [customRoles, setCustomRoles] = useState<CustomRole[]>([]);
  const [roleEditor, setRoleEditor] = useState<CustomRoleEditor | null>(null);
  const [customAgents, setCustomAgents] = useState<CustomAgent[]>([]);
  const [agentEditor, setAgentEditor] = useState<AgentEditor | null>(null);
  const [reviewerFollowsSlow, setReviewerFollowsSlow] = useState(true);
  const [securityReviewerFollowsSlow, setSecurityReviewerFollowsSlow] = useState(true);
  const [disabledProviders, setDisabledProviders] = useState<string[]>([]);
  const [filter, setFilter] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState("");
  const [theme, setTheme] = useState<Theme>(readTheme);
  const [presets, setPresets] = useState<Preset[]>(readPresets);
  const [selectedPresetId, setSelectedPresetId] = useState("");
  const [presetName, setPresetName] = useState("");
  const [presetMessage, setPresetMessage] = useState("");
  const [pendingDeletePresetId, setPendingDeletePresetId] = useState("");

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    try {
      window.localStorage.setItem(THEME_STORAGE_KEY, theme);
    } catch {
      // Theme still works for the current session when storage is unavailable.
    }
  }, [theme]);

  const applySnapshot = (next: ConfigSnapshot) => {
    const rawReviewer = next.agentModelOverrides.reviewer ?? "";
    const rawSecurityReviewer = next.agentModelOverrides["security-reviewer"] ?? "";
    const followsSlow = rawReviewer === "" || rawReviewer === "@slow";
    const securityFollowsSlow = rawSecurityReviewer === "" || rawSecurityReviewer === "@slow";
    setReviewerFollowsSlow(followsSlow);
    setSecurityReviewerFollowsSlow(securityFollowsSlow);
    setValues({
      default: next.modelRoles.default ?? "",
      smol: next.modelRoles.smol ?? "",
      slow: next.modelRoles.slow ?? "",
      vision: next.modelRoles.vision ?? "",
      advisor: next.modelRoles.advisor ?? "",
      reviewer: followsSlow ? next.modelRoles.slow ?? "" : rawReviewer,
      securityReviewer: securityFollowsSlow ? next.modelRoles.slow ?? "" : rawSecurityReviewer
    });
    const nextCustomRoles = Object.entries(next.modelRoles)
      .filter(([role]) => !SYSTEM_MODEL_ROLES.has(role))
      .map(([role, value]) => ({ role, value }))
      .sort((left, right) => left.role.localeCompare(right.role));
    setCustomRoles(nextCustomRoles);
    setCustomAgents(next.agentFiles.map(file => ({
      fileName: file.fileName,
      path: file.path,
      name: file.name,
      description: file.description,
      model: file.model,
      agentMarkdown: file.content,
      dirty: false
    })));
    setRoleEditor(null);
    setAgentEditor(null);
    setDisabledProviders([...next.disabledProviders]);
    setSnapshot(next);
    setSaved(false);
  };

  const refresh = async () => {
    setLoading(true);
    setError("");
    try {
      applySnapshot(await loadSnapshot());
    } catch (loadError) {
      setError(readableError(loadError));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void refresh();
  }, []);

  const providerNames = useMemo(() => {
    const names = new Set([...(snapshot?.models.map(model => model.provider) ?? []), ...disabledProviders]);
    return [...names].sort((left, right) => left.localeCompare(right));
  }, [disabledProviders, snapshot]);

  const selectedModelSlots = useMemo(() => {
    const fixedAssignments = SLOT_DEFINITIONS.map(definition => {
      if (definition.key === "reviewer" && reviewerFollowsSlow) {
        return { label: definition.title, value: values.slow, base: splitSelector(values.slow).base };
      }
      if (definition.key === "securityReviewer" && securityReviewerFollowsSlow) {
        return { label: definition.title, value: values.slow, base: splitSelector(values.slow).base };
      }
      const value = values[definition.key];
      return { label: definition.title, value, base: splitSelector(value).base };
    });
    const customAssignments = customRoles.map(customRole => ({
      label: customRole.role,
      value: customRole.value,
      base: splitSelector(customRole.value).base
    }));
    return [...fixedAssignments, ...customAssignments].filter(item => item.base && !item.base.startsWith("@"));
  }, [customRoles, reviewerFollowsSlow, securityReviewerFollowsSlow, values]);
  const invalidSelections = selectedModelSlots.filter(item => disabledProviders.includes(providerForSelector(item.value)));
  const hasInvalidSelections = invalidSelections.length > 0;
  const invalidCustomRoles = customRoles.filter(customRole => !customRole.value.trim());
  const invalidAgents = customAgents.filter(agent => Boolean(validateAgentMarkdown(agent.agentMarkdown)));
  const hasApplyErrors = hasInvalidSelections || invalidCustomRoles.length > 0 || invalidAgents.length > 0;


  const catalogModels = useMemo(() => {
    const normalizedFilter = filter.trim().toLowerCase();
    return (snapshot?.models ?? []).filter(model => {
      const matches = !normalizedFilter || `${model.provider} ${model.id} ${model.name}`.toLowerCase().includes(normalizedFilter);
      return matches && !disabledProviders.includes(model.provider);
    });
  }, [filter, disabledProviders, snapshot]);
  const agentRoleOptions = useMemo(() => {
    const roles = new Set([
      ...SYSTEM_MODEL_ROLES,
      ...Object.keys(snapshot?.modelRoles ?? {}),
      ...customRoles.map(customRole => customRole.role)
    ]);
    return [...roles].sort((left, right) => left.localeCompare(right)).map(role => `@${role}`);
  }, [customRoles, snapshot]);

  const emptyRoleSlots =
    MODEL_ROLE_DEFINITIONS.filter(definition => !values[definition.key]).length
    + customRoles.filter(customRole => !customRole.value).length;

  const updateValue = (key: SlotKey, value: string) => {
    setSaved(false);
    setValues(current => ({ ...current, [key]: value }));
  };

  const updateCustomRoleValue = (role: string, value: string) => {
    setSaved(false);
    setCustomRoles(current =>
      current.map(customRole => customRole.role === role ? { ...customRole, value } : customRole)
    );
  };

  const startCustomRole = () => {
    const firstModel = catalogModels[0]?.selector ?? snapshot?.models[0]?.selector ?? "";
    setRoleEditor({ role: "", value: firstModel });
  };

  const saveCustomRoleDraft = () => {
    if (
      !roleEditor
      || !CUSTOM_ROLE_PATTERN.test(roleEditor.role)
      || SYSTEM_MODEL_ROLES.has(roleEditor.role)
      || !roleEditor.value.trim()
    ) return;
    setCustomRoles(current => {
      if (current.some(item => item.role === roleEditor.role)) return current;
      return [...current, roleEditor].sort((left, right) => left.role.localeCompare(right.role));
    });
    setRoleEditor(null);
    setSaved(false);
  };

  const startAgent = () => {
    const model = agentRoleOptions.includes("@slow") ? "@slow" : agentRoleOptions[0] ?? "@default";
    setAgentEditor({
      mode: "create",
      originalName: "",
      fileName: "",
      path: "",
      name: "",
      description: "",
      model,
      agentMarkdown: "",
      dirty: true
    });
  };

  const editAgent = (agent: CustomAgent) => {
    setAgentEditor({ ...agent, mode: "edit", originalName: agent.name });
  };

  const saveAgentDraft = () => {
    if (!agentEditor || validateAgentMarkdown(agentEditor.agentMarkdown)) return;
    const name = frontmatterField(agentEditor.agentMarkdown, "name");
    const description = frontmatterField(agentEditor.agentMarkdown, "description");
    const model = frontmatterField(agentEditor.agentMarkdown, "model");
    if (!name || name !== agentEditor.name || !description || !model) return;
    const agent: CustomAgent = {
      fileName: agentEditor.fileName,
      path: agentEditor.path,
      name,
      description,
      model,
      agentMarkdown: agentEditor.agentMarkdown,
      dirty: true
    };
    setCustomAgents(current => {
      if (agentEditor.mode === "create") {
        if (current.some(item => item.name === name || item.fileName === agent.fileName)) return current;
        return [...current, agent].sort((left, right) => left.name.localeCompare(right.name));
      }
      return current.map(item => item.name === agentEditor.originalName ? agent : item);
    });
    setAgentEditor(null);
    setSaved(false);
  };

  const updateReviewerMode = (key: "reviewer" | "securityReviewer", follow: boolean) => {
    setSaved(false);
    if (key === "reviewer") setReviewerFollowsSlow(follow);
    else setSecurityReviewerFollowsSlow(follow);
    if (follow) setValues(current => ({ ...current, [key]: current.slow }));
  };

  const toggleProvider = (provider: string, disable: boolean) => {
    setSaved(false);
    setDisabledProviders(current => {
      if (disable) return current.includes(provider) ? current : [...current, provider];
      return current.filter(item => item !== provider);
    });
  };

  const persistPresetList = (next: Preset[]) => {
    setPresets(next);
    try {
      window.localStorage.setItem(PRESETS_STORAGE_KEY, JSON.stringify(next));
    } catch {
      setPresetMessage("预设仅保留在当前会话，浏览器存储不可用。");
    }
  };

  const loadPreset = (preset: Preset) => {
    setValues({ ...EMPTY_VALUES, ...preset.values });
    setReviewerFollowsSlow(preset.reviewerFollowsSlow);
    setSecurityReviewerFollowsSlow(preset.securityReviewerFollowsSlow);
    const existingRoles = new Set(customRoles.map(customRole => customRole.role));
    const skippedRoles = Object.keys(preset.customRoleValues).filter(role => !existingRoles.has(role));
    setCustomRoles(current =>
      current.map(customRole =>
        Object.hasOwn(preset.customRoleValues, customRole.role)
          ? { ...customRole, value: preset.customRoleValues[customRole.role] }
          : customRole
      )
    );
    setPendingDeletePresetId("");
    setSelectedPresetId(preset.id);
    setPresetName("");
    setSaved(false);
    setPresetMessage(
      skippedRoles.length
        ? `已载入：“${preset.name}”；当前没有这些自定义 Role：${skippedRoles.join("、")}。`
        : `已载入模型草稿：“${preset.name}”。`
    );
  };

  const savePresetOverride = () => {
    const selected = presets.find(preset => preset.id === selectedPresetId);
    if (!selected) {
      setPresetMessage("先选择一个预设。");
      return;
    }
    if (hasInvalidSelections) {
      setPresetMessage("存在非法选择，修复后才能保存。");
      return;
    }
    const customRoleValues = Object.fromEntries(customRoles.map(customRole => [customRole.role, customRole.value]));
    const nextPreset: Preset = {
      ...selected,
      values: { ...values },
      customRoleValues,
      reviewerFollowsSlow,
      securityReviewerFollowsSlow,
      updatedAt: new Date().toISOString()
    };
    persistPresetList(presets.map(preset => (preset.id === selected.id ? nextPreset : preset)));
    setPresetName("");
    setPresetMessage(`已覆盖：“${selected.name}”。`);
  };

  const savePresetAs = () => {
    if (hasInvalidSelections) {
      setPresetMessage("存在非法选择，修复后才能保存。");
      return;
    }
    const name = presetName.trim();
    if (!name) {
      setPresetMessage("请输入新预设名称。");
      return;
    }
    const customRoleValues = Object.fromEntries(customRoles.map(customRole => [customRole.role, customRole.value]));
    const nextPreset: Preset = {
      id: createPresetId(),
      name,
      values: { ...values },
      customRoleValues,
      reviewerFollowsSlow,
      securityReviewerFollowsSlow,
      updatedAt: new Date().toISOString()
    };
    persistPresetList([...presets, nextPreset]);
    setSelectedPresetId(nextPreset.id);
    setPresetName("");
    setPresetMessage(`已新建：“${nextPreset.name}”。`);
  };

  const deletePreset = () => {
    const selected = presets.find(preset => preset.id === selectedPresetId);
    if (!selected) return;
    if (pendingDeletePresetId !== selected.id) {
      setPendingDeletePresetId(selected.id);
      setPresetMessage(`再次点击“确认删除”以删除预设：“${selected.name}”。`);
      return;
    }
    persistPresetList(presets.filter(preset => preset.id !== selected.id));
    setPendingDeletePresetId("");
    setSelectedPresetId("");
    setPresetName("");
    setPresetMessage(`已删除：“${selected.name}”。`);
  };

  const request: SaveRequest = {
    smol: values.smol,
    default: values.default,
    slow: values.slow,
    vision: values.vision,
    advisor: values.advisor,
    customRoles: customRoles.map(customRole => ({
      role: customRole.role,
      selector: customRole.value
    })),
    agentFiles: customAgents
      .filter(agent => agent.dirty)
      .map(agent => ({ fileName: agent.fileName, agentMarkdown: agent.agentMarkdown })),
    reviewer: reviewerFollowsSlow ? "" : values.reviewer,
    securityReviewer: securityReviewerFollowsSlow ? "" : values.securityReviewer,
    disabledProviders
  };
  const deletePending = Boolean(selectedPresetId) && pendingDeletePresetId === selectedPresetId;


  const applyConfig = async () => {
    if (hasApplyErrors) {
      setError(
        invalidCustomRoles.length
          ? "自定义模型 Role 缺少有效模型。"
          : invalidAgents.length
            ? "存在无效 Agent Markdown。请打开对应 Agent 修复。"
            : "存在非法选择：已禁用 provider 不能应用。请勾选 Use 或更换 Model。"
      );
      return;
    }

    setSaving(true);
    setError("");
    setSaved(false);
    try {
      const next = await saveSnapshot(request);
      applySnapshot(next);
      setSaved(true);
    } catch (saveError) {
      setError(readableError(saveError));
    } finally {
      setSaving(false);
    }
  };

  if (loading) {
    return (
      <div className="loading-screen">
        <div className="loader" />
        <span>正在读取 OMP 配置与模型目录…</span>
      </div>
    );
  }

  if (error && !snapshot) {
    return (
      <div className="error-screen">
        <div className="error-mark">!</div>
        <h1>无法打开 OMP 配置</h1>
        <p>{error}</p>
        <p className="muted">请确认已安装 omp，并通过 Tauri 启动此应用。</p>
        <button className="button primary" onClick={() => void refresh()}>
          重新读取
        </button>
      </div>
    );
  }

  return (
    <div className="app-shell">
      <header className="app-header">
        <div className="brand-lockup">
          <div className="brand-mark">OM</div>
          <div>
            <div className="brand-title">OMP Model Studio</div>
            <div className="brand-subtitle">模型角色与 Agent 策略的简化控制面</div>
          </div>
        </div>
        <div className="header-actions">
          <div className="config-status">
            <span className="status-dot" />
            {snapshot?.configPath ?? "~/.omp/agent/config.yml"}
          </div>
          <button className="button quiet" onClick={() => setTheme(theme === "light" ? "dark" : "light")}>
            {theme === "light" ? "☾ Dark" : "☀ Light"}
          </button>
          <button className="button quiet" onClick={() => void refresh()} disabled={saving}>
            重新读取
          </button>
          <button
            className="button"
            onClick={() => void applyConfig()}
            disabled={saving || hasApplyErrors}
            title={hasApplyErrors ? "存在非法模型或无效 Agent Markdown" : "应用模型与自定义 Agent 配置"}
          >
            {saving ? "Applying…" : saved ? "Applied" : hasApplyErrors ? "修复配置" : "Apply"}
          </button>
        </div>
      </header>

      <div className="content-grid">
        <aside className="sidebar">
          <section className="intro-block">
            <span className="section-kicker">MODEL STUDIO</span>
            <h1>配置模型 Role，也定义 Agent。</h1>
            <p>Role 只决定模型；Agent 定义执行者与工具边界。草稿和预设不会自动应用。</p>
          </section>

          <section className="legend-card">
            <div className="legend-title"><span className="spark">✦</span> OMP 映射</div>
            <div className="mapping-row"><span>模型 Role</span><code>modelRoles.&lt;role&gt;</code></div>
            <div className="mapping-row"><span>Agent 定义</span><code>agents/*.md → @role</code></div>
            <p>Agent 可引用任意已有 Role；名称无需相同。Reviewer 仍默认跟随 Slow。</p>
          </section>

          <section className="preset-card">
            <div className="catalog-heading">
              <span>PRESETS</span>
              <strong>{presets.length}</strong>
            </div>
            <p className="preset-subtitle">只保存 Role 的模型组合，不保存 Agent Markdown</p>

            {presets.length > 0 && (
              <label className="preset-field">
                <span>Saved preset</span>
                <select
                  value={selectedPresetId}
                  onChange={event => {
                    const selected = presets.find(preset => preset.id === event.target.value);
                    if (selected) {
                      loadPreset(selected);
                    } else {
                      setPendingDeletePresetId("");
                      setSelectedPresetId("");
                      setPresetName("");
                      setPresetMessage("");
                    }
                  }}
                  aria-label="Select preset"
                >
                  <option value="">选择预设…</option>
                  {presets.map(preset => <option value={preset.id} key={preset.id}>{preset.name}</option>)}
                </select>
              </label>
            )}

            <label className="preset-field">
              <span>New preset name</span>
              <input
                value={presetName}
                onChange={event => setPresetName(event.target.value)}
                placeholder="输入新预设名称"
                aria-label="New preset name"
              />
            </label>

            <div className="preset-actions">
              <button
                className="button compact"
                onClick={savePresetOverride}
                disabled={!selectedPresetId || hasInvalidSelections}
              >
                Save
              </button>
              <button
                className="button compact"
                onClick={savePresetAs}
                disabled={hasInvalidSelections || !presetName.trim()}
              >
                Save as
              </button>
              <button
                className="button compact danger"
                onClick={deletePreset}
                disabled={!selectedPresetId}
                aria-label={deletePending ? "Confirm delete selected preset" : "Delete selected preset"}
              >
                {deletePending ? "确认删除" : "Delete"}
              </button>
            </div>

            <p className={`preset-status${presetMessage ? " has-message" : ""}`}>
              {presetMessage || "选择预设自动载入模型；Save 覆盖，Save as 新建。Markdown 始终保持独立。"}
            </p>
          </section>

          <section className="provider-card">
            <div className="catalog-heading"><span>PROVIDER LIST</span><strong>{providerNames.length}</strong></div>
            <p className="provider-help">只有勾选 Use 的 provider 才能在新的 Model 选择中使用；已选模型仍会保留。</p>
            <div className="provider-key">
              <span><i className="key-mark used-mark" /> Used by roles</span>
              <span><i className="key-mark disabled-mark" /> Disabled</span>
            </div>
            <div className="provider-list">
              {providerNames.map(provider => {
                const isDisabled = disabledProviders.includes(provider);
                const selected = selectedModelSlots.filter(item => providerForSelector(item.value) === provider);
                const isInvalid = selected.length > 0 && isDisabled;
                return (
                  <div className={`provider-row ${isDisabled ? "is-disabled" : ""} ${isInvalid ? "is-invalid" : ""}`} key={provider}>
                    <span className="provider-name">{provider}</span>
                    {selected.length ? (
                      <span className="provider-selected">Used · {selected.length}</span>
                    ) : (
                      <span className="provider-state">{isDisabled ? "disabled" : "available"}</span>
                    )}
                    <label className="provider-use-control">
                      <input
                        type="checkbox"
                        checked={!isDisabled}
                        onChange={event => toggleProvider(provider, !event.target.checked)}
                        aria-label={`${isDisabled ? "Enable" : "Disable"} ${provider}`}
                      />
                      <span>Use</span>
                    </label>
                  </div>
                );
              })}
              {providerNames.length === 0 && <div className="empty-state">没有发现 provider。</div>}
            </div>
          </section>

          <section className="catalog-card">
            <div className="catalog-heading"><span>MODEL CATALOG</span><strong>{catalogModels.length}</strong></div>
            <div className="catalog-meta">{providerNames.length} providers · {snapshot?.modelSource}</div>
            <label className="search-field">
              <span className="search-icon">⌕</span>
              <input value={filter} onChange={event => setFilter(event.target.value)} placeholder="Filter provider / model" />
              {filter && <button className="clear-search" onClick={() => setFilter("")} aria-label="Clear filter">×</button>}
            </label>
            <div className="model-catalog-list">
              {catalogModels.map(model => {
                const selected = selectedModelSlots.filter(item => item.base === model.selector);
                return (
                  <div className={`catalog-model-row ${selected.length ? "is-selected" : ""}`} key={model.selector}>
                    <span className="model-status" />
                    <span className="model-label"><strong>{model.provider}</strong>/{model.id}</span>
                    {selected.length > 0 && <span className="model-selected-badge">{selected.map(item => item.label).join(" · ")}</span>}
                  </div>
                );
              })}
              {catalogModels.length === 0 && <div className="empty-state">当前筛选没有可见模型。</div>}
            </div>
            <p>已选模型高亮显示；已禁用 provider 的模型不在列表中。</p>
          </section>

          {snapshot?.warnings.map(warning => <div className="warning-box" key={warning}>{warning}</div>)}
        </aside>

        <main className="main-panel">
          <div className="panel-heading">
            <div>
              <span className="section-kicker">MODEL ROLES</span>
              <h2>内建模型 Role</h2>
            </div>
            <div className="panel-summary">
              <span>{MODEL_ROLE_DEFINITIONS.length + customRoles.length - emptyRoleSlots}/{MODEL_ROLE_DEFINITIONS.length + customRoles.length} configured</span>
              <span className="summary-line" />
            </div>
          </div>

          <div className="slot-list">
            {MODEL_ROLE_DEFINITIONS.map(definition => (
              <SlotCard
                key={definition.key}
                definition={definition}
                value={values[definition.key]}
                models={snapshot?.models ?? []}
                filter={filter}
                disabledProviders={disabledProviders}
                reviewerFollowsSlow={reviewerFollowsSlow}
                securityReviewerFollowsSlow={securityReviewerFollowsSlow}
                slowValue={values.slow}
                onChange={value => updateValue(definition.key, value)}
                onFollowChange={updateReviewerMode}
              />
            ))}
          </div>


          <section className="agent-override-section">
            <div className="custom-role-heading">
              <div>
                <span className="section-kicker">BUILT-IN AGENT MODEL OVERRIDE</span>
                <h2>内建 Agent 模型覆盖</h2>
                <p>Reviewer 与 Security Reviewer 都是内建 Agent，不是 Role；这里仅配置它们使用的模型。</p>
              </div>
            </div>
            <div className="slot-list">
              {BUILTIN_AGENT_OVERRIDE_DEFINITIONS.map(definition => (
                <SlotCard
                  key={definition.key}
                  definition={definition}
                  value={values[definition.key]}
                  models={snapshot?.models ?? []}
                  filter={filter}
                  disabledProviders={disabledProviders}
                  reviewerFollowsSlow={reviewerFollowsSlow}
                  securityReviewerFollowsSlow={securityReviewerFollowsSlow}
                  slowValue={values.slow}
                  onChange={value => updateValue(definition.key, value)}
                  onFollowChange={updateReviewerMode}
                />
              ))}
            </div>
          </section>
          <section className="custom-role-section">
            <div className="custom-role-heading">
              <div>
                <span className="section-kicker">CUSTOM MODEL ROLES</span>
                <h2>自定义Role</h2>
                <p>仅维护 modelRoles 映射；用于独立的模型、成本或延迟边界。</p>
              </div>
              <button className="button" onClick={startCustomRole}>新增模型 Role</button>
            </div>
            {customRoles.length > 0 ? (
              <div className="slot-list">
                {customRoles.map(customRole => (
                  <CustomRoleCard
                    key={customRole.role}
                    customRole={customRole}
                    models={snapshot?.models ?? []}
                    filter={filter}
                    disabledProviders={disabledProviders}
                    onChange={value => updateCustomRoleValue(customRole.role, value)}
                  />
                ))}
              </div>
            ) : (
              <div className="custom-role-empty">
                尚未配置自定义模型 Role。只有需要独立模型预算时才新增。
              </div>
            )}
          </section>



          <section className="custom-agent-section">
            <div className="custom-role-heading">
              <div>
                <span className="section-kicker">CUSTOM AGENTS</span>
                <h2>自定义 Agent</h2>
                <p>每个 agents/*.md 定义执行者、工具边界与输出契约，并可引用任意模型 Role。</p>
              </div>
              <button className="button" onClick={startAgent}>新增自定义 Agent</button>
            </div>
            {customAgents.length > 0 ? (
              <div className="agent-list">
                {customAgents.map(agent => (
                  <CustomAgentCard
                    key={agent.fileName}
                    agent={agent}
                    onEdit={() => editAgent(agent)}
                  />
                ))}
              </div>
            ) : (
              <div className="custom-role-empty">
                尚未发现 ~/.omp/agent/agents/*.md。模型 Role 与 Agent 可分别创建。
              </div>
            )}
          </section>

          <div className="bottom-bar">
            <div className="bottom-note">
              <span className="info-icon">i</span>
              <span>
                预设只切换模型。Apply 会写入 modelRoles 和已编辑的 Agent 文件；Skill 不属于本工具的配置范围。
              </span>
            </div>
          </div>
          {error && <div className="inline-error">{error}</div>}
        </main>
      </div>
      {roleEditor && (
        <CustomRoleModal
          editor={roleEditor}
          models={snapshot?.models ?? []}
          filter={filter}
          disabledProviders={disabledProviders}
          existingRoles={customRoles.map(customRole => customRole.role)}
          onChange={setRoleEditor}
          onCancel={() => setRoleEditor(null)}
          onSave={saveCustomRoleDraft}
        />
      )}
      {agentEditor && (
        <AgentModal
          editor={agentEditor}
          roles={agentRoleOptions}
          existingAgents={customAgents.map(agent => agent.name)}
          onChange={setAgentEditor}
          onCancel={() => setAgentEditor(null)}
          onSave={saveAgentDraft}
        />
      )}
    </div>
  );
}
