export type ModelOption = {
  provider: string;
  id: string;
  selector: string;
  name: string;
  reasoning: boolean;
  thinking: string[];
  input: string[];
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
  disabledProviders: string[];
  models: ModelOption[];
  modelSource: string;
  agentFiles: AgentDefinitionFile[];
  warnings: string[];
};

export type CustomRoleSave = {
  role: string;
  selector: string;
};

export type AgentFileSave = {
  fileName: string;
  agentMarkdown: string;
};

export type SaveRequest = {
  smol: string;
  default: string;
  slow: string;
  vision: string;
  advisor: string;
  customRoles: CustomRoleSave[];
  agentFiles: AgentFileSave[];
  reviewer: string;
  securityReviewer: string;
  disabledProviders: string[];
};
