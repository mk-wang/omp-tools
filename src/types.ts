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
  modelSource: string;
  agentFiles: AgentDefinitionFile[];
  routingSettings: RoutingSettings;
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
  shadowedProviders: string[];
  routingSettings: RoutingSettings;
  ompPath?: string | null;
};
