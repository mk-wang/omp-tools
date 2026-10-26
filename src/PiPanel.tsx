import { useEffect, useMemo, useState } from "react";
import {
  discoverPiCustomProviderModels,
  loadPiSnapshot,
  openPiModelsFile,
  savePiCustomProviders,
  savePiTiers
} from "./piApi";
import {
  CUSTOM_PROVIDER_PATTERN,
  CustomProviderModal,
  customProviderApiOverrides,
  customProviderApiOverride,
  inferredCustomProviderApi,
  mergeDiscoveredModels,
  PI_CUSTOM_PROVIDER_API_OPTIONS,
  sortedCustomProviderModels,
  type CustomProviderEditor
} from "./CustomProviderModal";
import type {
  CustomProvider,
  CustomProviderSaveRequest,
  PiModelOption,
  PiSnapshot,
  PiTiersFile
} from "./types";

type Theme = "light" | "dark";

const PI_PATH_STORAGE_KEY = "omp-model-studio.pi-path";
// Same key App.tsx uses. Both panels own their theme independently (never
// mounted at the same time), so sharing the storage key just keeps the
// picked theme visually consistent across a tool switch without either
// panel importing anything from the other.
const THEME_STORAGE_KEY = "omp-model-studio.theme";
const TIER_NAME_PATTERN = /^[a-z][a-z0-9_-]*$/;
const THINKING_LEVELS = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];
const EMPTY_TIERS: PiTiersFile = { default: "", tiers: {}, assignments: {}, disabledAgents: [], maxRecursionDepth: 2 };

function splitKnownThinkingSuffix(value: string): { baseModel: string; thinking: string } {
  const colonIndex = value.lastIndexOf(":");
  if (colonIndex === -1) return { baseModel: value, thinking: "" };
  const suffix = value.slice(colonIndex + 1);
  return THINKING_LEVELS.includes(suffix)
    ? { baseModel: value.slice(0, colonIndex), thinking: suffix }
    : { baseModel: value, thinking: "" };
}

function baseModelOf(value: string): string {
  return splitKnownThinkingSuffix(value).baseModel;
}

function thinkingLevelOf(value: string): string {
  return splitKnownThinkingSuffix(value).thinking;
}

function modelSpecFor(value: string, thinking: string): string {
  const baseModel = baseModelOf(value).trim();
  return baseModel && thinking ? `${baseModel}:${thinking}` : baseModel;
}

function isValidModelSpec(value: string): boolean {
  const baseModel = baseModelOf(value).trim();
  return Boolean(
    thinkingLevelOf(value)
      && baseModel.includes("/")
      && !baseModel.startsWith("/")
      && !baseModel.endsWith("/")
  );
}

function cloneTiers(data: PiTiersFile): PiTiersFile {
  return {
    default: data.default,
    tiers: Object.fromEntries(Object.entries(data.tiers).map(([key, tier]) => [
      key,
      { model: tier.model, fallbackModels: [...tier.fallbackModels] }
    ])),
    assignments: Object.fromEntries(Object.entries(data.assignments).map(([key, agents]) => [key, [...agents]])),
    disabledAgents: [...data.disabledAgents],
    maxRecursionDepth: data.maxRecursionDepth
  };
}

function providerOf(selector: string): string {
  const baseModel = baseModelOf(selector);
  return baseModel.includes("/") ? baseModel.split("/")[0] : "";
}

function thinkingOptionsForModel(selector: string, models: PiModelOption[]): string[] {
  const model = models.find(candidate => candidate.selector === baseModelOf(selector));
  if (!model || !model.reasoning) return ["off"];
  const levels = model.thinking?.length ? model.thinking : THINKING_LEVELS;
  return [...new Set(["off", ...levels])];
}

function preferredThinkingForModel(
  selector: string,
  models: PiModelOption[],
  inheritedThinking = ""
): string {
  const options = thinkingOptionsForModel(selector, models);
  const current = thinkingLevelOf(selector) || inheritedThinking;
  if (current && options.includes(current)) return current;
  const model = models.find(candidate => candidate.selector === baseModelOf(selector));
  const defaultThinking = model?.defaultThinkingLevel;
  return defaultThinking && options.includes(defaultThinking)
    ? defaultThinking
    : options.includes("medium")
      ? "medium"
      : options[0] ?? "off";
}

function PiModelSelect({
  value,
  models,
  shadowedProviders,
  onChange,
  disabled = false
}: {
  value: string;
  models: PiModelOption[];
  shadowedProviders: string[];
  onChange: (value: string) => void;
  disabled?: boolean;
}) {
  const shadowedSet = new Set(shadowedProviders);
  const baseValue = baseModelOf(value);
  const currentProvider = providerOf(baseValue);
  const providers = useMemo(() => {
    const names = new Set(models.filter(model => !shadowedSet.has(model.provider)).map(model => model.provider));
    if (currentProvider) names.add(currentProvider);
    return [...names].sort((left, right) => left.localeCompare(right));
  }, [models, shadowedProviders, currentProvider]);
  const selectedProvider = currentProvider || providers[0] || "";
  const providerModels = models.filter(model => model.provider === selectedProvider);
  const selectedModel = providerModels.find(model => model.selector === baseValue);

  return (
    <div className="picker-grid pi-model-select">
      <label className="field-group">
        <span>Provider</span>
        <select
          value={selectedProvider}
          disabled={disabled || providers.length === 0}
          onChange={event => {
            const nextProvider = event.target.value;
            const firstModel = models.find(model => model.provider === nextProvider);
            const nextSelector = firstModel?.selector ?? `${nextProvider}/`;
            onChange(modelSpecFor(
              nextSelector,
              preferredThinkingForModel(nextSelector, models, thinkingLevelOf(value))
            ));
          }}
        >
          {providers.length === 0 && <option value="">No providers</option>}
          {providers.map(provider => (
            <option value={provider} key={provider}>
              {provider}{shadowedSet.has(provider) ? " · shadowed" : ""}
            </option>
          ))}
        </select>
      </label>
      <label className="field-group model-field">
        <span>Model</span>
        <select
          value={baseValue}
          disabled={disabled || providerModels.length === 0}
          onChange={event => {
            const nextBase = event.target.value;
            onChange(modelSpecFor(
              nextBase,
              preferredThinkingForModel(nextBase, models, thinkingLevelOf(value))
            ));
          }}
        >
          {!baseValue && providerModels.length === 0 && <option value="">No models</option>}
          {!baseValue && providerModels.length > 0 && <option value="">Select model…</option>}
          {baseValue && !providerModels.some(model => model.selector === baseValue) && (
            <option value={baseValue}>{baseValue.split("/").slice(1).join("/") || baseValue} · unavailable</option>
          )}
          {providerModels.map(model => (
            <option value={model.selector} key={model.selector}>{model.id}</option>
          ))}
        </select>
      </label>
      {selectedModel && (
        <div className="model-metadata">
          {selectedModel.contextWindow !== null && <span>Context {Math.round(selectedModel.contextWindow / 1000)}K</span>}
          {selectedModel.maxTokens !== null && <span>Output {Math.round(selectedModel.maxTokens / 1000)}K</span>}
          {selectedModel.images && <span>Images</span>}
          {selectedModel.thinking?.length && <span>Thinking {selectedModel.thinking.join("/")}</span>}
        </div>
      )}
    </div>
  );
}



function PiThinkingSelect({
  value,
  models,
  onChange,
  disabled = false
}: {
  value: string;
  models: PiModelOption[];
  onChange: (value: string) => void;
  disabled?: boolean;
}) {
  const baseModel = baseModelOf(value);
  const options = thinkingOptionsForModel(baseModel, models);
  const current = thinkingLevelOf(value);
  return (
    <label className="field-group thinking-field">
      <span>Thinking</span>
      <select
        value={current}
        disabled={disabled || !baseModel}
        onChange={event => onChange(modelSpecFor(value, event.target.value))}
      >
        {!current && <option value="">Select thinking…</option>}
        {current && !options.includes(current) && <option value={current}>{current}</option>}
        {options.map(level => <option value={level} key={level}>{level}</option>)}
      </select>
    </label>
  );
}


function readableError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function readTheme(): Theme {
  if (typeof window === "undefined") return "light";
  try {
    return window.localStorage.getItem(THEME_STORAGE_KEY) === "dark" ? "dark" : "light";
  } catch {
    return "light";
  }
}


function tierCardElementId(tierName: string): string {
  return `pi-tier-card-${tierName}`;
}


/**
 * Shadowing a provider moves each affected tier on to the next candidate
 * immediately instead of leaving it flagged for manual repair. The old
 * (now-shadowed) primary is dropped rather than kept as a fallback, matching
 * the OMP-side behavior — the provider is going into pi's own
 * `disabledProviders`, so nothing could ever route to it again until it is
 * explicitly un-shadowed. A tier whose whole fallback list is also shadowed
 * is left with an invalid primary; there is no safe candidate to guess, so it
 * still needs a manual pick.
 */
function reassignTiersAwayFromShadowedProviders(
  shadowed: string[],
  data: PiTiersFile
): { tiers: PiTiersFile; changed: boolean } {
  const shadowedSet = new Set(shadowed);
  const isShadowed = (selector: string) => {
    const provider = providerOf(selector);
    return Boolean(provider) && shadowedSet.has(provider);
  };

  const next = cloneTiers(data);
  let changed = false;

  for (const tier of Object.values(next.tiers)) {
    const strippedFallbacks = tier.fallbackModels.filter(selector => !isShadowed(selector));
    if (strippedFallbacks.length !== tier.fallbackModels.length) changed = true;
    tier.fallbackModels = strippedFallbacks;

    if (isShadowed(tier.model)) {
      const candidate = tier.fallbackModels.shift();
      if (candidate) {
        tier.model = candidate;
        changed = true;
      }
    }
  }

  return { tiers: next, changed };
}

function uniqueTierName(existing: Record<string, unknown>): string {
  let index = existing["new-tier"] ? 2 : 1;
  let candidate = index === 1 ? "new-tier" : `new-tier-${index}`;
  while (Object.hasOwn(existing, candidate)) {
    index += 1;
    candidate = `new-tier-${index}`;
  }
  return candidate;
}


export default function PiPanel({
  onSwitchTool
}: {
  onSwitchTool: () => void;
}) {
  const [snapshot, setSnapshot] = useState<PiSnapshot | null>(null);
  const [tiers, setTiers] = useState<PiTiersFile>(EMPTY_TIERS);
  const [shadowedProviders, setShadowedProviders] = useState<string[]>([]);
  const [customProviders, setCustomProviders] = useState<CustomProvider[]>([]);
  const [customProviderEditor, setCustomProviderEditor] = useState<CustomProviderEditor | null>(null);
  const [customProviderModelLoading, setCustomProviderModelLoading] = useState(false);
  const [customProviderModelError, setCustomProviderModelError] = useState("");
  const [customProviderSaving, setCustomProviderSaving] = useState(false);
  const [customProviderSaveError, setCustomProviderSaveError] = useState("");
  const [pendingDeleteCustomProvider, setPendingDeleteCustomProvider] = useState("");
  const [theme, setTheme] = useState<Theme>(readTheme);
  const [customPiPath, setCustomPiPath] = useState(() => {
    if (typeof window === "undefined") return "";
    try {
      return window.localStorage.getItem(PI_PATH_STORAGE_KEY) ?? "";
    } catch {
      return "";
    }
  });
  const [customPiPathDraft, setCustomPiPathDraft] = useState(customPiPath);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState("");
  const [pendingDeleteTier, setPendingDeleteTier] = useState("");
  const [highlightedTier, setHighlightedTier] = useState("");

  const jumpToTier = (tierName: string) => {
    if (!tierName) return;
    const element = document.getElementById(tierCardElementId(tierName));
    if (!element) return;
    element.scrollIntoView({ behavior: "smooth", block: "start" });
    setHighlightedTier(tierName);
    window.setTimeout(() => setHighlightedTier(current => (current === tierName ? "" : current)), 1600);
  };

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    try {
      window.localStorage.setItem(THEME_STORAGE_KEY, theme);
    } catch {
      // Theme still works for the current session when storage is unavailable.
    }
  }, [theme]);

  const load = async (path?: string) => {
    setLoading(true);
    setError("");
    try {
      const next = await loadPiSnapshot(path);
      setSnapshot(next);
      setTiers(next.tiers);
      setShadowedProviders([...next.shadowedProviders].sort((left, right) => left.localeCompare(right)));
      setCustomProviders(next.customProviders);
      setCustomProviderEditor(null);
      setCustomProviderModelLoading(false);
      setCustomProviderModelError("");
      setCustomProviderSaving(false);
      setCustomProviderSaveError("");
      setPendingDeleteCustomProvider("");
      setSaved(false);
    } catch (loadError) {
      setError(readableError(loadError));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void load(customPiPath || undefined);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const models = snapshot?.models ?? [];
  const selectableModels = useMemo(
    () => models.filter(model => !shadowedProviders.includes(model.provider)),
    [models, shadowedProviders]
  );
  const providers = useMemo(() => {
    // `pi --list-models` excludes providers already in `disabledProviders`, so a
    // shadowed provider would otherwise vanish from this list after Apply and
    // could never be un-shadowed again. Union with the current shadow set and
    // providers configured in models.json.
    const names = new Set(models.map(model => model.provider));
    shadowedProviders.forEach(provider => names.add(provider));
    customProviders.forEach(provider => names.add(provider.id));
    return [...names].sort((left, right) => left.localeCompare(right));
  }, [customProviders, models, shadowedProviders]);
  const agentNames = useMemo(
    () => [...(snapshot?.agentNames ?? [])].sort((left, right) => left.localeCompare(right)),
    [snapshot]
  );
  const tierNames = useMemo(() => Object.keys(tiers.tiers).sort((left, right) => left.localeCompare(right)), [tiers]);
  const roleOwner = useMemo(() => {
    const owner = new Map<string, string>();
    Object.entries(tiers.assignments).forEach(([tierName, agents]) => agents.forEach(agent => owner.set(agent, tierName)));
    return owner;
  }, [tiers]);

  const agentModelSummary = useMemo(() => {
    const disabledSet = new Set(tiers.disabledAgents);
    return agentNames.map(agent => {
      const isDisabled = disabledSet.has(agent);
      const assignedTier = roleOwner.get(agent);
      const isDefault = !assignedTier;
      const tierName = assignedTier ?? tiers.default;
      const tier = tierName ? tiers.tiers[tierName] : undefined;
      return {
        agent,
        tierName: tierName ?? "",
        isDefault,
        isDisabled,
        // disabledAgents wins over any tier assignment (pi-roster DESIGN.md §5.1).
        model: isDisabled ? "" : tier?.model ?? "",
        thinking: isDisabled ? "" : thinkingLevelOf(tier?.model ?? "")
      };
    });
  }, [agentNames, roleOwner, tiers]);

  const toggleDisabledAgent = (agent: string) => {
    const next = cloneTiers(tiers);
    next.disabledAgents = next.disabledAgents.includes(agent)
      ? next.disabledAgents.filter(item => item !== agent)
      : [...next.disabledAgents, agent];
    updateTiers(next);
  };

  const updateMaxRecursionDepth = (raw: string) => {
    const parsed = Number.parseInt(raw, 10);
    if (Number.isNaN(parsed)) return;
    updateTiers({ ...cloneTiers(tiers), maxRecursionDepth: parsed });
  };

  const shadowConflicts = useMemo(() => {
    const conflicts: string[] = [];
    Object.entries(tiers.tiers).forEach(([name, tier]) => {
      if (shadowedProviders.includes(providerOf(tier.model))) {
        conflicts.push(`Tier "${name}" 的主模型使用了 shadowed provider "${providerOf(tier.model)}"`);
      }
      tier.fallbackModels.forEach(selector => {
        if (shadowedProviders.includes(providerOf(selector))) {
          conflicts.push(`Tier "${name}" 的 fallback "${selector}" 使用了 shadowed provider`);
        }
      });
    });
    return conflicts;
  }, [tiers, shadowedProviders]);

  const invalidModelSpecs = tierNames.flatMap(name => {
    const tier = tiers.tiers[name];
    const errors = isValidModelSpec(tier.model)
      ? []
      : [`Tier "${name}" 的主模型必须是 provider/model-id:thinking 格式`];
    tier.fallbackModels.forEach((selector, index) => {
      if (!isValidModelSpec(selector)) errors.push(`Tier "${name}" 的 fallback ${index + 1} 必须是 provider/model-id:thinking 格式`);
    });
    return errors;
  });
  const canApply = tierNames.length > 0 && shadowConflicts.length === 0 && invalidModelSpecs.length === 0 && !saving;

  const updateTiers = (next: PiTiersFile) => {
    setSaved(false);
    setTiers(next);
  };

  const addTier = () => {
    const name = uniqueTierName(tiers.tiers);
    const next = cloneTiers(tiers);
    const model = selectableModels[0]?.selector ?? "";
    next.tiers[name] = {
      model: modelSpecFor(model, preferredThinkingForModel(model, models)),
      fallbackModels: []
    };
    next.assignments[name] = [];
    if (!next.default) next.default = name;
    updateTiers(next);
  };
  const removeTier = (name: string) => {
    if (pendingDeleteTier !== name) {
      setPendingDeleteTier(name);
      return;
    }
    const next = cloneTiers(tiers);
    delete next.tiers[name];
    delete next.assignments[name];
    if (next.default === name) next.default = Object.keys(next.tiers)[0] ?? "";
    setPendingDeleteTier("");
    updateTiers(next);
  };

  const renameTier = (oldName: string, rawName: string) => {
    const newName = rawName.trim().toLowerCase();
    if (newName === oldName) return;
    if (!TIER_NAME_PATTERN.test(newName) || Object.hasOwn(tiers.tiers, newName)) return;
    const next = cloneTiers(tiers);
    next.tiers[newName] = next.tiers[oldName];
    delete next.tiers[oldName];
    next.assignments[newName] = next.assignments[oldName] ?? [];
    delete next.assignments[oldName];
    if (next.default === oldName) next.default = newName;
    updateTiers(next);
  };

  const updateTierModel = (name: string, value: string) => {
    const next = cloneTiers(tiers);
    next.tiers[name].model = value;
    updateTiers(next);
  };

  const updateFallback = (name: string, index: number, value: string) => {
    const next = cloneTiers(tiers);
    next.tiers[name].fallbackModels[index] = value;
    updateTiers(next);
  };

  const addFallback = (name: string) => {
    const next = cloneTiers(tiers);
    const model = selectableModels[0]?.selector ?? "";
    next.tiers[name].fallbackModels.push(modelSpecFor(model, preferredThinkingForModel(model, models)));
    updateTiers(next);
  };

  const removeFallback = (name: string, index: number) => {
    const next = cloneTiers(tiers);
    next.tiers[name].fallbackModels.splice(index, 1);
    updateTiers(next);
  };

  const toggleAgent = (tierName: string, agent: string) => {
    const inTier = (tiers.assignments[tierName] ?? []).includes(agent);
    const next = cloneTiers(tiers);
    Object.keys(next.assignments).forEach(key => {
      next.assignments[key] = next.assignments[key].filter(item => item !== agent);
    });
    if (!inTier) next.assignments[tierName] = [...(next.assignments[tierName] ?? []), agent];
    updateTiers(next);
  };

  const setDefaultTier = (name: string) => {
    updateTiers({ ...cloneTiers(tiers), default: name });
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
    const reassigned = reassignTiersAwayFromShadowedProviders(nextShadowed, tiers);
    if (reassigned.changed) setTiers(reassigned.tiers);
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
      availableModels: [],
      discoveredModelIds: null
    });
  };

  const discoverProviderModels = async (editor: CustomProviderEditor) => {
    setCustomProviderModelLoading(true);
    setCustomProviderModelError("");
    try {
      const models = sortedCustomProviderModels(await discoverPiCustomProviderModels({
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
        const configured = current.mode === "edit"
          ? customProviders.find(provider => provider.id === current.originalId)
          : undefined;
        // Configured leaves seed the merge only while the editor still targets the same endpoint and Type.
        const configuredModels = configured
          && configured.baseUrl.trim() === current.baseUrl.trim()
          && current.api === inferredCustomProviderApi(configured.api, configured.models, PI_CUSTOM_PROVIDER_API_OPTIONS[0].value)
          ? configured.models
          : [];
        const mergedModels = mergeDiscoveredModels([...configuredModels, ...current.availableModels], models);
        const modelApiOverrides = { ...current.modelApiOverrides };
        mergedModels.forEach(model => {
          if (model.api && model.api !== current.api && !Object.hasOwn(modelApiOverrides, model.id)) {
            Object.defineProperty(modelApiOverrides, model.id, { value: model.api, enumerable: true, writable: true, configurable: true });
          }
        });
        return {
          ...current,
          modelApiOverrides,
          discoveredModelIds: models.map(model => model.id),
          availableModels: mergedModels
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
    providersToSave: CustomProvider[],
    removedProviderIds: string[],
    updatedApiKey?: { id: string; value: string }
  ): Promise<boolean> => {
    setCustomProviderSaving(true);
    setCustomProviderSaveError("");
    try {
      const request: CustomProviderSaveRequest = {
        customProviders: providersToSave.map(provider => ({
          id: provider.id,
          baseUrl: provider.baseUrl,
          api: provider.api,
          apiKey: updatedApiKey?.id === provider.id && updatedApiKey.value ? updatedApiKey.value : null,
          models: provider.models.map(model => ({
            id: model.id,
            api: model.api?.trim() || provider.api,
            thinking: model.thinking ?? [],
            defaultThinkingLevel: model.defaultThinkingLevel ?? null,
            configuration: model.configuration
          }))
        })),
        removedCustomProviders: removedProviderIds
      };
      const savedProviders = await savePiCustomProviders(request);
      setCustomProviders(savedProviders);
      setSnapshot(current => current ? { ...current, customProviders: savedProviders } : current);
      await load(customPiPath || undefined);
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
    const configuredModels = sortedCustomProviderModels(provider.models);
    const api = inferredCustomProviderApi(
      provider.api,
      configuredModels,
      PI_CUSTOM_PROVIDER_API_OPTIONS[0].value
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
      models: configuredModels.map(model => model.id),
      modelApiOverrides: customProviderApiOverrides(configuredModels, api),
      availableModels: [],
      discoveredModelIds: null
    });
  };

  const saveCustomProvider = async () => {
    if (!customProviderEditor || customProviderSaving) return;
    const editor = customProviderEditor;
    const id = editor.id.trim();
    const duplicate = editor.mode === "create" && customProviders.some(provider => provider.id === id);
    const validationError = !id
      ? "请输入 provider ID。"
      : !CUSTOM_PROVIDER_PATTERN.test(id)
        ? "Provider ID 格式无效。"
        : duplicate
          ? "该 provider 已存在。"
          : !/^https?:\/\/\S+$/i.test(editor.baseUrl.trim())
            ? "Base URL 必须以 http:// 或 https:// 开头。"
            : !PI_CUSTOM_PROVIDER_API_OPTIONS.some(option => option.value === editor.api)
              ? "请选择 Pi 支持的 Type。"
              : editor.models.some(modelId => {
                  const api = customProviderApiOverride(editor.modelApiOverrides, modelId).trim() || editor.api;
                  return !PI_CUSTOM_PROVIDER_API_OPTIONS.some(option => option.value === api);
                })
                ? "请选择每个 model 支持的 Type。"
                : editor.availableModels.length === 0
                  ? "请先获取 provider 返回的 model 列表。"
                  : editor.models.length === 0
                    ? "至少勾选一个 model。"
                    : "";
    if (validationError) {
      setCustomProviderSaveError(validationError);
      return;
    }


    const apiKey = editor.apiKey.trim();
    const configuredModels = editor.mode === "edit"
      ? customProviders.find(provider => provider.id === editor.originalId)?.models ?? []
      : [];
    // Refreshed metadata wins; configured metadata covers selected models the refresh omitted.
    const availableById = new Map(
      [...configuredModels, ...editor.availableModels].map(model => [model.id, model])
    );
    const nextProvider: CustomProvider = {
      id,
      baseUrl: editor.baseUrl.trim(),
      api: editor.api,
      apiKeyConfigured: Boolean(apiKey) || editor.apiKeyConfigured,
      authNone: false,
      models: sortedCustomProviderModels(
        editor.models.map(modelId => {
          const model = availableById.get(modelId) ?? { id: modelId, name: modelId };
          return {
            ...model,
            api: customProviderApiOverride(editor.modelApiOverrides, modelId).trim() || editor.api
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
  };

  const removeCustomProvider = async (id: string) => {
    if (customProviderSaving) return;
    if (pendingDeleteCustomProvider !== id) {
      setPendingDeleteCustomProvider(id);
      return;
    }
    const references = Object.entries(tiers.tiers).flatMap(([tierName, tier]) => [
      ...(providerOf(tier.model) === id ? [`Tier ${tierName} 主模型`] : []),
      ...tier.fallbackModels
        .filter(selector => providerOf(selector) === id)
        .map(selector => `Tier ${tierName} fallback ${selector}`)
    ]);
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
  };

  const openModelsFile = async () => {
    setError("");
    try {
      await openPiModelsFile();
    } catch (openError) {
      setError(readableError(openError));
    }
  };

  const applyTiers = async () => {
    setSaving(true);
    setError("");
    try {
      const next = await savePiTiers(tiers, shadowedProviders, customPiPath || undefined);
      setSnapshot(next);
      setTiers(next.tiers);
      setShadowedProviders([...next.shadowedProviders].sort((left, right) => left.localeCompare(right)));
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
        <span>正在读取 Pi 配置与模型目录…</span>
      </div>
    );
  }

  if (error && !snapshot) {
    return (
      <div className="error-screen">
        <div className="error-mark">!</div>
        <h1>无法读取 Pi 模型目录</h1>
        <p>{error}</p>
        <p className="muted">请指定本机的 pi 可执行文件完整路径（如 /opt/homebrew/bin/pi）并重新读取：</p>
        <div className="path-recovery-form">
          <input
            type="text"
            className="path-input"
            value={customPiPathDraft}
            onChange={event => setCustomPiPathDraft(event.target.value)}
            placeholder="输入 pi 完整路径"
            aria-label="pi 可执行文件完整路径"
          />
          <button
            className="button primary"
            onClick={() => {
              setCustomPiPath(customPiPathDraft);
              try {
                window.localStorage.setItem(PI_PATH_STORAGE_KEY, customPiPathDraft.trim());
              } catch {
                // Session-only when storage is unavailable.
              }
              void load(customPiPathDraft);
            }}
          >
            使用指定路径重新读取
          </button>
        </div>
        <button className="button quiet" onClick={onSwitchTool}>返回 OMP</button>
      </div>
    );
  }

  return (
    <div className="app-shell">
      <header className="app-header">
        <div className="brand-lockup">
          <div className="brand-mark">π</div>
          <div>
            <div className="brand-title">Pi Roster Tiers</div>
            <div className="brand-subtitle">pi-roster 的 tier 与 provider shadow</div>
          </div>
        </div>
        <div className="header-actions">
          <div className="tool-switcher">
            <button className="tool-tab" onClick={onSwitchTool}>OMP</button>
            <button className="tool-tab is-active">Pi</button>
          </div>
          <div className="config-status">
            <span className="status-dot" />
            {snapshot?.settingsPath ?? "~/.pi/agent/settings.json"}
          </div>
          <button className="button quiet" onClick={() => setTheme(theme === "light" ? "dark" : "light")}>
            {theme === "light" ? "☾ Dark" : "☀ Light"}
          </button>
          <button className="button quiet" onClick={() => void load(customPiPath || undefined)} disabled={saving}>
            重新读取
          </button>
          <button
            className="button"
            onClick={() => void applyTiers()}
            disabled={!canApply}
            title={!canApply ? "存在不完整 model spec 或 shadowed provider 冲突" : "写入 settings.json 的 “pi-roster” 配置块"}
          >
            {saving ? "Applying…" : saved ? "Applied" : "Apply"}
          </button>
        </div>
      </header>

      <div className="content-grid">
        <aside className="sidebar">
          <section className="intro-block">
            <span className="section-kicker">PI PROVIDERS</span>
            <h1>Provider Shadow</h1>
            <p>取消勾选会立即把受影响 tier 切换到各自 fallback 列表中下一个可用候选（没有可用候选的 tier 会保持标红，需要手动选择），Apply 时写入 pi 真实的 disabledProviders（pi 核心设置，对全部模型选择生效，不只是 pi-roster）。</p>
          </section>
          <section className="provider-card custom-provider-card">
            <div className="custom-provider-heading">
              <div className="catalog-heading"><span>PI CUSTOM PROVIDERS</span><strong>{customProviders.length}</strong></div>
              <div className="custom-provider-heading-actions">
                <button
                  className="button compact quiet"
                  onClick={() => void openModelsFile()}
                  disabled={customProviderSaving}
                  title="在 Finder 中显示 models.json"
                >
                  Reveal in Finder
                </button>
                <button className="button compact" onClick={startCustomProvider} disabled={customProviderSaving}>
                  新增 Provider
                </button>
              </div>
            </div>
            <p className="provider-help">Provider 保存后立即写入 ~/.pi/agent/models.json。Pi 只接受 OpenAI Completions、OpenAI Responses、Anthropic Messages 和 Google Generative AI；API key 可由 models.json、auth.json 或环境变量提供。</p>
            <div className="custom-provider-list">
              {customProviders.map(provider => (
                <div className="custom-provider-row" key={provider.id}>
                  <div className="custom-provider-summary">
                    <span className="custom-provider-name">{provider.id}</span>
                    <span className="custom-provider-meta">
                      {provider.api || "type missing"} · {provider.models.length} models ·
                      {" "}{provider.apiKeyConfigured ? "models.json key configured" : "external auth or key missing"}
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
                <div className="empty-state">尚未配置 Pi custom provider。</div>
              )}
            </div>
          </section>
          <section className="legend-card">
            <div className="provider-list">
              {providers.map(provider => {
                const isShadowed = shadowedProviders.includes(provider);
                return (
                  <label className={`provider-row${isShadowed ? " is-shadowed" : ""}`} key={provider}>
                    <input
                      type="checkbox"
                      checked={isShadowed}
                      onChange={event => toggleProvider(provider, event.target.checked)}
                    />
                    <span>{provider}</span>
                  </label>
                );
              })}
              {providers.length === 0 && <div className="empty-state">没有发现 ready 的 provider。</div>}
            </div>
          </section>

          <section className="intro-block">
            <span className="section-kicker">PI ROSTER</span>
            <h1>Disabled Agents</h1>
            <p>勾选的 Agent 无论分配到哪个 Tier 都不会被派发（disabledAgents 优先于 Tier 分配）。pi-roster 不带任何内置人设，这里列出的都是发现到的自定义 Agent。</p>
          </section>
          <section className="legend-card">
            <div className="provider-list">
              {agentNames.map(agent => {
                const isDisabled = tiers.disabledAgents.includes(agent);
                return (
                  <label className={`provider-row${isDisabled ? " is-shadowed" : ""}`} key={agent}>
                    <input
                      type="checkbox"
                      checked={isDisabled}
                      onChange={() => toggleDisabledAgent(agent)}
                    />
                    <span>{agent}</span>
                  </label>
                );
              })}
              {agentNames.length === 0 && <div className="empty-state">没有发现 agent 定义。</div>}
            </div>
          </section>

          <section className="intro-block">
            <span className="section-kicker">PI ROSTER</span>
            <h1>Max Recursion Depth</h1>
            <p>子 Agent 递归派发的深度上限；负数表示不限制。达到上限的子 Agent 不会拿到 dispatch 工具本身，不只是调用被拒绝。</p>
            <input
              type="number"
              className="path-input"
              value={tiers.maxRecursionDepth}
              onChange={event => updateMaxRecursionDepth(event.target.value)}
              aria-label="Max recursion depth"
            />
          </section>

          {snapshot?.warnings.map(warning => <div className="warning-box" key={warning}>{warning}</div>)}
          {invalidModelSpecs.length > 0 && (
            <div className="warning-box">
              {invalidModelSpecs.map(conflict => <div key={conflict}>{conflict}</div>)}
            </div>
          )}
          {shadowConflicts.length > 0 && (
            <div className="warning-box">
              {shadowConflicts.map(conflict => <div key={conflict}>{conflict}</div>)}
            </div>
          )}
        </aside>

        <main className="main-panel">
          <div className="panel-heading">
            <div>
              <span className="section-kicker">MODEL TIERS</span>
              <h2>Pi Roster Tiers</h2>
            </div>
            <div className="panel-summary">
              <span>{tierNames.length} tiers · {agentNames.length} agents</span>
            </div>
          </div>

          <section className="agent-summary-card">
            <div className="catalog-heading"><span>AGENT 实际使用的模型</span><strong>{agentNames.length}</strong></div>
            <p className="provider-help">先看这张表，再往下改 Tier；点一行会跳到对应的 Tier 卡片。没单独分配 Tier 的 Agent 会用标了 default 的那个 Tier。</p>
            <div className="agent-summary-table">
              <div className="agent-summary-row agent-summary-head">
                <span>AGENT</span><span>TIER</span><span>MODEL</span><span>THINKING</span>
              </div>
              {agentModelSummary.map(row => (
                <button
                  type="button"
                  className={`agent-summary-row${row.model ? "" : " is-invalid"}`}
                  key={row.agent}
                  onClick={() => jumpToTier(row.tierName)}
                  disabled={!row.tierName}
                >
                  <span className="agent-summary-name">{row.agent}</span>
                  <span>
                    {row.tierName || "—"}
                    {row.isDefault && row.tierName && <span className="agent-summary-default-badge">default</span>}
                  </span>
                  <span className="agent-summary-model">{row.model || "未配置"}</span>
                  <span>{row.thinking || "—"}</span>
                </button>
              ))}
              {agentModelSummary.length === 0 && <div className="custom-role-empty">没有发现 agent 定义。</div>}
            </div>
          </section>

          <div className="tiers-section embedded">
            <div className="custom-role-heading">
              <div>
                <p>Tier 是一组按优先级排的模型：主模型用不了时（比如 provider 挂了）自动换下一个。下面把 Agent 分配到 Tier 里，就能批量控制一批 Agent 用什么模型。</p>
              </div>
              <button className="button compact" onClick={addTier}>新增 Tier</button>
            </div>
            <div className="tiers-list">
              {tierNames.map(name => {
                const tier = tiers.tiers[name];
                const isDefault = tiers.default === name;
                return (
                  <article
                    className={`tier-card${highlightedTier === name ? " is-highlighted" : ""}`}
                    id={tierCardElementId(name)}
                    key={name}
                  >
                    <div className="tier-card-heading">
                      <input
                        className="tier-name-input"
                        defaultValue={name}
                        key={name}
                        onBlur={event => renameTier(name, event.target.value)}
                        aria-label="Tier 名称"
                      />
                      <div className="tier-actions">
                        <label
                          className="default-tier-toggle"
                          title="没被分配到任何 Tier 的 Agent，会用这个 Tier 的模型（写入 pi-roster.default）"
                        >
                          <input type="checkbox" checked={isDefault} onChange={() => setDefaultTier(name)} />
                          <span>default</span>
                        </label>
                        <button
                          className={`button quiet compact${pendingDeleteTier === name ? " danger" : ""}`}
                          onClick={() => removeTier(name)}
                        >
                          {pendingDeleteTier === name ? "确认移除" : "移除"}
                        </button>
                      </div>
                    </div>


                    <div className="tier-models">
                      <div className="tier-model-row">
                        <span className="fallback-order is-primary">主模型</span>
                        <div className="tier-model-picker">
                          <PiModelSelect
                            value={tier.model}
                            models={models}
                            shadowedProviders={shadowedProviders}
                            onChange={value => updateTierModel(name, value)}
                          />
                        </div>
                        <div className="tier-model-controls">
                          <PiThinkingSelect
                            value={tier.model}
                            models={models}
                            onChange={value => updateTierModel(name, value)}
                          />
                        </div>
                      </div>
                      {tier.fallbackModels.map((selector, index) => (
                        <div className="tier-model-row" key={index}>
                          <span className="fallback-order">Fallback {index + 1}</span>
                          <div className="tier-model-picker">
                            <PiModelSelect
                              value={selector}
                              models={models}
                              shadowedProviders={shadowedProviders}
                              onChange={value => updateFallback(name, index, value)}
                            />
                          </div>
                          <div className="tier-model-controls">
                            <PiThinkingSelect
                              value={selector}
                              models={models}
                              onChange={value => updateFallback(name, index, value)}
                            />
                            <button className="icon-button danger" onClick={() => removeFallback(name, index)} aria-label="Remove fallback">×</button>
                          </div>
                        </div>
                      ))}
                      <button className="button quiet compact" onClick={() => addFallback(name)}>添加 fallback</button>
                    </div>

                    <div className="tier-roles">
                      <span className="tier-roles-label">哪些 AGENT 用这个 TIER（点击切换）</span>
                      <div className="tier-role-chips">
                        {agentNames.map(agent => {
                          const inThisTier = (tiers.assignments[name] ?? []).includes(agent);
                          const ownerName = roleOwner.get(agent);
                          const ownerTier = ownerName && ownerName !== name ? ownerName : undefined;
                          return (
                            <button
                              type="button"
                              key={agent}
                              className={`tier-role-chip${inThisTier ? " is-active" : ""}`}
                              onClick={() => toggleAgent(name, agent)}
                              title={ownerTier ? `当前属于 ${ownerTier}` : undefined}
                            >
                              {agent}{ownerTier && <span className="tier-role-chip-owner"> · {ownerTier}</span>}
                            </button>
                          );
                        })}
                        {agentNames.length === 0 && <span className="custom-role-empty">没有发现 agent 定义。</span>}
                      </div>
                    </div>
                  </article>
                );
              })}
              {tierNames.length === 0 && (
                <div className="custom-role-empty">没有配置 Tier；新增一个来管理 pi-roster 的模型分配。</div>
              )}
            </div>
          </div>

          {error && snapshot && <div className="routing-errors">{error}</div>}
        </main>
      </div>
      {customProviderEditor && (
        <CustomProviderModal
          editor={customProviderEditor}
          existingProviderIds={customProviders.map(provider => provider.id)}
          apiOptions={PI_CUSTOM_PROVIDER_API_OPTIONS}
          productName="Pi"
          destinationPath="~/.pi/agent/models.json"
          requireApiKey={false}
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
    </div>
  );
}
