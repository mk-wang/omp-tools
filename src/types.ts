export type ModelCost = {
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
};

export type ModelOption = {
  provider: string;
  id: string;
  selector: string;
  name: string;
  reasoning: boolean;
  thinking: string[];
  input: string[];
  contextWindow: number | null;
  maxTokens: number | null;
  cost: ModelCost | null;
  /** Catalog kind from `omp models --kind all`; "" when that pass is unavailable. */
  kind: string;
};

export type CustomProviderModel = {
  id: string;
  name: string;
  api?: string;
  thinking?: string[];
  defaultThinkingLevel?: string;
};

export type CustomProviderSaveModel = {
  id: string;
  api: string;
  thinking?: string[];
  defaultThinkingLevel?: string | null;
};

export type CustomProvider = {
  id: string;
  baseUrl: string;
  /** UI default Type; persisted models carry the authoritative API values. */
  api: string;
  apiKeyConfigured: boolean;
  authNone: boolean;
  models: CustomProviderModel[];
};

export type CustomProviderSave = {
  id: string;
  baseUrl: string;
  /** Default Type retained by the editor and applied to models without an override. */
  api: string;
  apiKey: string | null;
  models: CustomProviderSaveModel[];
};

export type CustomProviderSaveRequest = {
  customProviders: CustomProviderSave[];
  removedCustomProviders: string[];
  target?: "omp" | "pi";
};

export type CustomProviderDiscoveryRequest = {
  providerId: string;
  baseUrl: string;
  api: string;
  apiKey: string | null;
  target?: "omp" | "pi";
};


export type FallbackChain = {
  target: string;
  selectors: string[];
};

export type RoutingSettings = {
  defaultThinkingLevel: string | null;
  autoThinkingMaxEffort: string | null;
  advisorEnabled: boolean | null;
  advisorSyncBacklog: string | null;
  advisorImmuneTurns: number | null;
  prewalkEnabled: boolean | null;
  retryModelFallback: boolean | null;
  retryFallbackChains: FallbackChain[];
  retryFallbackRevertPolicy: string | null;
};

export type AgentDefinitionFile = {
  fileName: string;
  path: string;
  name: string;
  description: string;
  model: string;
  content: string;
};

export type ConfigSnapshot = {
  configPath: string;
  configExists: boolean;
  modelRoles: Record<string, string>;
  agentModelOverrides: Record<string, string>;
  models: ModelOption[];
  /** Candidates for the non-chat kind roles, from `omp models --kind all`. */
  kindModels: ModelOption[];
  modelSource: string;
  agentFiles: AgentDefinitionFile[];
  routingSettings: RoutingSettings;
  customProviders: CustomProvider[];
  warnings: string[];
};

export type AgentFileSave = {
  fileName: string;
  agentMarkdown: string;
};

export type SaveRequest = {
  modelRoles: Record<string, string>;
  removedModelRoles: string[];
  agentModelOverrides: Record<string, string>;
  agentFiles: AgentFileSave[];
  routingSettings: RoutingSettings;
  ompPath: string | null;
};

export type PiModelOption = {
  provider: string;
  id: string;
  selector: string;
  contextWindow: number | null;
  maxTokens: number | null;
  reasoning: boolean;
  images: boolean;
  thinking?: string[];
  defaultThinkingLevel?: string;
};

/** Studio's Tier editor shape; each model spec owns its thinking suffix. */
export type PiTier = {
  /** Complete provider/model-id:thinking spec for the primary attempt. */
  model: string;
  /** Complete provider/model-id:thinking specs tried after the primary. */
  fallbackModels: string[];
};


export type PiTiersFile = {
  /** Maps to pi-roster.defaultModelProfile on disk. */
  default: string;
  /** Maps to pi-roster.modelProfiles on disk. */
  tiers: Record<string, PiTier>;
  assignments: Record<string, string[]>;
  /** Agent names pi-roster must never dispatch to, regardless of tier assignment. */
  disabledAgents: string[];
  /** Negative means unlimited, matching pi-roster's own semantics. */
  maxRecursionDepth: number;
};

export type PiSnapshot = {
  /** ~/.pi/agent/settings.json — pi-roster's "pi-roster" block lives inside it, no separate tiers file. */
  settingsPath: string;
  rosterConfigExists: boolean;
  tiers: PiTiersFile;
  models: PiModelOption[];
  customProviders: CustomProvider[];
  agentNames: string[];
  shadowedProviders: string[];
  warnings: string[];
};
