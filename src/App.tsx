import { useEffect, useMemo, useState } from "react";
import { discoverCustomProviderModels, loadSnapshot, openConfigFile, saveCustomProviders, saveSnapshot } from "./api";
import {
  CUSTOM_PROVIDER_API_OPTIONS,
  CUSTOM_PROVIDER_PATTERN,
  CustomProviderModal,
  customProviderApiOverrides,
  inferredCustomProviderApi,
  sortedCustomProviderModels,
  type CustomProviderEditor
} from "./CustomProviderModal";
import type {
  ConfigSnapshot,
  CustomProvider,
  FallbackChain,
  ModelOption,
  RoutingSettings,
  SaveRequest
} from "./types";
import "./styles.css";

type SlotKey =
  | "default"
  | "smol"
  | "slow"
  | "vision"
  | "plan"
  | "designer"
  | "commit"
  | "tiny"
  | "task"
  | "advisor"
  | "reviewer"
  | "securityReviewer";
type ModelRoleKey = Exclude<SlotKey, "reviewer" | "securityReviewer">;
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
  agentModelOverrides?: Record<string, string>;
  routingSettings?: RoutingSettings;
  shadowedProviders?: string[];
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
    eyebrow: "快速任务",
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
    key: "plan",
    title: "Plan",
    eyebrow: "架构规划",
    path: "modelRoles.plan",
    description: "Plan mode 的架构、边界和实施方案。",
    color: "violet"
  },
  {
    key: "task",
    title: "Task",
    eyebrow: "通用子任务",
    path: "modelRoles.task",
    description: "通用 task Agent 的实现与整合工作。",
    color: "cyan"
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
    key: "designer",
    title: "Designer",
    eyebrow: "设计实现",
    path: "modelRoles.designer",
    description: "UI/UX、视觉实现和设计审查。",
    color: "rose"
  },
  {
    key: "commit",
    title: "Commit",
    eyebrow: "提交文本",
    path: "modelRoles.commit",
    description: "Commit message 与 changelog 生成。",
    color: "amber"
  },
  {
    key: "tiny",
    title: "Tiny",
    eyebrow: "后台轻量",
    path: "modelRoles.tiny",
    description: "标题、分类和自动 thinking 等后台小任务。",
    color: "cyan"
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
const SYSTEM_MODEL_ROLE_IDS: ModelRoleKey[] = [
  "default",
  "smol",
  "slow",
  "plan",
  "task",
  "vision",
  "designer",
  "commit",
  "tiny",
  "advisor"
];
const SYSTEM_MODEL_ROLE_LOOKUP: Record<ModelRoleKey, true> = {
  default: true,
  smol: true,
  slow: true,
  plan: true,
  task: true,
  vision: true,
  designer: true,
  commit: true,
  tiny: true,
  advisor: true
};
const AGENT_OVERRIDE_KEY_LOOKUP: Record<"reviewer" | "securityReviewer", true> = {
  reviewer: true,
  securityReviewer: true
};

const MODEL_ROLE_DEFINITIONS = SLOT_DEFINITIONS.filter(
  definition => !Object.hasOwn(AGENT_OVERRIDE_KEY_LOOKUP, definition.key)
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
const OMP_PATH_STORAGE_KEY = "omp-model-studio.omp-path";
const THEME_STORAGE_KEY = "omp-model-studio.theme";
const CUSTOM_ROLE_PATTERN = /^[a-z][a-z0-9_-]*$/;

const EMPTY_VALUES: SlotValues = {
  default: "",
  smol: "",
  slow: "",
  plan: "",
  task: "",
  vision: "",
  designer: "",
  commit: "",
  tiny: "",
  advisor: "",
  reviewer: "",
  securityReviewer: ""
};

const EMPTY_ROUTING_SETTINGS: RoutingSettings = {
  defaultThinkingLevel: null,
  autoThinkingMaxEffort: null,
  advisorEnabled: null,
  advisorSyncBacklog: null,
  advisorImmuneTurns: null,
  prewalkEnabled: null,
  retryModelFallback: null,
  retryFallbackChains: [],
  retryFallbackRevertPolicy: null
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


function modelIdForProviderSelector(value: string, provider: string): string {
  const base = splitSelector(value).base;
  const prefix = `${provider}/`;
  return base.startsWith(prefix) ? base.slice(prefix.length) : "";
}


function isUnavailableModelSelector(value: string, models: ModelOption[]): boolean {
  const parsed = splitSelector(value);
  if (!parsed.base || parsed.alias || !parsed.base.includes("/") || parsed.base.endsWith("/*")) return false;
  return !models.some(model => model.selector === parsed.base);
}

function modelSelectorError(value: string, models: ModelOption[], allowProviderWildcard = false): string {
  const parsed = splitSelector(value);
  if (!parsed.base || parsed.alias) return "";
  if (parsed.base.endsWith("/*")) {
    return allowProviderWildcard && /^[^/]+\/\*$/.test(parsed.base)
      ? ""
      : "模型 selector 格式无效，请使用 provider/model 或 @role。";
  }
  if (!parsed.base.includes("/")) {
    return allowProviderWildcard
      ? "模型 selector 格式无效，请使用 provider/model、provider/* 或 @role。"
      : "模型 selector 格式无效，请使用 provider/model 或 @role。";
  }
  return isUnavailableModelSelector(value, models)
    ? "当前配置模型不在 OMP 当前可用模型列表中。"
    : "";
}

function firstModelSelector(value: string): string {
  return value.split(",").map(item => item.trim()).find(Boolean) ?? "";
}
function effectiveAgentModel(agent: CustomAgent, overrides: Record<string, string>): string {
  const override = overrides[agent.name]?.trim();
  return override || agent.model;
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

function stringList(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return [...new Set(
    value
      .filter((item): item is string => typeof item === "string")
      .map(item => item.trim())
      .filter(Boolean)
  )];
}


function cloneRoutingSettings(settings: RoutingSettings): RoutingSettings {
  return {
    ...settings,
    retryFallbackChains: settings.retryFallbackChains.map(chain => ({
      target: chain.target,
      selectors: [...chain.selectors]
    }))
  };
}

/**
 * Shadowing a provider is a Model Studio draft overlay: move on to the next
 * candidate immediately, not leave every affected Role flagged red for manual
 * repair. It is deliberately separate from OMP's `disabledProviders`, which
 * controls the models returned by `omp models --json` and is never changed here.
 * For each system/custom Role currently pointed at a newly-shadowed provider,
 * promote the first surviving (non-shadowed) candidate from that Role's own
 * fallback chain to primary. Roles with no fallback chain, or whose whole
 * chain is also shadowed, are left invalid — there is no safe candidate to
 * guess, so those still need a manual pick.
 * Any chain selector on a shadowed provider is stripped from the draft too,
 * so the resulting config no longer routes through a provider the user hid.
 */

function reassignAwayFromShadowedProviders(
  shadowed: string[],
  values: SlotValues,
  customRoles: CustomRole[],
  routingSettings: RoutingSettings,
  models: ModelOption[]
): { values: SlotValues; customRoles: CustomRole[]; routingSettings: RoutingSettings; changed: boolean } {
  const shadowedSet = new Set(shadowed);
  const isShadowedSelector = (selector: string) => {
    const provider = providerForSelector(selector);
    return Boolean(provider) && shadowedSet.has(provider);
  };
  const chains = routingSettings.retryFallbackChains.map(chain => ({
    target: chain.target,
    selectors: [...chain.selectors]
  }));

  const applicableChainIndex = (role: string, currentValue: string): number | undefined => {
    const current = splitSelector(currentValue);
    const currentProvider = providerForSelector(currentValue);
    if (!current.base || current.alias || !currentProvider || current.base.endsWith("/*")) return undefined;

    const exactIndex = chains.findIndex(chain => {
      const target = splitSelector(chain.target.trim());
      return target.base === current.base && target.base.includes("/") && !target.base.endsWith("/*");
    });
    if (exactIndex >= 0) return exactIndex;

    const currentModelId = current.base.slice(`${currentProvider}/`.length);
    let wildcardIndex = -1;
    let wildcardPrefixLength = -1;
    chains.forEach((chain, index) => {
      const target = splitSelector(chain.target.trim()).base;
      if (!target.endsWith("/*")) return;
      const template = target.slice(0, -2);
      const slash = template.indexOf("/");
      const provider = slash < 0 ? template : template.slice(0, slash);
      const idPrefix = slash < 0 ? "" : template.slice(slash + 1);
      if (provider !== currentProvider || (idPrefix && !currentModelId.startsWith(`${idPrefix}/`))) return;
      if (idPrefix.length > wildcardPrefixLength) {
        wildcardIndex = index;
        wildcardPrefixLength = idPrefix.length;
      }
    });
    if (wildcardIndex >= 0) return wildcardIndex;

    const roleIndex = chains.findIndex(chain => chain.target.trim() === role);
    if (roleIndex >= 0) return roleIndex;
    const defaultIndex = chains.findIndex(chain => chain.target.trim() === "default");
    return defaultIndex >= 0 ? defaultIndex : undefined;
  };

  const materializeCandidate = (selector: string, currentValue: string): string | null => {
    const parsed = splitSelector(selector);
    if (!parsed.base.endsWith("/*")) return parsed.base ? selector : null;
    const current = splitSelector(currentValue);
    const currentProvider = providerForSelector(currentValue);
    if (!currentProvider || !current.base.startsWith(`${currentProvider}/`)) return null;
    const currentModelId = current.base.slice(`${currentProvider}/`.length);
    const template = parsed.base.slice(0, -2);
    const slash = template.indexOf("/");
    const targetProvider = slash < 0 ? template : template.slice(0, slash);
    const idPrefix = slash < 0 ? "" : template.slice(slash + 1);
    const bareModelId = currentModelId.split("/").pop() ?? "";
    const candidateModelIds = idPrefix
      ? [`${idPrefix}/${bareModelId}`]
      : [currentModelId, ...(currentModelId === bareModelId ? [] : [bareModelId])];
    const model = candidateModelIds
      .map(modelId => `${targetProvider}/${modelId}`)
      .map(selectorBase => models.find(model => model.selector === selectorBase && !shadowedSet.has(model.provider)))
      .find((model): model is ModelOption => Boolean(model));
    return model ? withEffort(model.selector, parsed.effort) : null;
  };

  const consumedFallbacks = new Map<string, Set<string>>();
  const consume = (chain: FallbackChain, selector: string) => {
    const consumed = consumedFallbacks.get(chain.target.trim()) ?? new Set<string>();
    consumed.add(selector);
    consumedFallbacks.set(chain.target.trim(), consumed);
  };

  const promote = (role: string, currentValue: string): string | null => {
    if (!isShadowedSelector(currentValue)) return null;
    const chainIndex = applicableChainIndex(role, currentValue);
    if (chainIndex === undefined) return null;
    const chain = chains[chainIndex];
    for (const selector of chain.selectors) {
      if (isShadowedSelector(selector)) continue;
      const candidate = materializeCandidate(selector, currentValue);
      if (!candidate || isShadowedSelector(candidate)) continue;
      const parsed = splitSelector(candidate);
      if (!parsed.alias && parsed.base.includes("/") && !parsed.base.endsWith("/*") && isUnavailableModelSelector(candidate, models)) {
        continue;
      }
      consume(chain, selector);
      return candidate;
    }
    return null;
  };

  let valuesChanged = false;
  const nextValues = { ...values };
  SYSTEM_MODEL_ROLE_IDS.forEach(role => {
    const promoted = promote(role, values[role]);
    if (promoted) {
      nextValues[role] = promoted;
      valuesChanged = true;
    }
  });

  let customRolesChanged = false;
  const nextCustomRoles = customRoles.map(customRole => {
    const promoted = promote(customRole.role, customRole.value);
    if (!promoted) return customRole;
    customRolesChanged = true;
    return { ...customRole, value: promoted };
  });

  const finalChains = chains
    .filter(chain => !isShadowedSelector(chain.target))
    .map(chain => {
      const consumed = consumedFallbacks.get(chain.target.trim());
      return {
        target: chain.target,
        selectors: chain.selectors.filter(selector => !isShadowedSelector(selector) && !consumed?.has(selector))
      };
    })
    .filter(chain => chain.selectors.length > 0);
  const chainsChanged = finalChains.length !== chains.length
    || finalChains.some((chain, index) => {
      const original = chains[index];
      return !original
        || original.target !== chain.target
        || original.selectors.length !== chain.selectors.length
        || original.selectors.some((selector, selectorIndex) => selector !== chain.selectors[selectorIndex]);
    });

  return {
    values: nextValues,
    customRoles: nextCustomRoles,
    routingSettings: chainsChanged
      ? cloneRoutingSettings({ ...routingSettings, retryFallbackChains: finalChains })
      : routingSettings,
    changed: valuesChanged || customRolesChanged || chainsChanged
  };
}

function parseRoutingSettings(value: unknown): RoutingSettings | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
  const record = value as Record<string, unknown>;
  const nullableString = (field: string): string | null =>
    typeof record[field] === "string" ? record[field] : null;
  const nullableBoolean = (field: string): boolean | null =>
    typeof record[field] === "boolean" ? record[field] : null;
  const nullableNumber = (field: string): number | null =>
    typeof record[field] === "number" && Number.isFinite(record[field]) ? record[field] : null;
  const retryFallbackChains = Array.isArray(record.retryFallbackChains)
    ? record.retryFallbackChains.flatMap(value => {
        if (!value || typeof value !== "object" || Array.isArray(value)) return [];
        const chain = value as Record<string, unknown>;
        if (typeof chain.target !== "string" || !Array.isArray(chain.selectors)) return [];
        return [{
          target: chain.target,
          selectors: chain.selectors.filter((selector): selector is string => typeof selector === "string")
        }];
      })
    : [];
  return {
    defaultThinkingLevel: nullableString("defaultThinkingLevel"),
    autoThinkingMaxEffort: nullableString("autoThinkingMaxEffort"),
    advisorEnabled: nullableBoolean("advisorEnabled"),
    advisorSyncBacklog: nullableString("advisorSyncBacklog"),
    advisorImmuneTurns: nullableNumber("advisorImmuneTurns"),
    prewalkEnabled: nullableBoolean("prewalkEnabled"),
    retryModelFallback: nullableBoolean("retryModelFallback"),
    retryFallbackChains,
    retryFallbackRevertPolicy: nullableString("retryFallbackRevertPolicy")
  };
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
  // Shadow state is WebView-local; never import OMP's disabledProviders into it.
  const shadowedProviders = Object.hasOwn(record, "shadowedProviders")
    ? stringList(record.shadowedProviders)
    : undefined;
  return {
    id: record.id,
    name: record.name,
    values: {
      default: storedValues.default ?? "",
      smol: storedValues.smol ?? "",
      slow: storedValues.slow ?? "",
      plan: storedValues.plan ?? "",
      task: storedValues.task ?? "",
      vision: storedValues.vision ?? "",
      designer: storedValues.designer ?? "",
      commit: storedValues.commit ?? "",
      tiny: storedValues.tiny ?? "",
      advisor: storedValues.advisor ?? "",
      reviewer: storedValues.reviewer ?? "",
      securityReviewer: storedValues.securityReviewer ?? ""
    },
    customRoleValues,
    reviewerFollowsSlow: record.reviewerFollowsSlow !== false,
    securityReviewerFollowsSlow: record.securityReviewerFollowsSlow !== false,
    agentModelOverrides: Object.hasOwn(record, "agentModelOverrides")
      ? recordOfStrings(record.agentModelOverrides)
      : undefined,
    routingSettings: parseRoutingSettings(record.routingSettings),
    shadowedProviders,
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

function thinkingOptionsForModel(model: ModelOption | undefined): string[] {
  if (!model) return ["auto"];
  const supported = model.thinking.length > 0
    ? model.thinking
    : model.reasoning
      ? KNOWN_EFFORTS
      : ["off"];
  return [...new Set(["auto", ...supported])];
}
function formatTokenLimit(value: number): string {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(2).replace(/\.?0+$/, "")}M`;
  if (value >= 1_000) return `${Math.round(value / 1_000)}K`;
  return String(value);
}

function formatPrice(value: number): string {
  return value.toFixed(value < 0.1 ? 3 : 2).replace(/\.?0+$/, "");
}

function ModelMetadata({ model }: { model: ModelOption }) {
  return (
    <div className="model-metadata" aria-label={`${model.name} metadata`}>
      {model.contextWindow !== null && <span title="Context window">Context {formatTokenLimit(model.contextWindow)}</span>}
      {model.maxTokens !== null && <span title="Maximum output tokens">Output {formatTokenLimit(model.maxTokens)}</span>}
      {model.input.length > 0 && <span>{model.input.map(item => item[0].toUpperCase() + item.slice(1)).join(" + ")}</span>}
      {model.cost && (
        <>
          <span title="Standard input price per million tokens">In ${formatPrice(model.cost.input)}</span>
          <span title="Standard output price per million tokens">Out ${formatPrice(model.cost.output)} / 1M</span>
          <span title="Cache read / cache write price per million tokens">
            Cache ${formatPrice(model.cost.cacheRead)} / ${formatPrice(model.cost.cacheWrite)}
          </span>
        </>
      )}
    </div>
  );
}


function ModelPicker({
  value,
  models,
  filter,
  shadowedProviders,
  onChange,
  disabled = false
}: {
  value: string;
  models: ModelOption[];
  filter: string;
  shadowedProviders: string[];
  onChange: (value: string) => void;
  disabled?: boolean;
}) {
  const parsed = splitSelector(value);
  const currentProvider = providerForSelector(value);
  const shadowedProviderSet = new Set(shadowedProviders);
  const currentModel = models.find(model => model.selector === parsed.base);
  const providers = useMemo(() => {
    const names = new Set(
      models
        .filter(model => !shadowedProviderSet.has(model.provider))
        .map(model => model.provider)
    );
    if (currentProvider) names.add(currentProvider);
    return [...names].sort((left, right) => left.localeCompare(right));
  }, [currentProvider, models, shadowedProviders]);

  const selectedProvider = currentProvider || providers[0] || "";
  const selectedProviderIsShadowed = shadowedProviderSet.has(selectedProvider);
  const providerModels = models.filter(
    model => model.provider === selectedProvider && !selectedProviderIsShadowed
  );
  const normalizedFilter = filter.trim().toLowerCase();
  const filteredModels = providerModels.filter(model => {
    if (!normalizedFilter) return true;
    return `${model.id} ${model.name} ${model.provider}`.toLowerCase().includes(normalizedFilter);
  });
  const availableModels = filteredModels.length > 0 ? filteredModels : providerModels;
  const selectedModel = currentModel ?? availableModels[0];
  const selectedBase = parsed.base || selectedModel?.selector || "";
  const currentModelError = modelSelectorError(value, models);
  const providerHasModels = (provider: string) => models.some(model => model.provider === provider);
  const effortOptions = useMemo(() => thinkingOptionsForModel(selectedModel), [selectedModel]);

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

  const selectModel = (nextModel: ModelOption | undefined, fallbackSelector = "") => {
    const selector = nextModel?.selector ?? fallbackSelector;
    const nextEffort = thinkingOptionsForModel(nextModel).includes(parsed.effort)
      ? parsed.effort
      : "auto";
    onChange(withEffort(selector, nextEffort));
  };

  const changeProvider = (provider: string) => {
    selectModel(models.find(model => model.provider === provider), `${provider}/`);
  };

  return (
    <div className="model-picker">
      <div className="picker-grid">
        <label className="field-group">
          <span>Provider</span>
          <select
            value={selectedProvider}
            disabled={disabled || providers.length === 0}
            onChange={event => changeProvider(event.target.value)}
          >
            {providers.length === 0 && <option value="">No providers</option>}
            {providers.map(provider => {
              const isShadowed = shadowedProviderSet.has(provider);
              const hasModels = providerHasModels(provider);
              return (
                <option value={provider} key={provider} disabled={isShadowed || !hasModels}>
                  {provider}{isShadowed ? " · shadowed" : !hasModels ? " · unavailable" : ""}
                </option>
              );
            })}
          </select>
        </label>
        <label className="field-group model-field">
          <span>Model</span>
          <select
            value={selectedBase}
            disabled={disabled || availableModels.length === 0 || selectedProviderIsShadowed}
            onChange={event => selectModel(models.find(model => model.selector === event.target.value), event.target.value)}
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
            disabled={disabled || !selectedBase || selectedProviderIsShadowed || availableModels.length === 0}
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
      {(parsed.base ? currentModel : selectedModel) && (
        <ModelMetadata model={(parsed.base ? currentModel : selectedModel)!} />
      )}
      {currentModelError && <p className="model-picker-warning">{currentModelError}</p>}
    </div>
  );
}

function optionalBooleanFromControl(value: string): boolean | null {
  if (value === "on") return true;
  if (value === "off") return false;
  return null;
}

function RoutingPolicySection({
  settings,
  models,
  shadowedProviders,
  roleNames,
  errors,
  onChange
}: {
  settings: RoutingSettings;
  models: ModelOption[];
  shadowedProviders: string[];
  roleNames: string[];
  errors: string[];
  onChange: (settings: RoutingSettings) => void;
}) {
  const effectiveThinking = settings.defaultThinkingLevel ?? "high";
  const effectiveAdvisor = settings.advisorEnabled ?? false;
  const effectiveFallback = settings.retryModelFallback ?? true;
  const shadowedProviderSet = new Set(shadowedProviders);
  const selectableModels = useMemo(
    () => models.filter(model => !shadowedProviderSet.has(model.provider)),
    [models, shadowedProviders]
  );
  const providers = useMemo(
    () => [...new Set(selectableModels.map(model => model.provider))].sort((left, right) => left.localeCompare(right)),
    [selectableModels]
  );
  const targetSuggestions = useMemo(
    () => [
      ...new Set([
        ...roleNames,
        ...selectableModels.map(model => model.selector),
        ...providers.map(provider => `${provider}/*`)
      ])
    ],
    [providers, roleNames, selectableModels]
  );
  const selectorSuggestions = useMemo(
    () => [
      ...new Set([
        ...roleNames.map(role => `@${role}`),
        ...selectableModels.map(model => model.selector),
        ...providers.map(provider => `${provider}/*`)
      ])
    ],
    [providers, roleNames, selectableModels]
  );

  const replaceChain = (index: number, chain: FallbackChain) => {
    const retryFallbackChains = settings.retryFallbackChains.map((current, currentIndex) =>
      currentIndex === index ? chain : current
    );
    onChange({ ...settings, retryFallbackChains });
  };

  const moveChainSelector = (chainIndex: number, selectorIndex: number, offset: -1 | 1) => {
    const chain = settings.retryFallbackChains[chainIndex];
    const targetIndex = selectorIndex + offset;
    if (!chain || targetIndex < 0 || targetIndex >= chain.selectors.length) return;
    const selectors = [...chain.selectors];
    [selectors[selectorIndex], selectors[targetIndex]] = [selectors[targetIndex], selectors[selectorIndex]];
    replaceChain(chainIndex, { ...chain, selectors });
  };

  const addChain = () => {
    const existingTargets = new Set(settings.retryFallbackChains.map(chain => chain.target));
    const target = roleNames.find(role => !existingTargets.has(role)) ?? "";
    onChange({
      ...settings,
      retryFallbackChains: [...settings.retryFallbackChains, { target, selectors: [""] }]
    });
  };

  return (
    <section className="routing-section">
      <div className="custom-role-heading">
        <div>
          <span className="section-kicker">ROUTING POLICY</span>
          <h2>Thinking、Advisor 与故障转移</h2>
          <p>空值表示继承 OMP 默认；显式 Role thinking 后缀仍优先于全局 thinking。</p>
        </div>
      </div>

      <div className="policy-grid">
        <article className="policy-card">
          <div className="policy-card-heading">
            <span>THINKING</span>
            <code>defaultThinkingLevel</code>
          </div>
          <label className="policy-field">
            <span>Global level</span>
            <select
              value={settings.defaultThinkingLevel ?? ""}
              onChange={event => onChange({
                ...settings,
                defaultThinkingLevel: event.target.value || null
              })}
            >
              <option value="">OMP default · High</option>
              <option value="auto">Auto</option>
              {KNOWN_EFFORTS.map(effort => (
                <option value={effort} key={effort}>{EFFORT_LABELS[effort] ?? effort}</option>
              ))}
            </select>
          </label>
          <label className="policy-field">
            <span>Auto ceiling</span>
            <select
              value={settings.autoThinkingMaxEffort ?? ""}
              disabled={effectiveThinking !== "auto"}
              onChange={event => onChange({
                ...settings,
                autoThinkingMaxEffort: event.target.value || null
              })}
            >
              <option value="">OMP default · XHigh</option>
              <option value="xhigh">XHigh</option>
              <option value="max">Max</option>
            </select>
          </label>
          <p className="policy-effective">Effective global level: <strong>{effectiveThinking}</strong></p>
        </article>

        <article className="policy-card">
          <div className="policy-card-heading">
            <span>RUNTIME</span>
            <code>advisor / prewalk</code>
          </div>
          <label className="policy-field">
            <span>Advisor</span>
            <select
              value={settings.advisorEnabled === null ? "" : settings.advisorEnabled ? "on" : "off"}
              onChange={event => onChange({
                ...settings,
                advisorEnabled: optionalBooleanFromControl(event.target.value)
              })}
            >
              <option value="">OMP default · Off</option>
              <option value="on">On</option>
              <option value="off">Off</option>
            </select>
          </label>
          <div className="policy-field-row">
            <label className="policy-field">
              <span>Sync backlog</span>
              <select
                value={settings.advisorSyncBacklog ?? ""}
                disabled={!effectiveAdvisor}
                onChange={event => onChange({
                  ...settings,
                  advisorSyncBacklog: event.target.value || null
                })}
              >
                <option value="">OMP default · Off</option>
                {["off", "1", "3", "5"].map(value => (
                  <option value={value} key={value}>{value === "off" ? "Off" : `${value} turn${value === "1" ? "" : "s"}`}</option>
                ))}
              </select>
            </label>
            <label className="policy-field">
              <span>Immune turns</span>
              <select
                value={settings.advisorImmuneTurns ?? ""}
                disabled={!effectiveAdvisor}
                onChange={event => onChange({
                  ...settings,
                  advisorImmuneTurns: event.target.value ? Number(event.target.value) : null
                })}
              >
                <option value="">OMP default · 3</option>
                {[0, 1, 2, 3, 4, 5].map(value => <option value={value} key={value}>{value}</option>)}
              </select>
            </label>
          </div>
          <label className="policy-field">
            <span>Prewalk</span>
            <select
              value={settings.prewalkEnabled === null ? "" : settings.prewalkEnabled ? "on" : "off"}
              onChange={event => onChange({
                ...settings,
                prewalkEnabled: optionalBooleanFromControl(event.target.value)
              })}
            >
              <option value="">OMP default · Off</option>
              <option value="on">On · hand off to @smol</option>
              <option value="off">Off</option>
            </select>
          </label>
        </article>

        <article className="policy-card">
          <div className="policy-card-heading">
            <span>RETRY</span>
            <code>retry.modelFallback</code>
          </div>
          <label className="policy-field">
            <span>Model fallback</span>
            <select
              value={settings.retryModelFallback === null ? "" : settings.retryModelFallback ? "on" : "off"}
              onChange={event => onChange({
                ...settings,
                retryModelFallback: optionalBooleanFromControl(event.target.value)
              })}
            >
              <option value="">OMP default · On</option>
              <option value="on">On</option>
              <option value="off">Off</option>
            </select>
          </label>
          <label className="policy-field">
            <span>Return to primary</span>
            <select
              value={settings.retryFallbackRevertPolicy ?? ""}
              disabled={!effectiveFallback}
              onChange={event => onChange({
                ...settings,
                retryFallbackRevertPolicy: event.target.value || null
              })}
            >
              <option value="">OMP default · Cooldown expiry</option>
              <option value="cooldown-expiry">Cooldown expiry</option>
              <option value="never">Never</option>
            </select>
          </label>
          <p className="policy-effective">
            {effectiveFallback ? "Configured chains are eligible after retry recovery." : "Fallback chains stay saved but are inactive."}
          </p>
        </article>
      </div>

      <article className={`fallback-card${errors.length ? " invalid" : ""}`}>
        <div className="fallback-heading">
          <div>
            <span>FALLBACK CHAINS</span>
            <p>Target 可使用 Role、具体模型或 provider wildcard；候选按顺序尝试。</p>
          </div>
          <button className="button compact" onClick={addChain}>新增链</button>
        </div>
        <datalist id="fallback-target-options">
          {targetSuggestions.map(value => <option value={value} key={value} />)}
        </datalist>
        <datalist id="fallback-selector-options">
          {selectorSuggestions.map(value => <option value={value} key={value} />)}
        </datalist>
        {settings.retryFallbackChains.length > 0 ? (
          <div className="fallback-list">
            {settings.retryFallbackChains.map((chain, chainIndex) => (
              <div className="fallback-chain" key={chainIndex}>
                <label className="policy-field fallback-target">
                  <span>Target</span>
                  <input
                    list="fallback-target-options"
                    value={chain.target}
                    onChange={event => replaceChain(chainIndex, { ...chain, target: event.target.value })}
                    placeholder="default 或 provider/*"
                  />
                </label>
                <div className="fallback-selectors">
                  {chain.selectors.map((selector, selectorIndex) => (
                    <div className="fallback-selector-row" key={selectorIndex}>
                      <span className="fallback-order">{selectorIndex + 1}</span>
                      <input
                        list="fallback-selector-options"
                        value={selector}
                        onChange={event => {
                          const selectors = [...chain.selectors];
                          selectors[selectorIndex] = event.target.value;
                          replaceChain(chainIndex, { ...chain, selectors });
                        }}
                        placeholder="provider/model[:thinking] 或 @role"
                        aria-label={`Fallback ${selectorIndex + 1} for ${chain.target || "new target"}`}
                      />
                      <button
                        className="icon-button"
                        onClick={() => moveChainSelector(chainIndex, selectorIndex, -1)}
                        disabled={selectorIndex === 0}
                        aria-label="Move fallback up"
                      >↑</button>
                      <button
                        className="icon-button"
                        onClick={() => moveChainSelector(chainIndex, selectorIndex, 1)}
                        disabled={selectorIndex === chain.selectors.length - 1}
                        aria-label="Move fallback down"
                      >↓</button>
                      <button
                        className="icon-button danger"
                        onClick={() => replaceChain(chainIndex, {
                          ...chain,
                          selectors: chain.selectors.filter((_, index) => index !== selectorIndex)
                        })}
                        aria-label="Remove fallback"
                      >×</button>
                    </div>
                  ))}
                  <button
                    className="button quiet compact"
                    onClick={() => replaceChain(chainIndex, {
                      ...chain,
                      selectors: [...chain.selectors, ""]
                    })}
                  >
                    添加候选
                  </button>
                </div>
                <button
                  className="button quiet compact danger"
                  onClick={() => onChange({
                    ...settings,
                    retryFallbackChains: settings.retryFallbackChains.filter((_, index) => index !== chainIndex)
                  })}
                >
                  删除链
                </button>
              </div>
            ))}
          </div>
        ) : (
          <div className="custom-role-empty">没有配置 fallback chain；OMP 只使用自身的凭据与重试恢复。</div>
        )}
        {errors.length > 0 && (
          <div className="routing-errors">
            {errors.map(error => <span key={error}>{error}</span>)}
          </div>
        )}
      </article>
    </section>
  );
}

function SlotCard({
  definition,
  value,
  models,
  filter,
  shadowedProviders,
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
  shadowedProviders: string[];
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
  const isShadowed = Boolean(effectiveProvider && shadowedProviders.includes(effectiveProvider));
  const modelError = modelSelectorError(effectiveValue, models);
  const isInvalid = isShadowed || Boolean(modelError);
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
              {definition.path && !Object.hasOwn(AGENT_OVERRIDE_KEY_LOOKUP, definition.key) && (
                <code className="config-path">{definition.path}</code>
              )}
            </div>
            <p>{definition.description}</p>
          </div>
        </div>
        <div className="slot-header-meta">
          {isInvalid && (
            <span
              className="invalid-badge"
              title={modelError || "配置使用了 shadowed provider"}
            >
              非法
            </span>
          )}
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
          shadowedProviders={shadowedProviders}
          onChange={onChange}
        />
      )}

    </article>
  );
}
function AgentOverrideCard({
  name,
  override,
  inheritedModel,
  models,
  filter,
  shadowedProviders,
  followsSlow,
  slowFollowAgent,
  onChange,
  onFollowChange,
  onEnable
}: {
  name: string;
  override: string;
  inheritedModel: string;
  models: ModelOption[];
  filter: string;
  shadowedProviders: string[];
  followsSlow: boolean;
  slowFollowAgent?: "reviewer" | "security-reviewer";
  onChange: (value: string) => void;
  onFollowChange?: (follow: boolean) => void;
  onEnable: () => void;
}) {
  const isReviewer = slowFollowAgent === "reviewer";
  const isSecurityReviewer = slowFollowAgent === "security-reviewer";
  const followsSlowControl = Boolean(slowFollowAgent);
  const definition = SLOT_DEFINITIONS.find(item =>
    (isReviewer && item.key === "reviewer") || (isSecurityReviewer && item.key === "securityReviewer")
  );
  const hasOverride = Boolean(override.trim());
  const effectiveValue = followsSlowControl && followsSlow ? inheritedModel : override;
  const effectiveProvider = providerForSelector(effectiveValue);
  const isShadowed = Boolean(effectiveProvider && shadowedProviders.includes(effectiveProvider));
  const modelError = effectiveValue ? modelSelectorError(effectiveValue, models) : "";
  const isInvalid = Boolean(effectiveValue && (isShadowed || modelError));
  const inheritedText = inheritedModel || "Agent Markdown / OMP 默认策略";

  return (
    <article className={`slot-card ${definition?.color ?? "cyan"}${isInvalid ? " invalid" : ""}`}>
      <div className="slot-header">
        <div className="slot-heading">
          <span className="slot-dot" />
          <div>
            <div className="slot-title-line">
              <h2>{definition?.title ?? name}</h2>
              <span className="eyebrow">{definition?.eyebrow ?? "Agent · 自定义"}</span>
              <code className="config-path">task.agentModelOverrides.{name}</code>
            </div>
            <p>{definition?.description ?? `自定义 ${name} Agent 的模型覆盖；留空则使用 Agent 定义的 model。`}</p>
          </div>
        </div>
        <div className="slot-header-meta">
          {isInvalid && (
            <span className="invalid-badge" title={modelError || "配置使用了 shadowed provider"}>非法</span>
          )}
        </div>
      </div>

      {followsSlowControl && onFollowChange ? (
        <div className="reviewer-mode">
          <label className="switch-row">
            <input
              type="checkbox"
              checked={followsSlow}
              onChange={event => onFollowChange(event.target.checked)}
            />
            <span className="switch" aria-hidden="true" />
            <span>跟随 Slow 角色</span>
          </label>
        </div>
      ) : !hasOverride ? (
        <div className="agent-override-empty">
          <div>
            <span>未设置覆盖</span>
            <code>{inheritedText}</code>
          </div>
          <button className="button compact" onClick={onEnable}>设置覆盖</button>
        </div>
      ) : (
        <div className="reviewer-mode agent-override-active">
          <span className="switch-row">已设置模型覆盖</span>
          <button className="button quiet compact" onClick={() => onChange("")}>清除覆盖</button>
        </div>
      )}

      {followsSlowControl && followsSlow && (
        <code className="inherited-model">{effectiveValue || "尚未选择"}</code>
      )}
      {((followsSlowControl && !followsSlow) || (!followsSlowControl && hasOverride)) && (
        <ModelPicker
          value={override}
          models={models}
          filter={filter}
          shadowedProviders={shadowedProviders}
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
  shadowedProviders,
  deletePending,
  onChange,
  onDelete
}: {
  customRole: CustomRole;
  models: ModelOption[];
  filter: string;
  shadowedProviders: string[];
  deletePending: boolean;
  onChange: (value: string) => void;
  onDelete: () => void;
}) {
  const provider = providerForSelector(customRole.value);
  const isShadowed = Boolean(provider && shadowedProviders.includes(provider));
  const modelError = modelSelectorError(customRole.value, models);
  const isInvalid = isShadowed || Boolean(modelError);
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
        <div className="custom-role-actions">
          {isInvalid && (
            <span
              className="invalid-badge"
              title={modelError || "配置使用了 shadowed provider"}
            >
              非法
            </span>
          )}
          <button className="button quiet compact danger" onClick={onDelete}>
            {deletePending ? "确认移除" : "移除"}
          </button>
        </div>
      </div>
      <ModelPicker
        value={customRole.value}
        models={models}
        filter={filter}
        shadowedProviders={shadowedProviders}
        onChange={onChange}
      />
    </article>
  );
}

function CustomAgentCard({
  agent,
  effectiveModel,
  models,
  shadowedProviders,
  onEdit
}: {
  agent: CustomAgent;
  effectiveModel: string;
  models: ModelOption[];
  shadowedProviders: string[];
  onEdit: () => void;
}) {
  const validationError = validateAgentMarkdown(agent.agentMarkdown);
  const agentProvider = providerForSelector(effectiveModel);
  const isShadowed = Boolean(agentProvider && shadowedProviders.includes(agentProvider));
  const modelError = modelSelectorError(effectiveModel, models);
  const isInvalid = Boolean(validationError || isShadowed || modelError);
  return (
    <article className={`agent-card${isInvalid ? " invalid" : ""}`}>
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
        <code>{effectiveModel}</code>
        {isInvalid && (
          <span
            className="invalid-badge"
            title={validationError || modelError || "配置使用了 shadowed provider"}
          >
            非法
          </span>
        )}
        <button className="button compact" onClick={onEdit}>编辑 Markdown</button>
      </div>
    </article>
  );
}


function CustomRoleModal({
  editor,
  models,
  filter,
  shadowedProviders,
  existingRoles,
  onChange,
  onCancel,
  onSave
}: {
  editor: CustomRoleEditor;
  models: ModelOption[];
  filter: string;
  shadowedProviders: string[];
  existingRoles: string[];
  onChange: (editor: CustomRoleEditor) => void;
  onCancel: () => void;
  onSave: () => void;
}) {
  const roleError = !editor.role
    ? "请输入 Role key。"
    : !CUSTOM_ROLE_PATTERN.test(editor.role)
      ? "Role key 需以小写字母开头，只能包含小写字母、数字、-、_。"
      : Object.hasOwn(SYSTEM_MODEL_ROLE_LOOKUP, editor.role)
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
              shadowedProviders={shadowedProviders}
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
                model: firstModelSelector(frontmatterField(agentMarkdown, "model")) || editor.model
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

export default function App({ onSwitchTool }: { onSwitchTool: () => void }) {
  const [snapshot, setSnapshot] = useState<ConfigSnapshot | null>(null);
  const [values, setValues] = useState<SlotValues>(EMPTY_VALUES);
  const [customRoles, setCustomRoles] = useState<CustomRole[]>([]);
  const [roleEditor, setRoleEditor] = useState<CustomRoleEditor | null>(null);
  const [removedModelRoles, setRemovedModelRoles] = useState<string[]>([]);
  const [pendingDeleteCustomRole, setPendingDeleteCustomRole] = useState("");
  const [customAgents, setCustomAgents] = useState<CustomAgent[]>([]);
  const [agentEditor, setAgentEditor] = useState<AgentEditor | null>(null);
  const [agentModelOverrides, setAgentModelOverrides] = useState<Record<string, string>>({});
  const [reviewerFollowsSlow, setReviewerFollowsSlow] = useState(true);
  const [securityReviewerFollowsSlow, setSecurityReviewerFollowsSlow] = useState(true);
  const [shadowedProviders, setShadowedProviders] = useState<string[]>([]);
  const [customProviders, setCustomProviders] = useState<CustomProvider[]>([]);
  const [customProviderEditor, setCustomProviderEditor] = useState<CustomProviderEditor | null>(null);
  const [customProviderModelLoading, setCustomProviderModelLoading] = useState(false);
  const [customProviderModelError, setCustomProviderModelError] = useState("");
  const [customProviderSaving, setCustomProviderSaving] = useState(false);
  const [customProviderSaveError, setCustomProviderSaveError] = useState("");
  const [pendingDeleteCustomProvider, setPendingDeleteCustomProvider] = useState("");
  const [routingSettings, setRoutingSettings] = useState<RoutingSettings>(
    () => cloneRoutingSettings(EMPTY_ROUTING_SETTINGS)
  );
  const [filter, setFilter] = useState("");
  const [customOmpPath, setCustomOmpPath] = useState(() => {
    if (typeof window === "undefined") return "";
    return window.localStorage.getItem(OMP_PATH_STORAGE_KEY) ?? "";
  });
  const [customOmpPathDraft, setCustomOmpPathDraft] = useState(customOmpPath);
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

  const applySnapshot = (next: ConfigSnapshot, localShadowedProviders: string[] = []) => {
    const nextAgentModelOverrides = recordOfStrings(next.agentModelOverrides);
    const rawReviewer = nextAgentModelOverrides.reviewer ?? "";
    const rawSecurityReviewer = nextAgentModelOverrides["security-reviewer"] ?? "";
    const followsSlow = rawReviewer === "" || rawReviewer === "@slow";
    const securityFollowsSlow = rawSecurityReviewer === "" || rawSecurityReviewer === "@slow";
    setAgentModelOverrides(nextAgentModelOverrides);
    setReviewerFollowsSlow(followsSlow);
    setSecurityReviewerFollowsSlow(securityFollowsSlow);
    setValues({
      default: next.modelRoles.default ?? "",
      smol: next.modelRoles.smol ?? "",
      slow: next.modelRoles.slow ?? "",
      plan: next.modelRoles.plan ?? "",
      task: next.modelRoles.task ?? "",
      vision: next.modelRoles.vision ?? "",
      designer: next.modelRoles.designer ?? "",
      commit: next.modelRoles.commit ?? "",
      tiny: next.modelRoles.tiny ?? "",
      advisor: next.modelRoles.advisor ?? "",
      reviewer: followsSlow ? next.modelRoles.slow ?? "" : rawReviewer,
      securityReviewer: securityFollowsSlow ? next.modelRoles.slow ?? "" : rawSecurityReviewer
    });
    const nextCustomRoles = Object.entries(next.modelRoles)
      .filter(([role]) => !Object.hasOwn(SYSTEM_MODEL_ROLE_LOOKUP, role))
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
    setRemovedModelRoles([]);
    setPendingDeleteCustomRole("");
    setAgentEditor(null);
    setCustomProviders(next.customProviders);
    setCustomProviderEditor(null);
    setCustomProviderModelLoading(false);
    setCustomProviderModelError("");
    setCustomProviderSaving(false);
    setCustomProviderSaveError("");
    setPendingDeleteCustomProvider("");
    setRoutingSettings(cloneRoutingSettings(next.routingSettings));
    const visibleProviders = new Set(next.models.map(model => model.provider));
    setShadowedProviders(
      [...new Set(localShadowedProviders.filter(provider => visibleProviders.has(provider)))]
        .sort((left, right) => left.localeCompare(right))
    );
    setSnapshot(next);
    setSaved(false);
  };

  const refresh = async (pathToTry?: string) => {
    const targetPath = typeof pathToTry === "string" ? pathToTry : customOmpPath;
    setLoading(true);
    setError("");
    try {
      const next = await loadSnapshot(targetPath.trim() || undefined);
      applySnapshot(next);
      if (targetPath.trim()) {
        setCustomOmpPath(targetPath.trim());
        try {
          window.localStorage.setItem(OMP_PATH_STORAGE_KEY, targetPath.trim());
        } catch {
          // LocalStorage fallback
        }
      }
    } catch (loadError) {
      setError(readableError(loadError));
    } finally {
      setLoading(false);
    }
  };

  const openEditableFile = async (kind: "config" | "models") => {
    setError("");
    try {
      await openConfigFile(kind);
    } catch (openError) {
      setError(readableError(openError));
    }
  };

  useEffect(() => {
    void refresh();
  }, []);
  const providerNames = useMemo(() => {
    // OMP has already applied config.yml's `disabledProviders` before
    // returning this model list; local shadow state must not add providers back.
    const names = new Set(snapshot?.models.map(model => model.provider) ?? []);
    return [...names].sort((left, right) => left.localeCompare(right));
  }, [snapshot]);

  const selectedModelSlots = useMemo(() => {
    const fixedAssignments = MODEL_ROLE_DEFINITIONS.map(definition => {
      const value = values[definition.key];
      return { label: definition.title, value, base: splitSelector(value).base, allowProviderWildcard: false };
    });
    const customAssignments = customRoles.map(customRole => ({
      label: customRole.role,
      value: customRole.value,
      base: splitSelector(customRole.value).base,
      allowProviderWildcard: false
    }));
    const customAgentNames = new Set(customAgents.map(agent => agent.name));
    const agentOverrideAssignments = Object.entries(agentModelOverrides)
      .filter(([agentName]) => !customAgentNames.has(agentName))
      .map(([agentName, value]) => ({
        label: `Agent ${agentName}`,
        value,
        base: splitSelector(value).base,
        allowProviderWildcard: false
      }));
    const customAgentAssignments = customAgents.map(agent => {
      const value = effectiveAgentModel(agent, agentModelOverrides);
      return {
        label: `Agent ${agent.name}`,
        value,
        base: splitSelector(value).base,
        allowProviderWildcard: false
      };
    });
    const fallbackAssignments = routingSettings.retryFallbackChains.flatMap(chain =>
      chain.selectors.map((selector, index) => ({
        label: `Fallback ${chain.target || "new"} ${index + 1}`,
        value: selector,
        base: splitSelector(selector).base,
        allowProviderWildcard: true
      }))
    );
    return [...fixedAssignments, ...customAssignments, ...agentOverrideAssignments, ...customAgentAssignments, ...fallbackAssignments]
      .filter(item => item.base && !item.base.startsWith("@"));
  }, [agentModelOverrides, customAgents, customRoles, routingSettings.retryFallbackChains, values]);

  const availableModels = snapshot?.models ?? [];
  const invalidSelections = selectedModelSlots.filter(item => {
    const provider = providerForSelector(item.value);
    return shadowedProviders.includes(provider)
      || Boolean(modelSelectorError(item.value, availableModels, item.allowProviderWildcard));
  });
  const hasInvalidSelections = invalidSelections.length > 0;
  const hasUnavailableSelections = invalidSelections.some(item => isUnavailableModelSelector(item.value, availableModels));
  const invalidCustomRoles = customRoles.filter(customRole => !customRole.value.trim());
  const invalidAgentModels = customAgents.filter(agent => {
    const value = effectiveAgentModel(agent, agentModelOverrides);
    const provider = providerForSelector(value);
    return shadowedProviders.includes(provider) || Boolean(modelSelectorError(value, availableModels));
  });
  const hasUnavailableAgents = invalidAgentModels.some(agent =>
    isUnavailableModelSelector(effectiveAgentModel(agent, agentModelOverrides), availableModels)
  );
  const invalidAgentMarkdown = customAgents.filter(agent => Boolean(validateAgentMarkdown(agent.agentMarkdown)));
  const invalidAgents = [...new Set([...invalidAgentMarkdown, ...invalidAgentModels])];
  const invalidAgentOverrides = Object.entries(agentModelOverrides).filter(([, value]) => {
    const trimmed = value.trim();
    if (!trimmed) return false;
    const provider = providerForSelector(trimmed);
    return shadowedProviders.includes(provider) || Boolean(modelSelectorError(trimmed, availableModels));
  });
  const hasUnavailableAgentOverrides = invalidAgentOverrides.some(([, value]) => isUnavailableModelSelector(value, availableModels));

  const routingErrors = useMemo(() => {
    const errors: string[] = [];
    const targets = new Set<string>();
    const roleAssignments = new Map<string, string>([
      ...SYSTEM_MODEL_ROLE_IDS.map(role => [role, values[role]] as [string, string]),
      ...customRoles.map(role => [role.role, role.value] as [string, string])
    ]);
    routingSettings.retryFallbackChains.forEach((chain, chainIndex) => {
      const target = chain.target.trim();
      if (!target) errors.push(`Fallback chain ${chainIndex + 1} 缺少 target。`);
      else if (targets.has(target)) errors.push(`Fallback target “${target}”重复。`);
      else targets.add(target);
      const targetProvider = providerForSelector(target);
      if (targetProvider && shadowedProviders.includes(targetProvider)) {
        errors.push(`Fallback target “${target}”使用了 shadowed provider “${targetProvider}”。`);
      }
      if (isUnavailableModelSelector(target, availableModels)) {
        errors.push(`Fallback target “${target}”使用了当前 OMP 不可用的模型。`);
      }
      if (chain.selectors.length === 0) {
        errors.push(`Fallback target “${target || chainIndex + 1}”至少需要一个候选模型。`);
        return;
      }
      const selectors = new Set<string>();
      chain.selectors.forEach((rawSelector, selectorIndex) => {
        const selector = rawSelector.trim();
        if (!selector) {
          errors.push(`Fallback target “${target || chainIndex + 1}”的第 ${selectorIndex + 1} 个候选为空。`);
          return;
        }
        if (selectors.has(selector)) {
          errors.push(`Fallback target “${target}”重复使用 “${selector}”。`);
        }
        selectors.add(selector);
        if (selector === target) errors.push(`Fallback target “${target}”不能回退到自身。`);
        const primary = roleAssignments.get(target);
        if (primary && splitSelector(primary).base === splitSelector(selector).base) {
          errors.push(`Fallback target “${target}”不能回退到当前主模型。`);
        }
        const provider = providerForSelector(selector);
        if (provider && shadowedProviders.includes(provider)) {
          errors.push(`Fallback target “${target}”使用了 shadowed provider “${provider}”。`);
        }
        if (isUnavailableModelSelector(selector, availableModels)) {
          errors.push(`Fallback target “${target}”使用了当前 OMP 不可用的模型 “${selector}”。`);
        } else if (modelSelectorError(selector, availableModels, true)) {
          errors.push(`Fallback target “${target}”使用了无效模型 selector “${selector}”。`);
        }
      });
    });
    return errors;
  }, [availableModels, customRoles, routingSettings.retryFallbackChains, shadowedProviders, values]);
  const hasApplyErrors =
    hasInvalidSelections || invalidCustomRoles.length > 0 || invalidAgents.length > 0 || invalidAgentOverrides.length > 0 || routingErrors.length > 0;
  const hasPresetErrors = hasInvalidSelections || invalidCustomRoles.length > 0 || invalidAgentOverrides.length > 0 || routingErrors.length > 0;



  const catalogModels = useMemo(() => {
    const normalizedFilter = filter.trim().toLowerCase();
    return (snapshot?.models ?? []).filter(model => {
      const matches = !normalizedFilter || `${model.provider} ${model.id} ${model.name}`.toLowerCase().includes(normalizedFilter);
      return matches && !shadowedProviders.includes(model.provider);
    });
  }, [filter, shadowedProviders, snapshot]);
  const roleNames = useMemo(() => {
    const roles = new Set([
      ...SYSTEM_MODEL_ROLE_IDS,
      ...Object.keys(snapshot?.modelRoles ?? {}),
      ...customRoles.map(customRole => customRole.role)
    ]);
    return [...roles].sort((left, right) => left.localeCompare(right));
  }, [customRoles, snapshot]);
  const agentOverrideNames = useMemo(() => {
    const names = new Set(["reviewer", "security-reviewer"]);
    Object.keys(snapshot?.agentModelOverrides ?? {}).forEach(name => names.add(name));
    Object.keys(agentModelOverrides).forEach(name => names.add(name));
    customAgents.forEach(agent => names.add(agent.name));
    return [...names].sort((left, right) => left.localeCompare(right));
  }, [agentModelOverrides, customAgents, snapshot]);

  const agentRoleOptions = useMemo(() => roleNames.map(role => `@${role}`), [roleNames]);

  const emptyRoleSlots =
    MODEL_ROLE_DEFINITIONS.filter(definition => !values[definition.key]).length
    + customRoles.filter(customRole => !customRole.value).length;

  const updateValue = (key: SlotKey, value: string) => {
    setSaved(false);
    setValues(current => ({ ...current, [key]: value }));
  };
  const updateAgentOverride = (agentName: string, value: string) => {
    setSaved(false);
    setAgentModelOverrides(current => ({ ...current, [agentName]: value }));
    if (agentName === "reviewer") {
      setReviewerFollowsSlow(false);
      setValues(current => ({ ...current, reviewer: value }));
    } else if (agentName === "security-reviewer") {
      setSecurityReviewerFollowsSlow(false);
      setValues(current => ({ ...current, securityReviewer: value }));
    }
  };

  const updateRoutingSettings = (next: RoutingSettings) => {
    setSaved(false);
    setRoutingSettings(cloneRoutingSettings(next));
  };

  const updateCustomRoleValue = (role: string, value: string) => {
    setSaved(false);
    setCustomRoles(current =>
      current.map(customRole => customRole.role === role ? { ...customRole, value } : customRole)
    );
  };
  const removeCustomRole = (role: string) => {
    if (pendingDeleteCustomRole !== role) {
      setPendingDeleteCustomRole(role);
      return;
    }
    const alias = `@${role}`;
    const references = [
      ...SYSTEM_MODEL_ROLE_IDS
        .filter(systemRole => values[systemRole].trim() === alias)
        .map(systemRole => `modelRoles.${systemRole}`),
      ...Object.entries(agentModelOverrides)
        .filter(([, value]) => value.trim() === alias)
        .map(([agentName]) => `task.agentModelOverrides.${agentName}`),
      ...customRoles
        .filter(customRole => customRole.role !== role && customRole.value.trim() === alias)
        .map(customRole => `modelRoles.${customRole.role}`),
      ...customAgents
        .filter(agent => agent.model.trim() === alias)
        .map(agent => `Agent ${agent.name}`),
      ...routingSettings.retryFallbackChains.flatMap(chain => [
        ...(chain.target.trim() === role ? [`fallback target ${role}`] : []),
        ...chain.selectors
          .filter(selector => selector.trim() === alias)
          .map(() => `fallback candidate ${alias}`)
      ])
    ];
    if (references.length > 0) {
      setPendingDeleteCustomRole("");
      setError(`不能移除 Role “${role}”；仍被 ${references.join("、")} 引用。`);
      return;
    }

    setCustomRoles(current => current.filter(customRole => customRole.role !== role));
    if (snapshot && Object.hasOwn(snapshot.modelRoles, role)) {
      setRemovedModelRoles(current => current.includes(role) ? current : [...current, role]);
    }
    setPendingDeleteCustomRole("");
    setSaved(false);
  };


  const startCustomRole = () => {
    const firstModel = catalogModels[0]?.selector ?? "";
    setRoleEditor({ role: "", value: firstModel });
  };

  const saveCustomRoleDraft = () => {
    if (
      !roleEditor
      || !CUSTOM_ROLE_PATTERN.test(roleEditor.role)
      || Object.hasOwn(SYSTEM_MODEL_ROLE_LOOKUP, roleEditor.role)
      || !roleEditor.value.trim()
    ) return;
    setCustomRoles(current => {
      if (current.some(item => item.role === roleEditor.role)) return current;
      return [...current, roleEditor].sort((left, right) => left.role.localeCompare(right.role));
    });
    setRemovedModelRoles(current => current.filter(role => role !== roleEditor.role));
    setPendingDeleteCustomRole("");
    setRoleEditor(null);
    setSaved(false);
  };

  const startCustomProvider = () => {
    setError("");
    setCustomProviderModelError("");
    setCustomProviderSaveError("");
    setCustomProviderModelLoading(false);
    setCustomProviderEditor({
      mode: "create",
      originalId: "",
      id: "",
      baseUrl: "",
      api: "openai-completions",
      apiKey: "",
      apiKeyConfigured: false,
      authNone: false,
      models: [],
      modelApiOverrides: {},
      availableModels: []
    });
  };

  const discoverProviderModels = async (editor: CustomProviderEditor) => {
    setCustomProviderModelLoading(true);
    setCustomProviderModelError("");
    try {
      const models = sortedCustomProviderModels(await discoverCustomProviderModels({
        providerId: editor.id.trim(),
        baseUrl: editor.baseUrl.trim(),
        api: editor.api,
        apiKey: editor.apiKey.trim() || null
      }));
      setCustomProviderEditor(current => {
        if (
          !current
          || current.mode !== editor.mode
          || current.originalId !== editor.originalId
          || current.id !== editor.id
          || current.baseUrl !== editor.baseUrl
          || current.api !== editor.api
          || current.apiKey !== editor.apiKey
        ) {
          return current;
        }
        const modelIds = new Set(models.map(model => model.id));
        const staleSelectedModels = current.models
          .filter(id => !modelIds.has(id))
          .map(id => ({ id, name: id }));
        const modelApiOverrides = { ...current.modelApiOverrides };
        models.forEach(model => {
          if (model.api && model.api !== current.api && !Object.hasOwn(modelApiOverrides, model.id)) {
            modelApiOverrides[model.id] = model.api;
          }
        });
        return {
          ...current,
          modelApiOverrides,
          availableModels: sortedCustomProviderModels([...models, ...staleSelectedModels])
        };
      });
      if (models.length === 0) {
        setCustomProviderModelError("provider 返回了空的 model 列表。");
      }
    } catch (discoveryError) {
      setCustomProviderModelError(readableError(discoveryError));
    } finally {
      setCustomProviderModelLoading(false);
    }
  };

  const persistCustomProviders = async (
    providers: CustomProvider[],
    removedProviderIds: string[],
    updatedApiKey?: { id: string; value: string }
  ): Promise<boolean> => {
    setCustomProviderSaving(true);
    setCustomProviderSaveError("");
    try {
      const savedProviders = await saveCustomProviders({
        customProviders: providers.map(provider => ({
          id: provider.id,
          baseUrl: provider.baseUrl,
          api: provider.api,
          apiKey: updatedApiKey?.id === provider.id && updatedApiKey.value ? updatedApiKey.value : null,
          models: provider.models.map(model => ({
            id: model.id,
            api: model.api?.trim() || provider.api,
            thinking: model.thinking ?? [],
            defaultThinkingLevel: model.defaultThinkingLevel ?? null
          }))
        })),
        removedCustomProviders: removedProviderIds
      });
      setCustomProviders(savedProviders);
      setSnapshot(current => current ? { ...current, customProviders: savedProviders } : current);
      return true;
    } catch (saveError) {
      const message = readableError(saveError);
      setCustomProviderSaveError(message);
      setError(message);
      return false;
    } finally {
      setCustomProviderSaving(false);
    }
  };

  const editCustomProvider = (provider: CustomProvider) => {
    setError("");
    setCustomProviderModelError("");
    setCustomProviderSaveError("");
    setCustomProviderModelLoading(false);
    const availableModels = sortedCustomProviderModels(provider.models);
    const api = inferredCustomProviderApi(
      provider.api,
      availableModels,
      CUSTOM_PROVIDER_API_OPTIONS[0].value
    );
    setCustomProviderEditor({
      mode: "edit",
      originalId: provider.id,
      id: provider.id,
      baseUrl: provider.baseUrl,
      api,
      apiKey: "",
      apiKeyConfigured: provider.apiKeyConfigured,
      authNone: provider.authNone,
      models: availableModels.map(model => model.id),
      modelApiOverrides: customProviderApiOverrides(availableModels, api),
      availableModels
    });
  };

  const saveCustomProvider = async () => {
    if (!customProviderEditor || customProviderSaving) return;
    const editor = customProviderEditor;
    const id = editor.id.trim();
    const selectedModels = new Set(editor.models);
    const duplicate = editor.mode === "create" && customProviders.some(provider => provider.id === id);
    const validationError = !id
      ? "请输入 provider ID。"
      : !CUSTOM_PROVIDER_PATTERN.test(id)
        ? "Provider ID 格式无效。"
        : duplicate
          ? "该 provider 已存在。"
          : !/^https?:\/\/\S+$/i.test(editor.baseUrl.trim())
            ? "Base URL 必须以 http:// 或 https:// 开头。"
            : !CUSTOM_PROVIDER_API_OPTIONS.some(option => option.value === editor.api)
              ? "请选择 OMP 支持的 Type。"
              : editor.models.some(modelId => {
                  const api = editor.modelApiOverrides[modelId]?.trim() || editor.api;
                  return !CUSTOM_PROVIDER_API_OPTIONS.some(option => option.value === api);
                })
                ? "请选择每个 model 支持的 Type。"
                : !editor.apiKey.trim() && !editor.apiKeyConfigured && !editor.authNone
                  ? "请输入 API key。"
                  : editor.availableModels.length === 0
                    ? "请先获取 provider 返回的 model 列表。"
                    : editor.models.length === 0
                      ? "至少勾选一个 model。"
                      : "";
    if (validationError) {
      setCustomProviderSaveError(validationError);
      return;
    }
    const missingReferences = [...new Set([
      ...selectedModelSlots
        .filter(item => providerForSelector(item.value) === id)
        .filter(item => {
          const modelId = modelIdForProviderSelector(item.value, id);
          return modelId === "*" || !selectedModels.has(modelId);
        })
        .map(item => item.label),
      ...customAgents
        .filter(agent => providerForSelector(agent.model) === id)
        .filter(agent => {
          const modelId = modelIdForProviderSelector(agent.model, id);
          return modelId === "*" || !selectedModels.has(modelId);
        })
        .map(agent => `Agent ${agent.name}`)
    ])];
    if (missingReferences.length > 0) {
      setCustomProviderSaveError(`不能取消使用 provider “${id}”的 model；仍被 ${missingReferences.join("、")} 引用。`);
      return;
    }

    const apiKey = editor.apiKey.trim();
    const availableById = new Map(editor.availableModels.map(model => [model.id, model]));
    const nextProvider: CustomProvider = {
      id,
      baseUrl: editor.baseUrl.trim(),
      api: editor.api,
      apiKeyConfigured: Boolean(apiKey) || editor.apiKeyConfigured,
      authNone: editor.authNone && !apiKey,
      models: sortedCustomProviderModels(
        editor.models.map(modelId => {
          const model = availableById.get(modelId) ?? { id: modelId, name: modelId };
          return {
            ...model,
            api: editor.modelApiOverrides[modelId]?.trim() || editor.api
          };
        })
      )
    };
    const nextProviders = editor.mode === "create"
      ? [...customProviders, nextProvider].sort((left, right) => left.id.localeCompare(right.id))
      : customProviders.map(provider => provider.id === editor.originalId ? nextProvider : provider);
    const saved = await persistCustomProviders(
      nextProviders,
      [],
      apiKey ? { id, value: apiKey } : undefined
    );
    if (!saved) return;
    setPendingDeleteCustomProvider("");
    setCustomProviderEditor(null);
    setError("");
  };

  const removeCustomProvider = async (id: string) => {
    if (customProviderSaving) return;
    if (pendingDeleteCustomProvider !== id) {
      setPendingDeleteCustomProvider(id);
      return;
    }
    const references = [...new Set([
      ...selectedModelSlots
        .filter(item => providerForSelector(item.value) === id)
        .map(item => item.label),
      ...customAgents
        .filter(agent => providerForSelector(agent.model) === id)
        .map(agent => `Agent ${agent.name}`)
    ])];
    if (references.length > 0) {
      setPendingDeleteCustomProvider("");
      setError(`不能移除 provider “${id}”；仍被 ${references.join("、")} 使用。`);
      return;
    }
    const saved = await persistCustomProviders(
      customProviders.filter(provider => provider.id !== id),
      [id]
    );
    if (!saved) return;
    setPendingDeleteCustomProvider("");
    setCustomProviderEditor(null);
    setError("");
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
    const model = firstModelSelector(frontmatterField(agentEditor.agentMarkdown, "model"));
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
    const agentName = key === "reviewer" ? "reviewer" : "security-reviewer";
    if (key === "reviewer") setReviewerFollowsSlow(follow);
    else setSecurityReviewerFollowsSlow(follow);
    if (follow) {
      setAgentModelOverrides(current => ({ ...current, [agentName]: "" }));
      setValues(current => ({ ...current, [key]: current.slow }));
      return;
    }
    const currentOverride = agentModelOverrides[agentName]?.trim() ?? "";
    const explicitValue = currentOverride && currentOverride !== "@slow"
      ? currentOverride
      : values.slow || values.default || availableModels[0]?.selector || "@default";
    setAgentModelOverrides(current => ({ ...current, [agentName]: explicitValue }));
    setValues(current => ({ ...current, [key]: explicitValue }));
  };

  const toggleProvider = (provider: string, shadow: boolean) => {
    setSaved(false);
    if (!shadow) {
      setShadowedProviders(current => current.filter(item => item !== provider));
      return;
    }
    if (shadowedProviders.includes(provider)) return;
    const nextShadowed = [...shadowedProviders, provider].sort((left, right) => left.localeCompare(right));
    setShadowedProviders(nextShadowed);
    const reassigned = reassignAwayFromShadowedProviders(nextShadowed, values, customRoles, routingSettings, availableModels);
    if (reassigned.changed) {
      setValues(reassigned.values);
      setCustomRoles(reassigned.customRoles);
      setRoutingSettings(reassigned.routingSettings);
    }
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
    setAgentModelOverrides(current => {
      const next = preset.agentModelOverrides ? { ...preset.agentModelOverrides } : { ...current };
      if (!customAgents.some(agent => agent.name === "reviewer")) {
        next.reviewer = preset.reviewerFollowsSlow ? "" : preset.values.reviewer;
      }
      if (!customAgents.some(agent => agent.name === "security-reviewer")) {
        next["security-reviewer"] = preset.securityReviewerFollowsSlow ? "" : preset.values.securityReviewer;
      }
      return next;
    });

    if (preset.routingSettings) setRoutingSettings(cloneRoutingSettings(preset.routingSettings));
    const visibleProviders = new Set((snapshot?.models ?? []).map(model => model.provider));
    setShadowedProviders(
      preset.shadowedProviders
        ? [...new Set(preset.shadowedProviders.filter(provider => visibleProviders.has(provider)))]
            .sort((left, right) => left.localeCompare(right))
        : []
    );
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
        : `已载入路由草稿：“${preset.name}”。`
    );
  };

  const savePresetOverride = () => {
    const selected = presets.find(preset => preset.id === selectedPresetId);
    if (!selected) {
      setPresetMessage("先选择一个预设。");
      return;
    }
    if (hasPresetErrors) {
      setPresetMessage("存在非法路由选择，修复后才能保存。");
      return;
    }
    const customRoleValues = Object.fromEntries(customRoles.map(customRole => [customRole.role, customRole.value]));
    const nextPreset: Preset = {
      ...selected,
      values: { ...values },
      customRoleValues,
      reviewerFollowsSlow,
      securityReviewerFollowsSlow,
      agentModelOverrides: { ...agentModelOverrides },

      routingSettings: cloneRoutingSettings(routingSettings),
      shadowedProviders: [...shadowedProviders],
      updatedAt: new Date().toISOString()
    };
    persistPresetList(presets.map(preset => (preset.id === selected.id ? nextPreset : preset)));
    setPresetName("");
    setPresetMessage(`已覆盖：“${selected.name}”。`);
  };

  const savePresetAs = () => {
    if (hasPresetErrors) {
      setPresetMessage("存在非法路由选择，修复后才能保存。");
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
      agentModelOverrides: { ...agentModelOverrides },

      routingSettings: cloneRoutingSettings(routingSettings),
      shadowedProviders: [...shadowedProviders],
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
    modelRoles: Object.fromEntries([
      ...SYSTEM_MODEL_ROLE_IDS.map(role => [role, values[role]]),
      ...customRoles.map(customRole => [customRole.role, customRole.value])
    ]),
    removedModelRoles: [...removedModelRoles],
    agentModelOverrides: {
      ...agentModelOverrides,
      reviewer: customAgents.some(agent => agent.name === "reviewer")
        ? agentModelOverrides.reviewer ?? ""
        : reviewerFollowsSlow ? "" : values.reviewer,
      "security-reviewer": customAgents.some(agent => agent.name === "security-reviewer")
        ? agentModelOverrides["security-reviewer"] ?? ""
        : securityReviewerFollowsSlow ? "" : values.securityReviewer
    },
    agentFiles: customAgents
      .filter(agent => agent.dirty)
      .map(agent => ({ fileName: agent.fileName, agentMarkdown: agent.agentMarkdown })),
    routingSettings: cloneRoutingSettings(routingSettings),
    ompPath: customOmpPath.trim() || null
  };
  const deletePending = Boolean(selectedPresetId) && pendingDeletePresetId === selectedPresetId;


  const applyConfig = async () => {
    if (hasApplyErrors) {
      setError(
        routingErrors.length
          ? routingErrors[0]
          : invalidCustomRoles.length
            ? "自定义模型 Role 缺少有效模型。"
            : invalidAgentMarkdown.length
              ? "存在无效 Agent Markdown。请打开对应 Agent 修复。"
              : invalidAgentOverrides.length
                ? "存在非法 Agent 模型覆盖。请更换 Model 或 Role。"
                : hasUnavailableSelections || hasUnavailableAgents || hasUnavailableAgentOverrides
                  ? "存在非法选择：已配置模型不在 OMP 当前可用列表中。请更换 Model。"
                  : "存在非法选择：shadowed provider 不能应用。请勾选 Use 或更换 Model。"
      );
      return;
    }

    setSaving(true);
    setError("");
    setSaved(false);
    try {
      const next = await saveSnapshot(request);
      applySnapshot(next, shadowedProviders);
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
        <h1>无法读取 OMP 模型目录</h1>
        <p>{error}</p>
        <p className="muted">请指定本机的 omp 可执行文件完整路径（如 ~/.bun/bin/omp 或 /usr/local/bin/omp）并重新读取：</p>
        <div className="path-recovery-form">
          <input
            type="text"
            className="path-input"
            value={customOmpPathDraft}
            onChange={event => setCustomOmpPathDraft(event.target.value)}
            placeholder="输入 omp 完整路径，例如：~/.bun/bin/omp"
            aria-label="omp 可执行文件完整路径"
          />
          <button
            className="button primary"
            onClick={() => void refresh(customOmpPathDraft)}
          >
            使用指定路径重新读取
          </button>
        </div>
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
          <div className="tool-switcher">
            <button className="tool-tab is-active">OMP</button>
            <button className="tool-tab" onClick={onSwitchTool}>Pi</button>
          </div>
          <div className="config-status">
            <span className="status-dot" />
            {snapshot?.configPath ?? "~/.omp/agent/config.yml"}
          </div>
          <button
            className="button quiet"
            onClick={() => void openEditableFile("config")}
            disabled={saving}
            title="使用系统默认编辑器打开 config.yml"
          >
            直接编辑
          </button>
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
            title={hasApplyErrors ? "存在非法模型、路由策略或 Agent Markdown" : "应用模型与 Agent 路由配置"}
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
            <p className="preset-subtitle">保存 Role、路由策略和 shadow provider 状态；不复制 Agent Markdown</p>

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
                disabled={!selectedPresetId || hasPresetErrors}
              >
                Save
              </button>
              <button
                className="button compact"
                onClick={savePresetAs}
                disabled={hasPresetErrors || !presetName.trim()}
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
              {presetMessage || "选择预设载入完整路由草稿；Save 覆盖，Save as 新建。Agent Markdown 始终独立。"}
            </p>
          </section>

          <section className="provider-card custom-provider-card">
            <div className="custom-provider-heading">
              <div className="catalog-heading"><span>CUSTOM PROVIDERS</span><strong>{customProviders.length}</strong></div>
              <div className="custom-provider-heading-actions">
                <button
                  className="button compact quiet"
                  onClick={() => void openEditableFile("models")}
                  disabled={customProviderSaving}
                  title="使用系统默认编辑器打开 models.yml"
                >
                  直接编辑
                </button>
                <button className="button compact" onClick={startCustomProvider} disabled={customProviderSaving}>
                  新增 Provider
                </button>
              </div>
            </div>
            <p className="provider-help">Provider 保存后立即写入 ~/.omp/agent/models.yml，不需要窗口右上角的 Apply。也可以直接编辑文件，保存后点击“重新读取”。API key 不会从已有文件回显；留空表示保留原 key。</p>
            <div className="custom-provider-list">
              {customProviders.map(provider => (
                <div className="custom-provider-row" key={provider.id}>
                  <div className="custom-provider-summary">
                    <span className="custom-provider-name">{provider.id}</span>
                    <span className="custom-provider-meta">
                      {provider.api || "type missing"} · {provider.models.length} models ·
                      {" "}{provider.apiKeyConfigured ? "API key configured" : provider.authNone ? "keyless" : "API key missing"}
                    </span>
                  </div>
                  <div className="custom-provider-actions">
                    <button
                      className="button compact"
                      onClick={() => editCustomProvider(provider)}
                      disabled={customProviderSaving}
                    >
                      编辑
                    </button>
                    <button
                      className="button compact danger"
                      onClick={() => void removeCustomProvider(provider.id)}
                      disabled={customProviderSaving}
                      aria-label={pendingDeleteCustomProvider === provider.id ? `Confirm delete ${provider.id}` : `Delete ${provider.id}`}
                    >
                      {pendingDeleteCustomProvider === provider.id ? "确认删除" : "删除"}
                    </button>
                  </div>
                </div>
              ))}
              {customProviders.length === 0 && (
                <div className="empty-state">尚未配置 custom provider。</div>
              )}
            </div>
          </section>

          <section className="provider-card">
            <div className="catalog-heading"><span>PROVIDER LIST</span><strong>{providerNames.length}</strong></div>
            <p className="provider-help">列表来自 OMP 当前可用模型（`omp models --json` 已按 disabledProviders 过滤）；取消 Use 只在当前草稿中 shadow 该 provider，并把受影响 Role 切换到各自 fallback chain 中下一个可用候选。Agent 或 Agent override 若仍引用该 provider 会标红，需要手动更换。Apply 只写入 Role、路由策略和 Agent，不会修改 OMP 的 disabledProviders。</p>
            <div className="provider-key">
              <span><i className="key-mark used-mark" /> Used by routes</span>
              <span><i className="key-mark shadowed-mark" /> Shadowed · draft only</span>
            </div>
            <div className="provider-list">
              {providerNames.map(provider => {
                const isShadowed = shadowedProviders.includes(provider);
                const selected = selectedModelSlots.filter(item => providerForSelector(item.value) === provider);
                const isInvalid = selected.length > 0 && isShadowed;
                return (
                  <div className={`provider-row ${isShadowed ? "is-shadowed" : ""} ${isInvalid ? "is-invalid" : ""}`} key={provider}>
                    <span className="provider-name">{provider}</span>
                    {selected.length ? (
                      <span className="provider-selected">Used · {selected.length}</span>
                    ) : (
                      <span className="provider-state">{isShadowed ? "shadowed" : "available"}</span>
                    )}
                    <label className="provider-use-control">
                      <input
                        type="checkbox"
                        checked={!isShadowed}
                        onChange={event => toggleProvider(provider, !event.currentTarget.checked)}
                        aria-label={`${isShadowed ? "Unshadow" : "Shadow"} ${provider}`}
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
                    <span className="catalog-model-meta">
                      {model.contextWindow !== null ? formatTokenLimit(model.contextWindow) : "—"}
                      {model.cost ? ` · $${formatPrice(model.cost.input)}/$${formatPrice(model.cost.output)}` : ""}
                    </span>
                    {selected.length > 0 && <span className="model-selected-badge">{selected.map(item => item.label).join(" · ")}</span>}
                  </div>
                );
              })}
              {catalogModels.length === 0 && <div className="empty-state">当前筛选没有可见模型。</div>}
            </div>
            <p>已选模型高亮显示；shadowed provider 的模型不在列表中。</p>
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
                shadowedProviders={shadowedProviders}
                reviewerFollowsSlow={reviewerFollowsSlow}
                securityReviewerFollowsSlow={securityReviewerFollowsSlow}
                slowValue={values.slow}
                onChange={value => updateValue(definition.key, value)}
                onFollowChange={updateReviewerMode}
              />
            ))}
          </div>
          <RoutingPolicySection
            settings={routingSettings}
            models={snapshot?.models ?? []}
            shadowedProviders={shadowedProviders}
            roleNames={roleNames}
            errors={routingErrors}
            onChange={updateRoutingSettings}
          />



          <section className="agent-override-section">
            <div className="custom-role-heading">
              <div>
                <span className="section-kicker">AGENT MODEL OVERRIDES</span>
                <h2>Agent 模型覆盖</h2>
                <p>按已发现 Agent 与现有 task.agentModelOverrides 动态显示；留空表示使用 Agent 自身的 model 定义。</p>
              </div>
              <strong>{agentOverrideNames.length}</strong>
            </div>
            <div className="slot-list">
              {agentOverrideNames.map(agentName => {
                const customAgent = customAgents.find(agent => agent.name === agentName);
                const slowFollowAgent = !customAgent && agentName === "reviewer"
                  ? "reviewer"
                  : !customAgent && agentName === "security-reviewer"
                    ? "security-reviewer"
                    : undefined;
                const followsSlow = slowFollowAgent === "reviewer"
                  ? reviewerFollowsSlow
                  : slowFollowAgent === "security-reviewer"
                    ? securityReviewerFollowsSlow
                    : false;
                const inheritedModel = slowFollowAgent ? values.slow : customAgent?.model ?? "";
                const override = agentModelOverrides[agentName] ?? "";
                return (
                  <AgentOverrideCard
                    key={agentName}
                    name={agentName}
                    override={override}
                    inheritedModel={inheritedModel}
                    models={snapshot?.models ?? []}
                    filter={filter}
                    shadowedProviders={shadowedProviders}
                    followsSlow={followsSlow}
                    slowFollowAgent={slowFollowAgent}
                    onChange={value => updateAgentOverride(agentName, value)}
                    onFollowChange={slowFollowAgent
                      ? follow => updateReviewerMode(slowFollowAgent === "reviewer" ? "reviewer" : "securityReviewer", follow)
                      : undefined}
                    onEnable={() => updateAgentOverride(agentName, customAgent?.model || "@slow")}
                  />
                );
              })}
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
                    shadowedProviders={shadowedProviders}
                    deletePending={pendingDeleteCustomRole === customRole.role}
                    onChange={value => updateCustomRoleValue(customRole.role, value)}
                    onDelete={() => removeCustomRole(customRole.role)}
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
                    effectiveModel={effectiveAgentModel(agent, agentModelOverrides)}
                    models={snapshot?.models ?? []}
                    shadowedProviders={shadowedProviders}
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
                预设保存模型与路由草稿。Apply 会写入 modelRoles、routing policy 和已编辑的 Agent 文件；Skill 不属于本工具范围。
              </span>
            </div>
          </div>
          {error && <div className="inline-error">{error}</div>}
        </main>
      </div>
      {customProviderEditor && (
        <CustomProviderModal
          apiOptions={CUSTOM_PROVIDER_API_OPTIONS}
          productName="OMP"
          destinationPath="~/.omp/agent/models.yml"
          requireApiKey
          editor={customProviderEditor}
          existingProviderIds={customProviders.map(provider => provider.id)}
          discovering={customProviderModelLoading}
          discoveryError={customProviderModelError}
          saving={customProviderSaving}
          saveError={customProviderSaveError}
          onChange={editor => {
            setCustomProviderSaveError("");
            setCustomProviderEditor(editor);
          }}
          onDiscover={() => {
            if (customProviderEditor) void discoverProviderModels(customProviderEditor);
          }}
          onCancel={() => {
            if (!customProviderSaving) setCustomProviderEditor(null);
          }}
          onSave={() => void saveCustomProvider()}
        />
      )}
      {roleEditor && (
        <CustomRoleModal
          editor={roleEditor}
          models={snapshot?.models ?? []}
          filter={filter}
          shadowedProviders={shadowedProviders}
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
