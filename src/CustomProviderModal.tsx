import type { CustomProviderModel } from "./types";

export type CustomProviderEditor = {
  mode: "create" | "edit";
  originalId: string;
  id: string;
  baseUrl: string;
  api: string;
  apiKey: string;
  apiKeyConfigured: boolean;
  authNone: boolean;
  models: string[];
  modelApiOverrides: Record<string, string>;
  availableModels: CustomProviderModel[];
  discoveredModelIds: string[] | null;
};


export type CustomProviderApiOption = {
  value: string;
  label: string;
};

export const CUSTOM_PROVIDER_PATTERN = /^[a-zA-Z0-9][a-zA-Z0-9._-]*$/;

export const CUSTOM_PROVIDER_API_OPTIONS: readonly CustomProviderApiOption[] = [
  { value: "openai-completions", label: "OpenAI Completions" },
  { value: "openai-responses", label: "OpenAI Responses" },
  { value: "openai-codex-responses", label: "OpenAI Codex Responses" },
  { value: "azure-openai-responses", label: "Azure OpenAI Responses" },
  { value: "anthropic-messages", label: "Anthropic Messages" },
  { value: "bedrock-converse-stream", label: "Bedrock Converse" },
  { value: "google-generative-ai", label: "Google Generative AI" },
  { value: "google-gemini-cli", label: "Google Gemini CLI" },
  { value: "google-vertex", label: "Google Vertex" }
];

export const PI_CUSTOM_PROVIDER_API_OPTIONS: readonly CustomProviderApiOption[] = [
  { value: "openai-completions", label: "OpenAI Completions" },
  { value: "openai-responses", label: "OpenAI Responses" },
  { value: "anthropic-messages", label: "Anthropic Messages" },
  { value: "google-generative-ai", label: "Google Generative AI" }
];

export function sortedCustomProviderModels(models: CustomProviderModel[]): CustomProviderModel[] {
  return [...models].sort((left, right) => left.id.localeCompare(right.id));
}

export function inferredCustomProviderApi(
  providerApi: string,
  models: CustomProviderModel[],
  fallbackApi: string
): string {
  const configured = providerApi.trim();
  if (configured) return configured;

  const counts = new Map<string, number>();
  models.forEach(model => {
    const api = model.api?.trim();
    if (api) counts.set(api, (counts.get(api) ?? 0) + 1);
  });
  return [...counts.entries()]
    .sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]))[0]?.[0]
    ?? fallbackApi;
}

export function customProviderApiOverrides(
  models: CustomProviderModel[],
  defaultApi: string
): Record<string, string> {
  return Object.fromEntries(
    models.flatMap(model => {
      const api = model.api?.trim();
      return api && api !== defaultApi ? [[model.id, api] as const] : [];
    })
  );
}


export function CustomProviderModal({
  editor,
  existingProviderIds,
  apiOptions,
  productName,
  destinationPath,
  requireApiKey,
  discovering,
  discoveryError,
  saving,
  saveError,
  onChange,
  onDiscover,
  onCancel,
  onSave
}: {
  editor: CustomProviderEditor;
  existingProviderIds: string[];
  apiOptions: readonly CustomProviderApiOption[];
  productName: string;
  destinationPath: string;
  requireApiKey: boolean;
  discovering: boolean;
  discoveryError: string;
  saving: boolean;
  saveError: string;
  onChange: (editor: CustomProviderEditor) => void;
  onDiscover: () => void;
  onCancel: () => void;
  onSave: () => void;
}) {
  const normalizedId = editor.id.trim();
  const duplicateId = editor.mode === "create" && existingProviderIds.includes(normalizedId);
  const idError = !normalizedId
    ? "请输入 provider ID。"
    : !CUSTOM_PROVIDER_PATTERN.test(normalizedId)
      ? "Provider ID 只能包含字母、数字、-、_、.，且必须以字母或数字开头。"
      : duplicateId
        ? "该 provider 已存在。"
        : "";
  const baseUrlError = !editor.baseUrl.trim()
    ? "请输入 Base URL。"
    : !/^https?:\/\/\S+$/i.test(editor.baseUrl.trim())
      ? "Base URL 必须以 http:// 或 https:// 开头。"
      : "";
  const apiError = apiOptions.some(option => option.value === editor.api)
    ? ""
    : `请选择 ${productName} 支持的 Type。`;
  const modelApiError = editor.models.some(modelId => {
    const api = editor.modelApiOverrides[modelId]?.trim() || editor.api;
    return !apiOptions.some(option => option.value === api);
  })
    ? "请选择每个 model 支持的 Type。"
    : "";
  const keyError = requireApiKey && !editor.apiKey.trim() && !editor.apiKeyConfigured && !editor.authNone
    ? "请输入 API key。"
    : "";
  const modelError = editor.availableModels.length === 0
    ? "请先获取 provider 返回的 model 列表。"
    : editor.models.length === 0
      ? "至少勾选一个 model。"
      : "";
  const error = idError || baseUrlError || apiError || modelApiError || keyError || modelError;
  const selectedModels = new Set(editor.models);
  const discoveredModelIds = editor.discoveredModelIds;
  const unavailableModelIds = discoveredModelIds === null
    ? []
    : editor.models.filter(modelId => !discoveredModelIds.includes(modelId));
  const unavailableModelSet = new Set(unavailableModelIds);
  const availableModelIds = new Set(editor.availableModels.map(model => model.id));
  const staleSelectedModels = discoveredModelIds === null
    ? []
    : editor.models
      .filter(modelId => !availableModelIds.has(modelId))
      .map(id => ({ id, name: id }));
  const modelOptions = sortedCustomProviderModels([
    ...editor.availableModels,
    ...staleSelectedModels
  ]);

  const updateModelApi = (modelId: string, api: string) => {
    const modelApiOverrides = { ...editor.modelApiOverrides };
    if (api) modelApiOverrides[modelId] = api;
    else delete modelApiOverrides[modelId];
    onChange({ ...editor, modelApiOverrides });
  };

  return (
    <div
      className="modal-backdrop"
      role="presentation"
      onMouseDown={event => {
        if (event.target === event.currentTarget) onCancel();
      }}
    >
      <section className="role-modal custom-provider-modal" role="dialog" aria-modal="true" aria-labelledby="custom-provider-title">
        <div className="role-modal-header">
          <div>
            <span className="section-kicker">CUSTOM PROVIDER</span>
            <h2 id="custom-provider-title">{editor.mode === "create" ? `新增 ${productName} Custom Provider` : `编辑 ${editor.id}`}</h2>
          </div>
          <button className="modal-close" onClick={onCancel} aria-label="关闭">×</button>
        </div>
        <div className="role-modal-body">
          <div className="custom-provider-form-grid">
            <label className="modal-field">
              <span>Provider ID</span>
              {editor.mode === "create" ? (
                <input
                  value={editor.id}
                  onChange={event => onChange({ ...editor, id: event.target.value, availableModels: [], discoveredModelIds: null })}
                  placeholder="例如 team-gateway"
                  autoFocus
                />
              ) : (
                <code className="locked-role">{editor.id}</code>
              )}
            </label>
            <label className="modal-field">
              <span>Type</span>
              <select
                value={editor.api}
                onChange={event => onChange({ ...editor, api: event.target.value, availableModels: [], discoveredModelIds: null })}
              >
                <option value="">选择 API type…</option>
                {apiOptions.map(option => (
                  <option value={option.value} key={option.value}>{option.label}</option>
                ))}
              </select>
            </label>
          </div>
          <label className="modal-field">
            <span>Base URL</span>
            <input
              value={editor.baseUrl}
              onChange={event => onChange({ ...editor, baseUrl: event.target.value, availableModels: [], models: [], modelApiOverrides: {}, discoveredModelIds: null })}
              placeholder="https://gateway.example.com/v1"
              spellCheck={false}
            />
          </label>
          <label className="modal-field">
            <span>API key</span>
            <input
              type="password"
              value={editor.apiKey}
              onChange={event => onChange({ ...editor, apiKey: event.target.value, availableModels: [], discoveredModelIds: null })}
              placeholder={editor.apiKeyConfigured ? "已配置；留空保持不变" : requireApiKey ? "输入 API key" : "可留空，使用 Pi auth.json 或环境变量"}
              autoComplete="new-password"
              spellCheck={false}
            />
          </label>
          <div className="modal-field">
            <div className="custom-provider-model-heading">
              <span>Models from provider</span>
              <strong>{discoveredModelIds === null ? `${editor.models.length} 已配置 · 待获取上游列表` : `${editor.models.length} / ${modelOptions.length} selected`}</strong>
            </div>
            <div className="custom-provider-model-toolbar">
              <button
                className="button compact"
                onClick={onDiscover}
                disabled={discovering || !editor.baseUrl.trim()}
              >
                {discovering ? "获取中…" : "获取 Model 列表"}
              </button>
              <span>GET {editor.baseUrl.trim() || "<baseUrl>"}/models · 可选 rich catalog · Authorization: Bearer</span>
            </div>
            {unavailableModelIds.length > 0 && (
              <div className="custom-provider-model-warning" role="status">
                上游当前未提供：{unavailableModelIds.join("、")}。保留配置不会删除它，但仍会标记为不可用；取消勾选后保存可移除。
              </div>
            )}
            <div className="custom-provider-model-list">
              {modelOptions.map(model => {
                const selected = selectedModels.has(model.id);
                const isUnavailable = unavailableModelSet.has(model.id);
                const override = editor.modelApiOverrides[model.id] ?? "";
                const metadata = [
                  model.name !== model.id ? model.name : "",
                  model.thinking?.length ? `thinking: ${model.thinking.join(", ")}` : ""
                ].filter(Boolean).join(" · ");
                return (
                  <div className={`custom-provider-model-option${selected ? " is-selected" : ""}${isUnavailable ? " is-unavailable" : ""}`} key={model.id}>
                    <label className="custom-provider-model-selection">
                      <input
                        type="checkbox"
                        checked={selected}
                        onChange={event => onChange({
                          ...editor,
                          models: event.currentTarget.checked
                            ? [...editor.models, model.id]
                            : editor.models.filter(id => id !== model.id)
                        })}
                      />
                      <span className="custom-provider-model-id">
                        {model.id}
                        {isUnavailable && <em className="custom-provider-model-unavailable">上游不可用</em>}
                      </span>
                      {metadata && <small>{metadata}</small>}
                    </label>
                    <select
                      className="custom-provider-model-api"
                      value={override}
                      onChange={event => updateModelApi(model.id, event.target.value)}
                      aria-label={`${model.id} API type`}
                    >
                      <option value="">跟随总 Type（{editor.api || "未设置"}）</option>
                      {apiOptions.map(option => (
                        <option value={option.value} key={option.value}>{option.label}</option>
                      ))}
                    </select>
                  </div>
                );
              })}
              {modelOptions.length === 0 && (
                <div className="empty-state">点击“获取 Model 列表”，读取 provider 的 /models。</div>
              )}
            </div>
          </div>
          {discoveryError && <div className="modal-validation is-error">{discoveryError}</div>}
          {saveError && <div className="modal-validation is-error">{saveError}</div>}
          <div className={`modal-validation${error ? " is-error" : ""}`}>
            {error || `将立即写入 ${destinationPath}；不影响窗口右上角的 Apply。`}
          </div>
        </div>
        <div className="role-modal-footer">
          <span>总 Type 作为默认；每个 model 可单独覆盖，保存时 API type 写入各自的 model。</span>
          <div>
            <button className="button quiet" onClick={onCancel} disabled={saving}>取消</button>
            <button className="button" onClick={onSave} disabled={Boolean(error) || saving}>
              {saving ? "保存中…" : editor.mode === "create" ? "保存 Provider" : "更新 Provider"}
            </button>
          </div>
        </div>
      </section>
    </div>
  );
}
