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

export type JsonPrimitive = string | number | boolean | null;

export type JsonValue = JsonPrimitive | JsonValue[] | { [key: string]: JsonValue };

export type JsonObject = { [key: string]: JsonValue };

export type CustomProviderModel = {
  id: string;
  name: string;
  api?: string;
  thinking?: string[];
  defaultThinkingLevel?: string;
  /** Canonical OMP model capabilities; excludes identity, routing and credential settings. */
  configuration?: JsonObject;
};

export type CustomProviderSaveModel = {
  id: string;
  api: string;
  thinking?: string[];
  defaultThinkingLevel?: string | null;
  configuration?: JsonObject;
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

export type CustomProviderStore = {
  customProviders: CustomProvider[];
  version: string;
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
  expectedVersion: string;
  customProviders: CustomProviderSave[];
  removedCustomProviders: string[];
};

export type CustomProviderDiscoveryRequest = {
  providerId: string;
  baseUrl: string;
  api: string;
  apiKey: string | null;
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
  customProvidersVersion: string;
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
