mod model_metadata;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use serde_yaml::{Mapping, Value as YamlValue};
use std::{
    collections::{hash_map::RandomState, BTreeMap, HashMap, HashSet},
    env, fs,
    hash::BuildHasher,
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
    time::Duration,
};

const THINKING_LEVELS: [&str; 7] = ["minimal", "low", "medium", "high", "xhigh", "max", "off"];
const RICH_MODEL_CATALOG_CLIENT_VERSION: &str = "0.144.1";
const CONFIGURED_THINKING_LEVELS: [&str; 8] = [
    "auto", "minimal", "low", "medium", "high", "xhigh", "max", "off",
];
const ADVISOR_SYNC_BACKLOGS: [&str; 4] = ["off", "1", "3", "5"];
const FALLBACK_REVERT_POLICIES: [&str; 2] = ["cooldown-expiry", "never"];
const AUTO_THINKING_MAX_EFFORTS: [&str; 2] = ["xhigh", "max"];

const SYSTEM_MODEL_ROLES: [&str; 10] = [
    "default", "smol", "slow", "vision", "plan", "designer", "commit", "tiny", "task", "advisor",
];
/// Non-chat roles OMP resolves against the catalog `kind` (and, for `web`, a
/// `webSearch` grounding) rather than the chat pool. Mirrors
/// `KIND_ROLE_IDS` in oh-my-pi's model-roles.ts.
const KIND_MODEL_ROLES: [&str; 5] = ["image", "web", "speech", "dictation", "judge"];
const CUSTOM_PROVIDER_APIS: [&str; 21] = [
    "openai-completions",
    "openai-responses",
    "openai-codex-responses",
    "azure-openai-responses",
    "anthropic-messages",
    "bedrock-converse-stream",
    "google-generative-ai",
    "google-gemini-cli",
    "google-vertex",
    "openrouter-decisions",
    "openai-decisions",
    "workers-ai-systemone",
    "typesafe",
    "openai-images",
    "openrouter-images",
    "xai-tts",
    "openai-speech",
    "openai-embeddings",
    "openrouter-rerank",
    "openrouter-video",
    "openai-transcriptions",
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelCost {
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelOption {
    provider: String,
    id: String,
    selector: String,
    name: String,
    reasoning: bool,
    thinking: Vec<String>,
    input: Vec<String>,
    context_window: Option<u64>,
    max_tokens: Option<u64>,
    cost: Option<ModelCost>,
    /// Catalog kind from `omp models --kind all`. Empty when that metadata pass
    /// is unavailable; role validation is then skipped rather than guessed.
    /// Note: `omp models --json` emits no `webSearch` field, so a chat model's
    /// grounding is not observable here — see `accepts_kind_role`.
    kind: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CustomProviderModel {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    api: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    thinking: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_thinking_level: Option<String>,
    #[serde(skip_serializing_if = "serde_json::Map::is_empty")]
    configuration: model_metadata::Configuration,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CustomProviderSaveModel {
    id: String,
    api: String,
    #[serde(default)]
    thinking: Vec<String>,
    #[serde(default)]
    default_thinking_level: Option<String>,
    #[serde(default)]
    configuration: model_metadata::Configuration,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CustomProvider {
    id: String,
    base_url: String,
    api: String,
    api_key_configured: bool,
    auth_none: bool,
    models: Vec<CustomProviderModel>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CustomProviderSave {
    id: String,
    base_url: String,
    api: String,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    models: Vec<CustomProviderSaveModel>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CustomProviderSaveRequest {
    expected_version: String,
    #[serde(default)]
    custom_providers: Vec<CustomProviderSave>,
    #[serde(default)]
    removed_custom_providers: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CustomProviderStore {
    custom_providers: Vec<CustomProvider>,
    version: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CustomProviderDiscoveryRequest {
    provider_id: String,
    base_url: String,
    api: String,
    #[serde(default)]
    api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentDefinitionFile {
    file_name: String,
    path: String,
    name: String,
    description: String,
    model: String,
    content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FallbackChain {
    target: String,
    selectors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoutingSettings {
    default_thinking_level: Option<String>,
    auto_thinking_max_effort: Option<String>,
    advisor_enabled: Option<bool>,
    advisor_sync_backlog: Option<String>,
    advisor_immune_turns: Option<u64>,
    prewalk_enabled: Option<bool>,
    retry_model_fallback: Option<bool>,
    retry_fallback_chains: Vec<FallbackChain>,
    retry_fallback_revert_policy: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigSnapshot {
    config_path: String,
    config_exists: bool,
    model_roles: BTreeMap<String, String>,
    agent_model_overrides: BTreeMap<String, String>,
    models: Vec<ModelOption>,
    /// Candidates for the non-chat kind roles, drawn from `omp models --kind all`.
    kind_models: Vec<ModelOption>,
    model_source: String,
    agent_files: Vec<AgentDefinitionFile>,
    routing_settings: RoutingSettings,
    custom_providers: Vec<CustomProvider>,
    custom_providers_version: String,
    warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentFileSave {
    file_name: String,
    agent_markdown: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveRequest {
    model_roles: BTreeMap<String, String>,
    removed_model_roles: Vec<String>,
    agent_model_overrides: BTreeMap<String, String>,
    agent_files: Vec<AgentFileSave>,
    routing_settings: RoutingSettings,
    omp_path: Option<String>,
}
fn home_dir() -> Result<PathBuf, String> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| "无法定位用户主目录".to_string())
}

fn agent_dir() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".omp").join("agent"))
}

fn config_file(agent: &Path) -> PathBuf {
    let yaml = agent.join("config.yml");
    if yaml.exists() {
        return yaml;
    }
    let alternate = agent.join("config.yaml");
    if alternate.exists() {
        return alternate;
    }
    yaml
}

fn models_file(agent: &Path) -> PathBuf {
    let yaml = agent.join("models.yml");
    if yaml.exists() {
        return yaml;
    }
    let alternate = agent.join("models.yaml");
    if alternate.exists() {
        return alternate;
    }
    yaml
}

fn editable_file(agent: &Path, kind: &str) -> Result<PathBuf, String> {
    match kind {
        "config" => Ok(config_file(agent)),
        "models" => Ok(models_file(agent)),
        _ => Err(format!("不支持直接编辑的文件类型 “{kind}”")),
    }
}

fn reveal_in_file_manager(path: &Path) -> Result<(), String> {
    let path = path.to_string_lossy().into_owned();
    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg("-R").arg(&path).spawn();
    #[cfg(target_os = "windows")]
    let result = Command::new("explorer")
        .arg(format!("/select,{path}"))
        .spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = Command::new("xdg-open")
        .arg(Path::new(&path).parent().unwrap_or_else(|| Path::new(".")))
        .spawn();

    result
        .map(|_| ())
        .map_err(|error| format!("无法在文件管理器中显示文件: {error}"))
}

fn display_path(path: &Path) -> String {
    if let Ok(home) = home_dir() {
        if let Ok(relative) = path.strip_prefix(home) {
            return format!("~/{}", relative.display());
        }
    }
    path.display().to_string()
}

/// A role name OMP itself owns. Both the chat roles and the non-chat kind
/// roles are built-in, so they must never be treated as user-invented names:
/// that mislabels them in the UI and lets them be "removed".
/// Whether `modelRoles.<role>` already holds a non-empty selector.
fn model_role_is_set(root: &YamlValue, role: &str) -> bool {
    string_map(match root {
        YamlValue::Mapping(map) => map.get(YamlValue::String("modelRoles".to_string())),
        _ => None,
    })
    .get(role)
    .is_some_and(|value| !value.trim().is_empty())
}

fn is_system_role(role: &str) -> bool {
    SYSTEM_MODEL_ROLES.contains(&role) || KIND_MODEL_ROLES.contains(&role)
}

fn valid_custom_role(role: &str) -> bool {
    let mut characters = role.chars();
    matches!(characters.next(), Some(first) if first.is_ascii_lowercase())
        && characters.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '-' | '_')
        })
        && !is_system_role(role)
}

fn first_model_selector(value: &YamlValue) -> Option<String> {
    match value {
        YamlValue::String(selector) => selector
            .split(',')
            .map(str::trim)
            .find(|selector| !selector.is_empty())
            .map(str::to_string),
        YamlValue::Sequence(selectors) => selectors
            .iter()
            .filter_map(YamlValue::as_str)
            .map(str::trim)
            .find(|selector| !selector.is_empty())
            .map(str::to_string),
        _ => None,
    }
}

fn parse_agent_markdown(content: &str) -> Result<(String, String, String), String> {
    let normalized = content.replace("\r\n", "\n");
    let rest = normalized
        .strip_prefix("---\n")
        .ok_or_else(|| "缺少开头的 YAML frontmatter".to_string())?;
    let (frontmatter, body) = rest
        .split_once("\n---\n")
        .ok_or_else(|| "缺少结束的 YAML frontmatter 分隔符".to_string())?;
    if body.trim().is_empty() {
        return Err("Agent Markdown 必须包含非空说明正文".to_string());
    }

    let parsed: YamlValue = serde_yaml::from_str(frontmatter)
        .map_err(|error| format!("YAML frontmatter 无法解析: {error}"))?;
    let mapping = parsed
        .as_mapping()
        .ok_or_else(|| "YAML frontmatter 必须是对象".to_string())?;
    let field = |name: &str| {
        mapping
            .get(YamlValue::String(name.to_string()))
            .and_then(YamlValue::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let name = field("name").ok_or_else(|| "Agent Markdown 缺少 name".to_string())?;
    let description =
        field("description").ok_or_else(|| "Agent Markdown 缺少 description".to_string())?;
    let model = mapping
        .get(YamlValue::String("model".to_string()))
        .and_then(first_model_selector)
        .ok_or_else(|| "Agent Markdown 缺少有效 model".to_string())?;

    Ok((name, description, model))
}

fn load_agent_files(agent: &Path, warnings: &mut Vec<String>) -> Vec<AgentDefinitionFile> {
    let directory = agent.join("agents");
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            warnings.push(format!("无法读取 {}: {error}", display_path(&directory)));
            return Vec::new();
        }
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("md"))
        .collect::<Vec<_>>();
    paths.sort();

    paths
        .into_iter()
        .filter_map(|path| {
            let content = match fs::read_to_string(&path) {
                Ok(content) => content,
                Err(error) => {
                    warnings.push(format!("无法读取 {}: {error}", display_path(&path)));
                    return None;
                }
            };
            let (name, description, model) = match parse_agent_markdown(&content) {
                Ok(parsed) => parsed,
                Err(error) => {
                    warnings.push(format!("{} 无效: {error}", display_path(&path)));
                    return None;
                }
            };
            Some(AgentDefinitionFile {
                file_name: path.file_name()?.to_str()?.to_string(),
                path: display_path(&path),
                name,
                description,
                model,
                content,
            })
        })
        .collect()
}

fn load_yaml(path: &Path) -> Result<(YamlValue, bool), String> {
    if !path.exists() {
        return Ok((YamlValue::Mapping(Mapping::new()), false));
    }

    let content = fs::read_to_string(path).map_err(|error| format!("读取配置失败: {error}"))?;
    let parsed: YamlValue = serde_yaml::from_str(&content)
        .map_err(|error| format!("config.yml YAML 无法解析: {error}"))?;
    match parsed {
        YamlValue::Null => Ok((YamlValue::Mapping(Mapping::new()), true)),
        YamlValue::Mapping(_) => Ok((parsed, true)),
        _ => Err("config.yml 根节点必须是 YAML 对象".to_string()),
    }
}

fn load_models_yaml(path: &Path) -> Result<(YamlValue, bool), String> {
    if !path.exists() {
        return Ok((YamlValue::Mapping(Mapping::new()), false));
    }

    let content = fs::read_to_string(path)
        .map_err(|error| format!("读取 {} 失败: {error}", display_path(path)))?;
    let parsed: YamlValue = serde_yaml::from_str(&content)
        .map_err(|error| format!("{} YAML 无法解析: {error}", display_path(path)))?;
    match parsed {
        YamlValue::Null => Ok((YamlValue::Mapping(Mapping::new()), true)),
        YamlValue::Mapping(_) => Ok((parsed, true)),
        _ => Err(format!("{} 根节点必须是 YAML 对象", display_path(path))),
    }
}

fn string_map(value: Option<&YamlValue>) -> BTreeMap<String, String> {
    let Some(YamlValue::Mapping(map)) = value else {
        return BTreeMap::new();
    };

    map.iter()
        .filter_map(|(key, value)| {
            let (YamlValue::String(key), YamlValue::String(value)) = (key, value) else {
                return None;
            };
            Some((key.clone(), value.clone()))
        })
        .collect()
}
fn string_list(value: Option<&YamlValue>) -> Vec<String> {
    let Some(YamlValue::Sequence(values)) = value else {
        return Vec::new();
    };

    values
        .iter()
        .filter_map(YamlValue::as_str)
        .map(str::to_string)
        .collect()
}
fn nested_value<'a>(root: &'a YamlValue, path: &[&str]) -> Option<&'a YamlValue> {
    let mut value = root;
    for segment in path {
        let map = value.as_mapping()?;
        value = map.get(YamlValue::String((*segment).to_string()))?;
    }
    Some(value)
}

fn optional_string(root: &YamlValue, path: &[&str]) -> Option<String> {
    nested_value(root, path)
        .and_then(YamlValue::as_str)
        .map(str::to_string)
}

fn optional_bool(root: &YamlValue, path: &[&str]) -> Option<bool> {
    nested_value(root, path).and_then(YamlValue::as_bool)
}

fn optional_u64(root: &YamlValue, path: &[&str]) -> Option<u64> {
    nested_value(root, path).and_then(YamlValue::as_u64)
}

fn fallback_chains(value: Option<&YamlValue>) -> Vec<FallbackChain> {
    let Some(YamlValue::Mapping(map)) = value else {
        return Vec::new();
    };
    map.iter()
        .filter_map(|(target, selectors)| {
            let target = target.as_str()?.to_string();
            Some(FallbackChain {
                target,
                selectors: string_list(Some(selectors)),
            })
        })
        .collect()
}

fn nested_string_map(root: &YamlValue, path: &[&str]) -> BTreeMap<String, String> {
    let mut value = root;
    for segment in path {
        let YamlValue::Mapping(map) = value else {
            return BTreeMap::new();
        };
        let key = YamlValue::String((*segment).to_string());
        let Some(next) = map.get(&key) else {
            return BTreeMap::new();
        };
        value = next;
    }
    string_map(Some(value))
}

fn mapping_string(map: &Mapping, key: &str) -> Option<String> {
    map.get(YamlValue::String(key.to_string()))
        .and_then(YamlValue::as_str)
        .map(str::to_string)
}
fn yaml_model_api(value: &YamlValue) -> Option<String> {
    value
        .as_mapping()
        .and_then(|map| mapping_string(map, "api"))
        .and_then(|api| (!api.trim().is_empty()).then_some(api.trim().to_string()))
}

fn inferred_custom_provider_api(
    provider_api: Option<String>,
    models: &[CustomProviderModel],
) -> String {
    if let Some(api) = provider_api.filter(|api| !api.trim().is_empty()) {
        return api;
    }

    let mut counts = BTreeMap::<String, usize>::new();
    for model in models {
        if let Some(api) = model.api.as_ref().filter(|api| !api.trim().is_empty()) {
            *counts.entry(api.clone()).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .max_by(|(left_api, left_count), (right_api, right_count)| {
            left_count
                .cmp(right_count)
                .then_with(|| right_api.cmp(left_api))
        })
        .map(|(api, _)| api)
        .unwrap_or_default()
}

fn yaml_model_id(value: &YamlValue) -> Option<String> {
    let id = match value {
        YamlValue::String(id) => id.trim().to_string(),
        YamlValue::Mapping(map) => mapping_string(map, "id")?.trim().to_string(),
        _ => return None,
    };
    (!id.is_empty()).then_some(id)
}

fn normalize_custom_provider_thinking_level(raw: &str) -> Option<String> {
    let normalized = raw.trim().to_ascii_lowercase();
    let normalized = if normalized == "none" {
        "off"
    } else {
        normalized.as_str()
    };
    THINKING_LEVELS
        .contains(&normalized)
        .then(|| normalized.to_string())
}

fn normalize_custom_provider_thinking_levels(values: &[String]) -> Vec<String> {
    let found = values
        .iter()
        .filter_map(|value| normalize_custom_provider_thinking_level(value))
        .collect::<HashSet<_>>();
    THINKING_LEVELS
        .iter()
        .filter(|level| found.contains(**level))
        .map(|level| (*level).to_string())
        .collect()
}

fn yaml_model_thinking_levels(value: &YamlValue) -> Vec<String> {
    let Some(map) = value.as_mapping() else {
        return Vec::new();
    };
    if let Some(levels) = map
        .get(YamlValue::String("thinking".to_string()))
        .and_then(YamlValue::as_mapping)
        .and_then(|thinking| thinking.get(YamlValue::String("efforts".to_string())))
        .and_then(YamlValue::as_sequence)
    {
        return normalize_custom_provider_thinking_levels(
            &levels
                .iter()
                .filter_map(YamlValue::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>(),
        );
    }
    map.get(YamlValue::String("thinkingLevelMap".to_string()))
        .and_then(YamlValue::as_mapping)
        .map(|level_map| {
            normalize_custom_provider_thinking_levels(
                &level_map
                    .iter()
                    .filter(|(_, value)| !matches!(value, YamlValue::Null))
                    .filter_map(|(level, _)| level.as_str().map(str::to_string))
                    .collect::<Vec<_>>(),
            )
        })
        .unwrap_or_default()
}

fn yaml_model_default_thinking_level(value: &YamlValue) -> Option<String> {
    value
        .as_mapping()
        .and_then(|map| map.get(YamlValue::String("thinking".to_string())))
        .and_then(YamlValue::as_mapping)
        .and_then(|thinking| thinking.get(YamlValue::String("defaultLevel".to_string())))
        .and_then(YamlValue::as_str)
        .and_then(normalize_custom_provider_thinking_level)
}

fn json_model_thinking_levels(value: &JsonValue) -> Vec<String> {
    let Some(object) = value.as_object() else {
        return Vec::new();
    };
    let supported_levels = object
        .get("metadata")
        .and_then(|metadata| metadata.get("supported_reasoning_levels"))
        .or_else(|| object.get("supported_reasoning_levels"))
        .and_then(JsonValue::as_array)
        .map(|levels| {
            levels
                .iter()
                .filter_map(|level| match level {
                    JsonValue::String(level) => Some(level.clone()),
                    JsonValue::Object(level) => level
                        .get("effort")
                        .and_then(JsonValue::as_str)
                        .map(str::to_string),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .filter(|levels| !levels.is_empty());
    if let Some(levels) = supported_levels {
        return normalize_custom_provider_thinking_levels(&levels);
    }
    if let Some(levels) = object
        .get("thinking")
        .and_then(JsonValue::as_object)
        .and_then(|thinking| thinking.get("efforts").or_else(|| thinking.get("levels")))
        .and_then(JsonValue::as_array)
    {
        return normalize_custom_provider_thinking_levels(
            &levels
                .iter()
                .filter_map(JsonValue::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>(),
        );
    }
    object
        .get("thinkingLevelMap")
        .and_then(JsonValue::as_object)
        .map(|level_map| {
            normalize_custom_provider_thinking_levels(
                &level_map
                    .iter()
                    .filter(|(_, value)| !value.is_null())
                    .map(|(level, _)| level.clone())
                    .collect::<Vec<_>>(),
            )
        })
        .unwrap_or_default()
}

fn json_model_default_thinking_level(value: &JsonValue) -> Option<String> {
    value.get("metadata").and_then(|metadata| metadata.get("default_reasoning_level"))
        .or_else(|| value.get("default_reasoning_level"))
        .or_else(|| value.get("thinking").and_then(|thinking| thinking.get("defaultLevel")))
        .and_then(JsonValue::as_str)
        .and_then(normalize_custom_provider_thinking_level)
}

fn json_model_is_hidden(value: &JsonValue) -> bool {
    value
        .as_object()
        .and_then(|object| object.get("visibility"))
        .and_then(JsonValue::as_str)
        .is_some_and(|visibility| visibility.eq_ignore_ascii_case("hide"))
}

fn custom_provider_models(value: Option<&YamlValue>) -> Vec<CustomProviderModel> {
    let Some(YamlValue::Sequence(values)) = value else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    let mut models = values
        .iter()
        .filter_map(|value| {
            let id = yaml_model_id(value)?;
            if !seen.insert(id.clone()) {
                return None;
            }
            let name = value
                .as_mapping()
                .and_then(|map| mapping_string(map, "name"))
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| id.clone());
            Some(CustomProviderModel {
                id,
                name,
                api: yaml_model_api(value),
                thinking: yaml_model_thinking_levels(value),
                default_thinking_level: yaml_model_default_thinking_level(value),
                configuration: model_metadata::snapshot(&serde_json::to_value(value).unwrap_or(JsonValue::Null)),
            })
        })
        .collect::<Vec<_>>();
    models.sort_by(|left, right| left.id.cmp(&right.id));
    models
}
fn load_custom_providers(root: &YamlValue) -> Vec<CustomProvider> {
    let Some(providers) = nested_value(root, &["providers"]).and_then(YamlValue::as_mapping) else {
        return Vec::new();
    };

    let mut result = providers
        .iter()
        .filter_map(|(key, value)| {
            let id = key.as_str()?.trim();
            let config = value.as_mapping()?;
            let models =
                custom_provider_models(config.get(YamlValue::String("models".to_string())));
            if models.is_empty() {
                return None;
            }
            let provider_api = mapping_string(config, "api");
            Some(CustomProvider {
                id: id.to_string(),
                base_url: mapping_string(config, "baseUrl").unwrap_or_default(),
                api: inferred_custom_provider_api(provider_api, &models),
                api_key_configured: mapping_string(config, "apiKey")
                    .is_some_and(|key| !key.trim().is_empty()),
                auth_none: mapping_string(config, "auth").is_some_and(|auth| auth.trim() == "none"),
                models,
            })
        })
        .collect::<Vec<_>>();
    result.sort_by(|left, right| left.id.cmp(&right.id));
    result
}

fn json_model_id(value: &JsonValue) -> Option<String> {
    let id = match value {
        JsonValue::String(id) => id.trim().to_string(),
        JsonValue::Object(object) => ["id", "slug", "model", "name"]
            .iter()
            .find_map(|key| object.get(*key).and_then(JsonValue::as_str))
            .unwrap_or_default()
            .trim()
            .to_string(),
        _ => return None,
    };
    (!id.is_empty()).then_some(id)
}

fn ensure_mapping(value: &mut YamlValue) -> &mut Mapping {
    if !matches!(value, YamlValue::Mapping(_)) {
        *value = YamlValue::Mapping(Mapping::new());
    }
    match value {
        YamlValue::Mapping(map) => map,
        _ => unreachable!("ensure_mapping always returns a mapping"),
    }
}

fn remove_nested_value(root: &mut YamlValue, path: &[&str]) -> bool {
    let Some(map) = root.as_mapping_mut() else {
        return false;
    };
    let key = YamlValue::String(path[0].to_string());
    if path.len() == 1 {
        map.remove(&key);
        return map.is_empty();
    }

    let remove_child = map
        .get_mut(&key)
        .is_some_and(|child| remove_nested_value(child, &path[1..]));
    if remove_child {
        map.remove(&key);
    }
    map.is_empty()
}

fn set_nested_value(root: &mut YamlValue, path: &[&str], value: Option<YamlValue>) {
    if path.is_empty() {
        return;
    }
    let Some(value) = value else {
        remove_nested_value(root, path);
        return;
    };

    let mut current = root;
    for segment in &path[..path.len() - 1] {
        let map = ensure_mapping(current);
        let key = YamlValue::String((*segment).to_string());
        current = map
            .entry(key)
            .or_insert_with(|| YamlValue::Mapping(Mapping::new()));
    }

    let map = ensure_mapping(current);
    let key = YamlValue::String(path[path.len() - 1].to_string());
    map.insert(key, value);
}

fn set_nested_string(root: &mut YamlValue, path: &[&str], value: &str) {
    let trimmed = value.trim();
    set_nested_value(
        root,
        path,
        (!trimmed.is_empty()).then(|| YamlValue::String(trimmed.to_string())),
    );
}

fn set_nested_optional_string(root: &mut YamlValue, path: &[&str], value: Option<&str>) {
    set_nested_value(
        root,
        path,
        value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| YamlValue::String(value.to_string())),
    );
}

fn set_nested_optional_bool(root: &mut YamlValue, path: &[&str], value: Option<bool>) {
    set_nested_value(root, path, value.map(YamlValue::Bool));
}

fn set_nested_optional_u64(root: &mut YamlValue, path: &[&str], value: Option<u64>) {
    set_nested_value(
        root,
        path,
        value.map(|value| YamlValue::Number(value.into())),
    );
}

fn set_nested_fallback_chains(root: &mut YamlValue, path: &[&str], chains: &[FallbackChain]) {
    let value = if chains.is_empty() {
        None
    } else {
        let mut map = Mapping::new();
        for chain in chains {
            map.insert(
                YamlValue::String(chain.target.trim().to_string()),
                YamlValue::Sequence(
                    chain
                        .selectors
                        .iter()
                        .map(|selector| YamlValue::String(selector.trim().to_string()))
                        .collect(),
                ),
            );
        }
        Some(YamlValue::Mapping(map))
    };
    set_nested_value(root, path, value);
}

fn valid_custom_provider_id(id: &str) -> bool {
    let mut characters = id.chars();
    matches!(characters.next(), Some(first) if first.is_ascii_alphanumeric())
        && characters.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

fn valid_custom_provider_base_url(base_url: &str) -> bool {
    base_url.starts_with("http://") || base_url.starts_with("https://")
}

fn existing_custom_provider<'a>(root: &'a YamlValue, id: &str) -> Option<&'a YamlValue> {
    nested_value(root, &["providers"])
        .and_then(YamlValue::as_mapping)
        .and_then(|providers| providers.get(YamlValue::String(id.to_string())))
}
fn validate_custom_provider_models(
    provider_id: &str,
    models: &[CustomProviderSaveModel],
) -> Result<(), String> {
    if models.is_empty() {
        return Err(format!(
            "OMP custom provider “{provider_id}”至少需要一个 model"
        ));
    }

    let mut seen_models = HashSet::new();
    for model in models {
        let model_id = model.id.trim();
        if model_id.is_empty() {
            return Err(format!(
                "OMP custom provider “{provider_id}”包含空 model"
            ));
        }
        if !seen_models.insert(model_id.to_string()) {
            return Err(format!(
                "OMP custom provider “{provider_id}”的 model “{model_id}”重复"
            ));
        }
        let api = model.api.trim();
        if !CUSTOM_PROVIDER_APIS.contains(&api) {
            return Err(format!(
                "OMP custom provider “{provider_id}”的 model “{model_id}” Type “{api}”不受支持"
            ));
        }
        model_metadata::validate(&model.configuration)
            .map_err(|error| format!("OMP model “{model_id}”：{error}"))?;
    }
    Ok(())
}

fn validate_custom_providers(
    root: &YamlValue,
    providers: &[CustomProviderSave],
) -> Result<(), String> {
    let mut seen_providers = HashSet::new();
    for provider in providers {
        let id = provider.id.trim();
        if !valid_custom_provider_id(id) {
            return Err(format!(
                "Custom provider “{id}”无效：只能包含字母、数字、-、_、.，且必须以字母或数字开头"
            ));
        }
        if !seen_providers.insert(id.to_string()) {
            return Err(format!("Custom provider “{id}”重复"));
        }

        let base_url = provider.base_url.trim();
        if !valid_custom_provider_base_url(base_url) {
            return Err(format!(
                "Custom provider “{id}”的 Base URL 必须以 http:// 或 https:// 开头"
            ));
        }
        let api = provider.api.trim();
        if !CUSTOM_PROVIDER_APIS.contains(&api) {
            return Err(format!(
                "Custom provider “{id}”的 Type “{api}”不受 OMP 支持"
            ));
        }
        validate_custom_provider_models(id, &provider.models)?;

        let existing = existing_custom_provider(root, id).and_then(YamlValue::as_mapping);
        let existing_models = existing
            .and_then(|config| config.get(YamlValue::String("models".to_string())))
            .and_then(YamlValue::as_sequence);
        for model in &provider.models {
            let mut effective = existing_models
                .and_then(|models| models.iter().find(|value| yaml_model_id(value).as_deref() == Some(model.id.trim())))
                .and_then(|value| serde_json::to_value(value).ok())
                .map(|value| model_metadata::configured(&value))
                .unwrap_or_default();
            model_metadata::merge(&mut effective, &model.configuration);
            model_metadata::validate(&effective)
                .map_err(|error| format!("OMP model “{}”：{error}", model.id.trim()))?;
            model_metadata::validate_api(&effective, model.api.trim())
                .map_err(|error| format!("OMP model “{}”：{error}", model.id.trim()))?;
        }
        let existing_key = existing
            .and_then(|config| mapping_string(config, "apiKey"))
            .filter(|key| !key.trim().is_empty());
        let existing_keyless = existing
            .and_then(|config| mapping_string(config, "auth"))
            .is_some_and(|auth| auth.trim() == "none");
        let requested_key = provider.api_key.as_deref().map(str::trim);
        if provider.api_key.is_some() && requested_key.is_some_and(str::is_empty) {
            return Err(format!("Custom provider “{id}”的 API key 不能为空"));
        }
        if requested_key.is_none() && existing_key.is_none() && !existing_keyless {
            return Err(format!("Custom provider “{id}”需要 API key"));
        }
    }
    Ok(())
}

fn validate_removed_custom_providers(
    providers: &[CustomProviderSave],
    removed: &[String],
) -> Result<(), String> {
    let provider_ids = providers
        .iter()
        .map(|provider| provider.id.trim())
        .collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    for provider in removed {
        let id = provider.trim();
        if !valid_custom_provider_id(id) {
            return Err(format!("待删除的 Custom provider “{id}”无效"));
        }
        if !seen.insert(id) {
            return Err(format!("待删除的 Custom provider “{id}”重复"));
        }
        if provider_ids.contains(id) {
            return Err(format!("Custom provider “{id}”不能同时保存和删除"));
        }
    }
    Ok(())
}

fn custom_provider_yaml(existing: Option<&YamlValue>, provider: &CustomProviderSave) -> YamlValue {
    let mut config = existing
        .and_then(YamlValue::as_mapping)
        .cloned()
        .unwrap_or_default();
    config.insert(
        YamlValue::String("baseUrl".to_string()),
        YamlValue::String(provider.base_url.trim().to_string()),
    );
    config.remove(&YamlValue::String("api".to_string()));

    if let Some(api_key) = provider
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
    {
        config.insert(
            YamlValue::String("apiKey".to_string()),
            YamlValue::String(api_key.to_string()),
        );
        if mapping_string(&config, "auth").as_deref() == Some("none") {
            config.remove(&YamlValue::String("auth".to_string()));
        }
    }

    let model_apis = provider
        .models
        .iter()
        .map(|model| (model.id.trim(), model.api.trim()))
        .collect::<BTreeMap<_, _>>();
    let model_metadata = provider
        .models
        .iter()
        .map(|model| (model.id.trim(), model))
        .collect::<BTreeMap<_, _>>();
    let selected_models = model_apis.keys().copied().collect::<HashSet<_>>();
    let mut retained_models = Vec::new();
    let mut retained_ids = HashSet::new();
    if let Some(existing_models) = config
        .get(YamlValue::String("models".to_string()))
        .and_then(YamlValue::as_sequence)
    {
        for model in existing_models {
            let Some(id) = yaml_model_id(model) else {
                continue;
            };
            let Some(api) = model_apis.get(id.as_str()) else {
                continue;
            };
            if selected_models.contains(id.as_str()) && retained_ids.insert(id.clone()) {
                let mut model_config = model.as_mapping().cloned().unwrap_or_default();
                model_config.insert(
                    YamlValue::String("id".to_string()),
                    YamlValue::String(id.clone()),
                );
                model_config.insert(
                    YamlValue::String("api".to_string()),
                    YamlValue::String((*api).to_string()),
                );
                if let Some(metadata) = model_metadata.get(id.as_str()) {
                    apply_omp_model_capabilities(&mut model_config, metadata);
                }
                apply_omp_session_header_compat(&mut model_config, *api);
                retained_models.push(YamlValue::Mapping(model_config));
            }
        }
    }
    for model in &provider.models {
        let id = model.id.trim();
        if retained_ids.insert(id.to_string()) {
            let mut model_value = Mapping::new();
            model_value.insert(
                YamlValue::String("id".to_string()),
                YamlValue::String(id.to_string()),
            );
            model_value.insert(
                YamlValue::String("api".to_string()),
                YamlValue::String(model.api.trim().to_string()),
            );
            apply_omp_model_capabilities(&mut model_value, model);
            apply_omp_session_header_compat(&mut model_value, model.api.trim());
            retained_models.push(YamlValue::Mapping(model_value));
        }
    }
    retained_models.sort_by(|left, right| yaml_model_id(left).cmp(&yaml_model_id(right)));
    config.insert(
        YamlValue::String("models".to_string()),
        YamlValue::Sequence(retained_models),
    );
    YamlValue::Mapping(config)
}

fn set_custom_providers(
    root: &mut YamlValue,
    providers: &[CustomProviderSave],
    removed: &[String],
) {
    let mut next = nested_value(root, &["providers"])
        .and_then(YamlValue::as_mapping)
        .cloned()
        .unwrap_or_default();
    for provider in removed {
        next.remove(&YamlValue::String(provider.trim().to_string()));
    }
    for provider in providers {
        let id = provider.id.trim();
        let existing = next.get(YamlValue::String(id.to_string())).cloned();
        next.insert(
            YamlValue::String(id.to_string()),
            custom_provider_yaml(existing.as_ref(), provider),
        );
    }
    let value = (!next.is_empty()).then_some(YamlValue::Mapping(next));
    set_nested_value(root, &["providers"], value);
}

fn model_from_parts(
    provider: String,
    id: String,
    name: String,
    reasoning: bool,
    thinking: Vec<String>,
    input: Vec<String>,
    context_window: Option<u64>,
    max_tokens: Option<u64>,
    cost: Option<ModelCost>,
) -> Option<ModelOption> {
    if provider.is_empty() || id.is_empty() {
        return None;
    }
    Some(ModelOption {
        selector: format!("{provider}/{id}"),
        provider,
        id,
        name,
        reasoning,
        thinking,
        input,
        context_window,
        max_tokens,
        cost,
        kind: String::new(),
    })
}

fn json_string(object: &serde_json::Map<String, JsonValue>, key: &str) -> String {
    object
        .get(key)
        .and_then(JsonValue::as_str)
        .unwrap_or_default()
        .to_string()
}

fn custom_provider_models_url(base_url: &str) -> Result<String, String> {
    let base_url = base_url.trim().trim_end_matches('/');
    if !valid_custom_provider_base_url(base_url) {
        return Err("Base URL 必须以 http:// 或 https:// 开头".to_string());
    }
    Ok(if base_url.ends_with("/models") {
        base_url.to_string()
    } else {
        format!("{base_url}/models")
    })
}

fn custom_provider_model_urls(base_url: &str) -> Result<Vec<String>, String> {
    let base_url = base_url.trim().trim_end_matches('/');
    let has_version_suffix = base_url.rsplit_once("/v").is_some_and(|(_, suffix)| {
        !suffix.is_empty() && suffix.chars().all(|character| character.is_ascii_digit())
    });
    if has_version_suffix {
        return Ok(vec![custom_provider_models_url(base_url)?]);
    }
    if !valid_custom_provider_base_url(base_url) {
        return Err("Base URL 必须以 http:// 或 https:// 开头".to_string());
    }
    Ok(vec![
        format!("{base_url}/v1/models"),
        format!("{base_url}/v3/models"),
    ])
}
fn custom_provider_rich_models_url(models_url: &str) -> String {
    let separator = if models_url.contains('?') { '&' } else { '?' };
    format!("{models_url}{separator}client_version={RICH_MODEL_CATALOG_CLIENT_VERSION}")
}

fn discovery_header_value(name: &HeaderName, value: &str) -> Result<HeaderValue, String> {
    let mut header = HeaderValue::from_str(value)
        .map_err(|_| format!("Provider 请求头 {name} 的值无效"))?;
    header.set_sensitive(true);
    Ok(header)
}

fn resolve_discovery_header(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    let Some(command) = raw.strip_prefix('!') else {
        return Ok(env::var(raw).ok().filter(|value| !value.is_empty())
            .unwrap_or_else(|| raw.to_owned()));
    };
    // ponytail: read a single token file directly; never evaluate shell commands.
    let unsupported = || "只支持用 !cat 引用请求头文件".to_string();
    let (program, arg) = command.trim().split_once(char::is_whitespace).ok_or_else(unsupported)?;
    if !matches!(program, "cat" | "/bin/cat" | "/usr/bin/cat") {
        return Err(unsupported());
    }
    let arg = arg.trim();
    let arg = arg.strip_prefix('"').and_then(|arg| arg.strip_suffix('"')).unwrap_or(arg);
    let path = if let Some(relative) = arg.strip_prefix("~/")
        .or_else(|| arg.strip_prefix("$HOME/"))
        .or_else(|| arg.strip_prefix("${HOME}/"))
    {
        home_dir()?.join(relative)
    } else if Path::new(arg).is_absolute() {
        PathBuf::from(arg)
    } else {
        return Err(unsupported());
    };
    let unreadable = || "无法读取请求头文件".to_string();
    if !fs::metadata(&path).is_ok_and(|meta| meta.is_file() && meta.len() <= 8192) {
        return Err(unreadable());
    }
    let value = fs::read_to_string(path).map_err(|_| unreadable())?;
    if value.trim().is_empty() {
        return Err("请求头文件为空".to_string());
    }
    Ok(value)
}

fn discovery_client(
    api: &str,
    api_key: &str,
    draft_key: bool,
    configured_headers: Option<&YamlValue>,
) -> Result<reqwest::Client, String> {
    let x_api_key = HeaderName::from_static("x-api-key");
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    let bearer = discovery_header_value(&AUTHORIZATION, &format!("Bearer {api_key}"))?;
    headers.insert(AUTHORIZATION, bearer);
    if api == "anthropic-messages" {
        let key = discovery_header_value(&x_api_key, api_key)?;
        headers.insert(x_api_key.clone(), key);
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
    }
    if let Some(configured_headers) = configured_headers {
        let configured_headers = configured_headers.as_mapping()
            .ok_or_else(|| "Provider headers 必须是映射".to_string())?;
        for (name, raw) in configured_headers {
            let name = name.as_str().and_then(|name| HeaderName::from_bytes(name.as_bytes()).ok())
                .ok_or_else(|| "Provider 请求头名称无效".to_string())?;
            if draft_key && (name == AUTHORIZATION || (api == "anthropic-messages" && name == x_api_key)) {
                continue;
            }
            let raw = raw.as_str().ok_or_else(|| format!("Provider 请求头 {name} 必须是字符串"))?;
            let resolved = resolve_discovery_header(raw)
                .map_err(|error| format!("Provider 请求头 {name}：{error}"))?;
            if !resolved.trim().is_empty() {
                let value = discovery_header_value(&name, resolved.trim())?;
                headers.insert(name, value);
            }
        }
    }
    let redirect_limit = reqwest::redirect::Policy::default();
    reqwest::Client::builder()
        .default_headers(headers)
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().first().is_some_and(|url| url.origin() == attempt.url().origin()) {
                redirect_limit.redirect(attempt)
            } else {
                attempt.stop()
            }
        }))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| format!("创建 model discovery 客户端失败: {error}"))
}

async fn fetch_optional_custom_provider_rich_catalog(
    client: &reqwest::Client,
    models_url: &str,
) -> Result<Option<Vec<CustomProviderModel>>, String> {
    let rich_url = custom_provider_rich_models_url(models_url);
    let response = client.get(&rich_url)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|error| format!("读取 provider model 列表失败（GET {rich_url}）：{error}"))?;
    let status = response.status();
    if matches!(status, reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::METHOD_NOT_ALLOWED) {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(format!("读取 provider model 列表失败（HTTP {status}）"));
    }
    let body = response.text().await
        .map_err(|error| format!("读取 provider model 响应失败：{error}"))?;
    let models = parse_custom_provider_model_list(&body)?;
    Ok((!models.is_empty()).then_some(models))
}

fn resolve_custom_provider_api_key(raw_key: &str) -> Option<String> {
    let raw_key = raw_key.trim();
    if raw_key.is_empty() {
        return None;
    }
    let resolved = env::var(raw_key).unwrap_or_else(|_| raw_key.to_string());
    (!resolved.trim().is_empty()).then(|| resolved.trim().to_string())
}
fn discovered_custom_provider_model_api(value: &JsonValue) -> Option<String> {
    let object = value.as_object()?;
    if let Some(api) = object
        .get("api")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|api| CUSTOM_PROVIDER_APIS.contains(api))
    {
        return Some(api.to_string());
    }

    let endpoint_types = object
        .get("supported_endpoint_types")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(JsonValue::as_str)
        .map(str::to_ascii_lowercase)
        .collect::<HashSet<_>>();
    let candidate = if endpoint_types
        .iter()
        .any(|endpoint| endpoint == "anthropic" || endpoint == "anthropic-messages")
    {
        "anthropic-messages"
    } else if endpoint_types
        .iter()
        .any(|endpoint| endpoint == "openai-responses" || endpoint == "responses")
    {
        "openai-responses"
    } else if endpoint_types.iter().any(|endpoint| {
        endpoint == "openai" || endpoint == "openai-completions" || endpoint == "chat-completions"
    }) {
        "openai-completions"
    } else {
        return None;
    };
    Some(candidate.to_string())
}

fn parse_custom_provider_model_list(
    content: &str,
) -> Result<Vec<CustomProviderModel>, String> {
    let document: JsonValue = serde_json::from_str(content)
        .map_err(|error| format!("provider model 列表不是有效 JSON: {error}"))?;
    let values = match &document {
        JsonValue::Array(values) => Some(values),
        JsonValue::Object(object) => ["data", "models", "items"]
            .iter()
            .find_map(|key| object.get(*key).and_then(JsonValue::as_array)),
        _ => None,
    }
    .ok_or_else(|| "provider model 列表缺少 data/models/items 数组".to_string())?;

    let mut seen = HashSet::new();
    let mut models = Vec::new();
    for value in values {
        let (id, name, api) = match value {
            JsonValue::String(id) => (id.trim().to_string(), id.trim().to_string(), None),
            JsonValue::Object(object) => {
                let id = json_model_id(value).unwrap_or_default();
                let name = ["name", "display_name", "displayName"]
                    .iter()
                    .find_map(|key| object.get(*key).and_then(JsonValue::as_str))
                    .unwrap_or(&id)
                    .trim()
                    .to_string();
                let api = discovered_custom_provider_model_api(value);
                (id, name, api)
            }
            _ => continue,
        };
        if id.is_empty() || !seen.insert(id.clone()) {
            continue;
        }
        let hidden = json_model_is_hidden(value);
        let mut configuration = if hidden {
            model_metadata::Configuration::new()
        } else {
            model_metadata::discover(value)
                .map_err(|error| format!("Model “{id}”：{error}"))?
        };
        if !hidden && name != id && !name.is_empty() {
            configuration.insert("name".to_string(), JsonValue::String(name.clone()));
        }
        models.push(CustomProviderModel {
            id,
            name: if name.is_empty() {
                "Unnamed model".to_string()
            } else {
                name
            },
            api,
            thinking: if hidden {
                Vec::new()
            } else {
                json_model_thinking_levels(value)
            },
            default_thinking_level: if hidden {
                None
            } else {
                json_model_default_thinking_level(value)
            },
            configuration,
        });
    }
    models.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(models)
}
fn merge_custom_provider_model_capabilities(
    mut models: Vec<CustomProviderModel>,
    rich_models: Vec<CustomProviderModel>,
) -> Vec<CustomProviderModel> {
    let rich_by_id = rich_models
        .into_iter()
        .map(|model| (model.id.clone(), model))
        .collect::<BTreeMap<_, _>>();
    for model in &mut models {
        let Some(rich_model) = rich_by_id.get(&model.id) else {
            continue;
        };
        let mut configuration = rich_model.configuration.clone();
        model_metadata::merge(&mut configuration, &model.configuration);
        model.configuration = configuration;
        if model.name == model.id && rich_model.name != rich_model.id {
            model.name = rich_model.name.clone();
            model.configuration.insert("name".to_string(), JsonValue::String(model.name.clone()));
        }
        if model.api.is_none() {
            model.api = rich_model.api.clone();
        }
        if !rich_model.thinking.is_empty() {
            model.thinking = rich_model.thinking.clone();
            model.default_thinking_level = rich_model.default_thinking_level.clone();
        }
    }
    models.sort_by(|left, right| left.id.cmp(&right.id));
    models
}

fn model_effort_metadata_mode(api: &str, model_id: &str) -> Option<&'static str> {
    match api {
        "openai-completions" | "openai-responses" | "openai-codex-responses"
        | "azure-openai-responses" => Some("effort"),
        "anthropic-messages" => {
            let name = model_id.rsplit('/').next()?.strip_prefix("claude-")?;
            let (family, revision) = name.split_once('-')?;
            if !matches!(family, "sonnet" | "opus" | "haiku" | "fable" | "mythos") {
                return None;
            }
            let mut revision = revision.split(['.', '-']);
            let major = revision.next()?.parse::<u32>().ok()?;
            // A date suffix on a major-only model is not a minor revision.
            let minor = revision.next().filter(|part| part.len() <= 2)
                .and_then(|part| part.parse::<u32>().ok());
            let adaptive = major >= 5
                || (matches!(family, "sonnet" | "opus")
                    && major == 4 && minor.is_some_and(|minor| minor >= 6));
            // Leave legacy budget models and opaque aliases to OMP's native inference.
            adaptive.then_some("anthropic-adaptive")
        }
        _ => None,
    }
}
/**
 * Custom provider routes do not match OMP's built-in OpenCode provider names;
 * mirror the generated session identity for the OpenAI transports so a proxy
 * can forward it to an upstream session-affinity header.
 */
fn apply_omp_session_header_compat(config: &mut Mapping, api: &str) {
    if !matches!(api, "openai-completions" | "openai-responses") {
        return;
    }
    let compat_key = YamlValue::String("compat".to_string());
    if !config.contains_key(&compat_key) {
        config.insert(compat_key.clone(), YamlValue::Mapping(Mapping::new()));
    }
    let Some(compat) = config
        .get_mut(&compat_key)
        .and_then(YamlValue::as_mapping_mut)
    else {
        return;
    };
    let header_key = YamlValue::String("promptCacheSessionHeader".to_string());
    if !compat.contains_key(&header_key) {
        compat.insert(header_key, YamlValue::String("X-Session-Id".to_string()));
    }
}

fn model_effort_levels(model: &CustomProviderSaveModel) -> Vec<String> {
    normalize_custom_provider_thinking_levels(&model.thinking)
        .into_iter()
        .filter(|level| level != "off")
        .collect()
}

fn apply_omp_model_capabilities(config: &mut Mapping, model: &CustomProviderSaveModel) {
    model_metadata::apply_yaml(config, &model.configuration);
    if config.contains_key(YamlValue::String("thinking".to_string())) {
        if model.configuration.contains_key("thinking") {
            config.entry(YamlValue::String("reasoning".to_string()))
                .or_insert(YamlValue::Bool(true));
        }
        return;
    }
    let Some(mode) = model_effort_metadata_mode(model.api.trim(), model.id.trim()) else {
        return;
    };
    let efforts = model_effort_levels(model);
    if efforts.is_empty() {
        return;
    }
    config.entry(YamlValue::String("reasoning".to_string()))
        .or_insert(YamlValue::Bool(true));
    let mut thinking = Mapping::new();
    thinking.insert(
        YamlValue::String("mode".to_string()),
        YamlValue::String(mode.to_string()),
    );
    thinking.insert(
        YamlValue::String("efforts".to_string()),
        YamlValue::Sequence(efforts.iter().cloned().map(YamlValue::String).collect()),
    );
    let default_level = model
        .default_thinking_level
        .as_deref()
        .and_then(normalize_custom_provider_thinking_level)
        .filter(|level| efforts.contains(level) && level != "off");
    if let Some(default_level) = default_level {
        thinking.insert(
            YamlValue::String("defaultLevel".to_string()),
            YamlValue::String(default_level),
        );
    }
    config.insert(
        YamlValue::String("thinking".to_string()),
        YamlValue::Mapping(thinking),
    );
}

fn parse_models_json(content: &str, read_kind: bool) -> Result<Vec<ModelOption>, String> {
    let document: JsonValue = serde_json::from_str(content)
        .map_err(|error| format!("OMP 模型列表不是有效 JSON: {error}"))?;
    let Some(models) = document.get("models").and_then(JsonValue::as_array) else {
        return Err("OMP 模型 JSON 缺少 models 数组".to_string());
    };

    let mut result = Vec::new();
    for item in models {
        let Some(object) = item.as_object() else {
            continue;
        };
        let provider = json_string(object, "provider");
        let id = json_string(object, "id");
        let selector = json_string(object, "selector");
        let name = {
            let value = json_string(object, "name");
            if value.is_empty() {
                id.clone()
            } else {
                value
            }
        };
        let reasoning = object
            .get("reasoning")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false);
        let thinking = object
            .get("thinking")
            .and_then(JsonValue::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(JsonValue::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let input = object
            .get("input")
            .and_then(JsonValue::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(JsonValue::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let context_window = object.get("contextWindow").and_then(JsonValue::as_u64);
        let max_tokens = object.get("maxTokens").and_then(JsonValue::as_u64);
        let cost = object.get("cost").and_then(|value| {
            let cost = value.as_object()?;
            Some(ModelCost {
                input: cost.get("input")?.as_f64()?,
                output: cost.get("output")?.as_f64()?,
                cache_read: cost.get("cacheRead")?.as_f64()?,
                cache_write: cost.get("cacheWrite")?.as_f64()?,
            })
        });

        if let Some(mut model) = model_from_parts(
            provider,
            id,
            name,
            reasoning,
            thinking,
            input,
            context_window,
            max_tokens,
            cost,
        ) {
            // OMP's selector is the authoritative identity; older output has
            // no selector field, so retain the provider/id fallback.
            if !selector.is_empty() {
                model.selector = selector;
            }
            // Only the `--kind all` payload carries `kind`; the chat list omits it.
            if read_kind {
                let value = json_string(object, "kind");
                model.kind = if value.is_empty() { "chat".to_string() } else { value };
            }
            result.push(model);
        }
    }
    Ok(dedupe_models(result))
}

fn dedupe_models(models: Vec<ModelOption>) -> Vec<ModelOption> {
    let mut seen = HashSet::new();
    let mut unique = models
        .into_iter()
        .filter(|model| seen.insert(model.selector.clone()))
        .collect::<Vec<_>>();
    unique.sort_by(|left, right| {
        left.provider
            .cmp(&right.provider)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.id.cmp(&right.id))
    });
    unique
}

fn default_system_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(home) = home_dir() {
        paths.push(home.join(".local").join("bin"));
        paths.push(home.join(".bun").join("bin"));
        paths.push(home.join(".cargo").join("bin"));
        paths.push(home.join(".nvm").join("versions").join("node"));
    }
    #[cfg(target_os = "macos")]
    {
        paths.push(PathBuf::from("/opt/homebrew/bin"));
        paths.push(PathBuf::from("/opt/homebrew/sbin"));
        paths.push(PathBuf::from("/usr/local/bin"));
        paths.push(PathBuf::from("/usr/local/sbin"));
    }
    #[cfg(unix)]
    {
        paths.push(PathBuf::from("/usr/bin"));
        paths.push(PathBuf::from("/bin"));
        paths.push(PathBuf::from("/usr/sbin"));
        paths.push(PathBuf::from("/sbin"));
    }
    paths
}

fn build_subprocess_path(custom_parent: Option<&Path>) -> std::ffi::OsString {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();

    if let Some(parent) = custom_parent.filter(|p| !p.as_os_str().is_empty()) {
        if seen.insert(parent.to_path_buf()) {
            paths.push(parent.to_path_buf());
        }
    }

    if let Some(existing) = env::var_os("PATH") {
        for p in env::split_paths(&existing) {
            if seen.insert(p.clone()) {
                paths.push(p);
            }
        }
    }

    for p in default_system_paths() {
        if seen.insert(p.clone()) {
            paths.push(p);
        }
    }

    env::join_paths(paths.into_iter()).unwrap_or_default()
}

fn executable_candidates(custom_path: Option<&str>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(custom) = custom_path.map(str::trim).filter(|value| !value.is_empty()) {
        candidates.push(PathBuf::from(custom));
    }
    if let Some(path) = env::var_os("PATH") {
        candidates.extend(env::split_paths(&path).map(|directory| directory.join("omp")));
    }
    if let Ok(home) = home_dir() {
        candidates.push(home.join(".local").join("bin").join("omp"));
        candidates.push(home.join(".bun").join("bin").join("omp"));
    }
    candidates.push(PathBuf::from("omp"));
    let mut seen = HashSet::new();
    candidates.retain(|candidate| seen.insert(candidate.clone()));
    candidates
}

fn load_omp_models(custom_path: Option<&str>) -> Result<(Vec<ModelOption>, PathBuf), String> {
    let candidates = executable_candidates(custom_path);
    let mut failure_reasons = Vec::new();
    for candidate in &candidates {
        let mut command = Command::new(candidate);
        command.args(["models", "--json"]).env("NO_COLOR", "1");
        let subprocess_path = build_subprocess_path(candidate.parent());
        command.env("PATH", subprocess_path);
        let output = match command.output() {
            Ok(output) => output,
            Err(error) => {
                failure_reasons.push(format!("{}: {error}", candidate.display()));
                continue;
            }
        };

        if !output.status.success() {
            let status_code = output
                .status
                .code()
                .map_or("terminated".to_string(), |code| code.to_string());
            failure_reasons.push(format!("{}: 进程退出码 {status_code}", candidate.display()));
            continue;
        }
        let models = parse_models_json(&String::from_utf8_lossy(&output.stdout), false)?;
        return Ok((models, candidate.clone()));
    }

    if let Some(custom) = custom_path.map(str::trim).filter(|value| !value.is_empty()) {
        Err(format!(
            "无法通过指定的 omp 路径 “{custom}” 读取模型列表。请检查可执行文件路径与权限。\n尝试详情：\n{}",
            failure_reasons.join("\n")
        ))
    } else {
        Err(format!(
            "未找到可用的 omp 可执行文件或无法通过 `omp models --json` 读取模型。\n请指定 omp 的完整路径。\n尝试详情：\n{}",
            failure_reasons.join("\n")
        ))
    }
}

/// The non-chat candidate pool for the kind roles. `omp models --json` only
/// returns chat rows and carries no `kind`, so this pass is both the metadata
/// source and the source of image/search/tts/stt/judge candidates. Chat roles
/// keep using the plain chat list.
fn load_omp_kind_models(custom_path: Option<&str>) -> Vec<ModelOption> {
    for candidate in executable_candidates(custom_path) {
        let mut command = Command::new(&candidate);
        command
            .args(["models", "--kind", "all", "--json"])
            .env("NO_COLOR", "1");
        command.env("PATH", build_subprocess_path(candidate.parent()));
        let Ok(output) = command.output() else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        if let Ok(models) = parse_models_json(&String::from_utf8_lossy(&output.stdout), true) {
            return models;
        }
    }
    Vec::new()
}

/// `provider/id` -> kind, for save-time role validation.
fn kind_index(models: &[ModelOption]) -> HashMap<String, String> {
    models
        .iter()
        .filter(|model| !model.kind.is_empty())
        .map(|model| (model.selector.clone(), model.kind.clone()))
        .collect()
}

/// Rows usable by at least one kind role, including `web` search candidates.
/// Grounded chat models already enter through `judge`; the CLI does not expose
/// enough metadata to distinguish their web-search capability.
fn kind_role_candidates(models: &[ModelOption]) -> Vec<ModelOption> {
    models
        .iter()
        .filter(|model| {
            KIND_MODEL_ROLES
                .iter()
                .any(|role| accepts_kind_role(role, &model.kind) == Some(true))
        })
        .cloned()
        .collect()
}

/// Whether a catalog entry satisfies a non-chat role, mirroring
/// `MODEL_ROLES[...].accepts` in oh-my-pi's model-roles.ts.
///
/// `None` means "not judgeable" and must never be treated as a rejection. That
/// covers two cases: the metadata pass was unavailable (empty `kind`), and the
/// `web` role's chat branch — OMP accepts a chat model carrying a `webSearch`
/// grounding, but `omp models --json` emits no such field, so a grounded chat
/// model cannot be told apart from an ungrounded one.
fn accepts_kind_role(role: &str, kind: &str) -> Option<bool> {
    if kind.is_empty() {
        return None;
    }
    Some(match role {
        "image" => kind == "image",
        "speech" => kind == "tts",
        "dictation" => kind == "stt",
        "judge" => matches!(kind, "judge" | "tiny" | "chat"),
        "web" => match kind {
            "search" => true,
            "chat" => return None,
            _ => false,
        },
        _ => return None,
    })
}

/// Human-readable requirement shown when a kind role is assigned an
/// incompatible model.
fn kind_role_expectation(role: &str) -> &'static str {
    match role {
        "image" => "image",
        "speech" => "tts",
        "dictation" => "stt",
        "judge" => "judge / tiny / chat",
        "web" => "search（或带 webSearch grounding 的 chat）",
        _ => "对应 kind",
    }
}

/// `provider/id` with a leading `@role` alias and any `:effort` suffix removed.
fn selector_base(selector: &str) -> &str {
    let trimmed = selector.trim();
    let without_alias = trimmed.strip_prefix('@').unwrap_or(trimmed);
    without_alias.split(':').next().unwrap_or(without_alias)
}

/// Reject a kind-role selector that cannot satisfy the role. An unknown model is
/// left alone: OMP reports that separately as "unknown model", and a selector
/// that is not in the catalog yet must not be blocked here.
fn validate_kind_role_selector(
    role: &str,
    selector: &str,
    kind_index: &HashMap<String, String>,
) -> Result<(), String> {
    let Some(kind) = kind_index.get(selector_base(selector)) else {
        return Ok(());
    };
    match accepts_kind_role(role, kind) {
        Some(true) | None => Ok(()),
        Some(false) => Err(format!(
            "Role “{role}”只能使用 kind 为 {} 的模型，但 “{selector}” 的 kind 是 “{kind}”。\
             \n非 chat kind 无法在 models.yml 中自定义（见 oh-my-pi#13272），请改选内建目录中的模型，\
             \n或清空该 Role 以使用内建优先级链。",
            kind_role_expectation(role)
        )),
    }
}

/// True when a configured `image` Role is dead because its tool is gated off.
fn image_role_blocked_by_tool_gate(image_selector: Option<&str>, tool_enabled: Option<bool>) -> bool {
    image_selector.is_some_and(|selector| !selector.trim().is_empty()) && tool_enabled != Some(true)
}

fn load_snapshot_from_disk(custom_omp_path: Option<&str>) -> Result<ConfigSnapshot, String> {
    let agent = agent_dir()?;
    let config = config_file(&agent);
    let models_path = models_file(&agent);
    let (root, config_exists) = load_yaml(&config)?;
    let (models_root, _) = load_models_yaml(&models_path)?;
    let model_roles = string_map(match &root {
        YamlValue::Mapping(map) => map.get(YamlValue::String("modelRoles".to_string())),
        _ => None,
    });
    let agent_model_overrides = nested_string_map(&root, &["task", "agentModelOverrides"]);
    let routing_settings = RoutingSettings {
        default_thinking_level: optional_string(&root, &["defaultThinkingLevel"]),
        auto_thinking_max_effort: optional_string(&root, &["providers", "autoThinkingMaxEffort"]),
        advisor_enabled: optional_bool(&root, &["advisor", "enabled"]),
        advisor_sync_backlog: optional_string(&root, &["advisor", "syncBacklog"]),
        advisor_immune_turns: optional_u64(&root, &["advisor", "immuneTurns"]),
        prewalk_enabled: optional_bool(&root, &["prewalk", "enabled"]),
        retry_model_fallback: optional_bool(&root, &["retry", "modelFallback"]),
        retry_fallback_chains: fallback_chains(nested_value(&root, &["retry", "fallbackChains"])),
        retry_fallback_revert_policy: optional_string(&root, &["retry", "fallbackRevertPolicy"]),
    };

    let mut warnings = Vec::new();
    let agent_files = load_agent_files(&agent, &mut warnings);
    let (mut models, model_executable) = load_omp_models(custom_omp_path)?;
    // The chat list carries no `kind` of its own, so enrich it from the
    // `--kind all` pass and derive the kind-role candidate pool from the same rows.
    let kind_models = load_omp_kind_models(custom_omp_path);
    let kinds = kind_index(&kind_models);
    for model in &mut models {
        if let Some(kind) = kinds.get(&model.selector) {
            model.kind = kind.clone();
        }
    }
    let kind_models = kind_role_candidates(&kind_models);

    // The `image` role is inert while its tool is gated off: OMP registers
    // `generate_image` only when `generate_image.enabled` is true, and it
    // defaults to false. Surface that instead of leaving a configured-but-dead
    // role that looks complete in this UI.
    if image_role_blocked_by_tool_gate(
        model_roles.get("image").map(String::as_str),
        optional_bool(&root, &["generate_image", "enabled"]),
    ) {
        warnings.push(
            "已配置 image Role，但 generate_image 工具未启用（generate_image.enabled 默认 false），\
             \n该 Role 目前不会生效。请在 config.yml 中加入 generate_image.enabled: true。"
                .to_string(),
        );
    }

    Ok(ConfigSnapshot {
        config_path: display_path(&config),
        config_exists,
        model_roles,
        agent_model_overrides,
        kind_models,
        models,
        model_source: format!("{} · omp models --json", display_path(&model_executable)),
        agent_files,
        routing_settings,
        custom_providers: load_custom_providers(&models_root),
        custom_providers_version: omp_models_version(&models_root)?,
        warnings,
    })
}

fn write_yaml_atomically(path: &Path, value: &YamlValue) -> Result<(), String> {
    let content = serde_yaml::to_string(value)
        .map_err(|error| format!("生成 {} 失败: {error}", display_path(path)))?;
    write_text_atomically(path, &content)
}

fn valid_agent_file_name(file_name: &str) -> bool {
    let path = Path::new(file_name);
    path.file_name().and_then(|name| name.to_str()) == Some(file_name)
        && path.extension().and_then(|extension| extension.to_str()) == Some("md")
        && !file_name.starts_with('.')
}

fn write_text_atomically(path: &Path, content: &str) -> Result<(), String> {
    fs::create_dir_all(
        path.parent()
            .ok_or_else(|| "无法确定配置目录".to_string())?,
    )
    .map_err(|error| format!("创建配置目录失败: {error}"))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "配置文件名无效".to_string())?;
    let temporary = path.with_file_name(format!(".{file_name}.omp-model-studio.tmp"));
    fs::write(&temporary, content).map_err(|error| format!("写入临时配置失败: {error}"))?;
    fs::rename(&temporary, path).map_err(|error| format!("替换配置文件失败: {error}"))?;
    Ok(())
}

fn validate_routing_settings(settings: &RoutingSettings) -> Result<(), String> {
    if let Some(level) = settings.default_thinking_level.as_deref() {
        if !CONFIGURED_THINKING_LEVELS.contains(&level) {
            return Err(format!("defaultThinkingLevel “{level}”无效"));
        }
    }
    if let Some(level) = settings.auto_thinking_max_effort.as_deref() {
        if !AUTO_THINKING_MAX_EFFORTS.contains(&level) {
            return Err(format!("providers.autoThinkingMaxEffort “{level}”无效"));
        }
    }
    if let Some(backlog) = settings.advisor_sync_backlog.as_deref() {
        if !ADVISOR_SYNC_BACKLOGS.contains(&backlog) {
            return Err(format!("advisor.syncBacklog “{backlog}”无效"));
        }
    }
    if let Some(policy) = settings.retry_fallback_revert_policy.as_deref() {
        if !FALLBACK_REVERT_POLICIES.contains(&policy) {
            return Err(format!("retry.fallbackRevertPolicy “{policy}”无效"));
        }
    }

    let mut targets = HashSet::new();
    for chain in &settings.retry_fallback_chains {
        let target = chain.target.trim();
        if target.is_empty() {
            return Err("Fallback chain 缺少 target".to_string());
        }
        if !targets.insert(target) {
            return Err(format!("Fallback target “{target}”重复"));
        }
        if chain.selectors.is_empty() {
            return Err(format!("Fallback target “{target}”至少需要一个候选模型"));
        }
        let mut selectors = HashSet::new();
        for selector in &chain.selectors {
            let selector = selector.trim();
            if selector.is_empty() {
                return Err(format!("Fallback target “{target}”包含空模型"));
            }
            if selector == target {
                return Err(format!("Fallback target “{target}”不能回退到自身"));
            }
            if !selectors.insert(selector) {
                return Err(format!("Fallback target “{target}”重复使用 “{selector}”"));
            }
        }
    }
    Ok(())
}

#[tauri::command]
async fn discover_custom_provider_models(
    request: CustomProviderDiscoveryRequest,
) -> Result<Vec<CustomProviderModel>, String> {
    let provider_id = request.provider_id.trim();
    if !valid_custom_provider_id(provider_id) {
        return Err("Provider ID 无效".to_string());
    }
    let api = request.api.trim();
    if !CUSTOM_PROVIDER_APIS.contains(&api) {
        return Err(format!("Custom provider Type “{api}”不受 OMP 支持"));
    }

    let base_url = reqwest::Url::parse(request.base_url.trim().trim_end_matches('/'))
        .map_err(|_| "Base URL 无效".to_string())?;
    let matches_base = |saved: &str| {
        reqwest::Url::parse(saved.trim().trim_end_matches('/')).is_ok_and(|url| url == base_url)
    };
    let urls = custom_provider_model_urls(base_url.as_str())?;
    let agent = agent_dir()?;
    let (models_root, _) = load_models_yaml(&models_file(&agent))?;
    let saved = existing_custom_provider(&models_root, provider_id)
        .and_then(YamlValue::as_mapping)
        .filter(|config| config.get("baseUrl").and_then(YamlValue::as_str).is_some_and(matches_base));
    let existing_key = saved.and_then(|config| mapping_string(config, "apiKey"));
    let configured_headers = saved.and_then(|config| config.get("headers"));
    let draft_key = request.api_key.as_deref().map(str::trim).filter(|key| !key.is_empty());
    let api_key = draft_key
        .or(existing_key.as_deref())
        .and_then(resolve_custom_provider_api_key)
        .ok_or_else(|| format!("Custom provider “{provider_id}”需要此 Base URL 的 API key"))?;
    let client = discovery_client(api, &api_key, draft_key.is_some(), configured_headers)?;
    for (index, url) in urls.iter().enumerate() {
        let response = client.get(url)
            .send()
            .await
            .map_err(|error| format!("读取 provider model 列表失败（GET {url}）：{error}"))?;
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND && index + 1 < urls.len() {
            continue;
        }
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(format!(
                "provider 未提供 model discovery endpoint：GET {url} 返回 HTTP 404"
            ));
        }
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(format!(
                "provider model 列表鉴权失败（HTTP {status}），请检查 API key 与 provider 请求头"
            ));
        }
        let body = response
            .text()
            .await
            .map_err(|error| format!("读取 provider model 响应失败：{error}"))?;
        if !status.is_success() {
            return Err(format!("读取 provider model 列表失败（HTTP {status}）"));
        }
        let models = parse_custom_provider_model_list(&body)?;
        if models.is_empty() {
            return Err("接口未返回模型列表，请确认 Base URL 与 API key".to_string());
        }
        let rich_models = fetch_optional_custom_provider_rich_catalog(&client, url)
            .await?
            .unwrap_or_default();
        return Ok(merge_custom_provider_model_capabilities(models, rich_models));
    }
    Err("provider model discovery 没有可用 endpoint".to_string())
}

const STALE_CUSTOM_PROVIDER_STORE: &str =
    "Provider 配置已在外部修改。请关闭编辑窗口，点击“重新读取”后重试。";

/// Use a process-local hash key because the document includes credentials and
/// only the opaque version token crosses IPC.
fn document_version(serialized: &str) -> String {
    // LazyLock is newer than the crate's declared MSRV (1.77.2).
    static HASHER: OnceLock<RandomState> = OnceLock::new();
    let fingerprint = HASHER.get_or_init(RandomState::new).hash_one(serialized);
    format!("{fingerprint:016x}")
}

fn omp_models_version(root: &YamlValue) -> Result<String, String> {
    serde_yaml::to_string(root)
        .map(|serialized| document_version(&serialized))
        .map_err(|error| format!("序列化 models.yml 失败: {error}"))
}

#[tauri::command]
fn save_custom_providers(
    request: CustomProviderSaveRequest,
) -> Result<CustomProviderStore, String> {
    let agent = agent_dir()?;
    let models_path = models_file(&agent);
    let (mut root, models_exists) = load_models_yaml(&models_path)?;
    if omp_models_version(&root)? != request.expected_version {
        return Err(STALE_CUSTOM_PROVIDER_STORE.to_string());
    }
    validate_custom_providers(&root, &request.custom_providers)?;
    validate_removed_custom_providers(&request.custom_providers, &request.removed_custom_providers)?;
    set_custom_providers(
        &mut root,
        &request.custom_providers,
        &request.removed_custom_providers,
    );
    let version = if models_exists
        || !request.custom_providers.is_empty()
        || !request.removed_custom_providers.is_empty()
    {
        let content = serde_yaml::to_string(&root)
            .map_err(|error| format!("生成 {} 失败: {error}", display_path(&models_path)))?;
        write_text_atomically(&models_path, &content)?;
        document_version(&content)
    } else {
        request.expected_version
    };
    Ok(CustomProviderStore {
        custom_providers: load_custom_providers(&root),
        version,
    })
}

#[tauri::command]
fn open_config_file(kind: String) -> Result<(), String> {
    let agent = agent_dir()?;
    let kind = kind.trim();
    let path = editable_file(&agent, kind)?;
    if !path.exists() {
        let next_step = if kind == "models" {
            "请先保存一个 Custom Provider"
        } else {
            "请先通过窗口右上角 Apply 保存 config.yml"
        };
        return Err(format!("文件 {} 不存在，{next_step}", display_path(&path)));
    }
    reveal_in_file_manager(&path)
}

#[tauri::command]
fn load_snapshot(omp_path: Option<String>) -> Result<ConfigSnapshot, String> {
    load_snapshot_from_disk(omp_path.as_deref())
}
#[tauri::command]
fn save_snapshot(request: SaveRequest) -> Result<ConfigSnapshot, String> {
    let agent = agent_dir()?;
    let config = config_file(&agent);
    let (mut root, _) = load_yaml(&config)?;

    for (role, selector) in &request.model_roles {
        let role = role.trim();
        if !is_system_role(&role) && !valid_custom_role(role) {
            return Err(format!(
                "自定义 Role “{role}”无效：仅允许小写字母开头及小写字母、数字、-、_，且不能覆盖 OMP 系统 Role"
            ));
        }
        if !is_system_role(&role) && selector.trim().is_empty() {
            return Err(format!("自定义 Role “{role}”尚未选择模型"));
        }
    }
    let mut removed_roles = HashSet::new();
    for role in &request.removed_model_roles {
        let role = role.trim();
        if !valid_custom_role(role) {
            return Err(format!("只能删除有效的自定义 Role，收到 “{role}”"));
        }
        if request.model_roles.contains_key(role) {
            return Err(format!("自定义 Role “{role}”不能同时保存和删除"));
        }
        if !removed_roles.insert(role) {
            return Err(format!("待删除的自定义 Role “{role}”重复"));
        }
    }

    // Kind roles resolve against the catalog `kind`, not the chat pool. A
    // mismatch here is what produced an `image` role pointing at a chat model
    // that could never satisfy it, so refuse it at save time instead.
    let touches_kind_role = request
        .model_roles
        .keys()
        .chain(
            request
                .routing_settings
                .retry_fallback_chains
                .iter()
                .map(|chain| &chain.target),
        )
        .any(|role| KIND_MODEL_ROLES.contains(&role.trim()));
    if touches_kind_role {
        let kinds = kind_index(&load_omp_kind_models(request.omp_path.as_deref()));
        for (role, selector) in &request.model_roles {
            let role = role.trim();
            if KIND_MODEL_ROLES.contains(&role) {
                validate_kind_role_selector(role, selector, &kinds)?;
            }
        }
        for chain in &request.routing_settings.retry_fallback_chains {
            let role = chain.target.trim();
            if !KIND_MODEL_ROLES.contains(&role) {
                continue;
            }
            for selector in &chain.selectors {
                validate_kind_role_selector(role, selector, &kinds)?;
            }
        }
    }

    validate_routing_settings(&request.routing_settings)?;

    let mut seen_files = HashSet::new();
    let mut seen_agent_names = HashSet::new();
    let mut agent_writes = Vec::new();
    for agent_file in &request.agent_files {
        if !valid_agent_file_name(&agent_file.file_name) {
            return Err(format!(
                "Agent Markdown 文件名 “{}”无效",
                agent_file.file_name
            ));
        }
        if !seen_files.insert(agent_file.file_name.clone()) {
            return Err(format!(
                "Agent Markdown 文件 “{}”重复",
                agent_file.file_name
            ));
        }
        let (agent_name, _, _) = parse_agent_markdown(&agent_file.agent_markdown)
            .map_err(|error| format!("{} 无效: {error}", agent_file.file_name))?;
        if !seen_agent_names.insert(agent_name.clone()) {
            return Err(format!("Agent 名称 “{agent_name}”重复"));
        }

        let target = agent.join("agents").join(&agent_file.file_name);
        if target.exists() {
            let existing = fs::read_to_string(&target)
                .map_err(|error| format!("无法读取 {}: {error}", display_path(&target)))?;
            let existing_name = parse_agent_markdown(&existing)
                .ok()
                .map(|(name, _, _)| name);
            if existing_name.as_deref() != Some(agent_name.as_str()) {
                return Err(format!(
                    "拒绝以 Agent “{agent_name}”覆盖 {}",
                    display_path(&target)
                ));
            }
        }
        agent_writes.push((target, agent_file.agent_markdown.as_str()));
    }

    for (path, content) in agent_writes {
        write_text_atomically(&path, content)?;
    }

    for role in &request.removed_model_roles {
        set_nested_value(&mut root, &["modelRoles", role.trim()], None);
    }
    for (role, selector) in &request.model_roles {
        // An empty selector on a role the user never configured means "unset",
        // not "clear": writing it would inject `role: ""` keys into a config the
        // user curates by hand. Clearing an existing key still writes through.
        if selector.is_empty() && !model_role_is_set(&root, role.trim()) {
            continue;
        }
        set_nested_string(&mut root, &["modelRoles", role.trim()], selector);
    }
    for (agent_name, selector) in &request.agent_model_overrides {
        set_nested_string(
            &mut root,
            &["task", "agentModelOverrides", agent_name.trim()],
            selector,
        );
    }
    let routing = &request.routing_settings;
    set_nested_optional_string(
        &mut root,
        &["defaultThinkingLevel"],
        routing.default_thinking_level.as_deref(),
    );
    set_nested_optional_string(
        &mut root,
        &["providers", "autoThinkingMaxEffort"],
        routing.auto_thinking_max_effort.as_deref(),
    );
    set_nested_optional_bool(&mut root, &["advisor", "enabled"], routing.advisor_enabled);
    set_nested_optional_string(
        &mut root,
        &["advisor", "syncBacklog"],
        routing.advisor_sync_backlog.as_deref(),
    );
    set_nested_optional_u64(
        &mut root,
        &["advisor", "immuneTurns"],
        routing.advisor_immune_turns,
    );
    set_nested_optional_bool(&mut root, &["prewalk", "enabled"], routing.prewalk_enabled);
    set_nested_optional_bool(
        &mut root,
        &["retry", "modelFallback"],
        routing.retry_model_fallback,
    );
    set_nested_fallback_chains(
        &mut root,
        &["retry", "fallbackChains"],
        &routing.retry_fallback_chains,
    );
    set_nested_optional_string(
        &mut root,
        &["retry", "fallbackRevertPolicy"],
        routing.retry_fallback_revert_policy.as_deref(),
    );
    write_yaml_atomically(&config, &root)?;
    load_snapshot_from_disk(request.omp_path.as_deref())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_snapshot,
            save_snapshot,
            save_custom_providers,
            open_config_file,
            discover_custom_provider_models
        ])
        .run(tauri::generate_context!())
        .expect("error while running OMP Model Studio");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_home() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after unix epoch")
            .as_nanos();
        env::temp_dir().join(format!("omp-model-studio-test-{stamp}"))
    }
    fn saved_model(id: &str, api: &str) -> CustomProviderSaveModel {
        CustomProviderSaveModel {
            id: id.to_string(),
            api: api.to_string(),
            thinking: Vec::new(),
            default_thinking_level: None,
            configuration: model_metadata::Configuration::new(),
        }
    }
    /// Version the backend stamps on the current on-disk store, so tests can
    /// freeze the same value the editor would have read from a snapshot.
    fn omp_store_version() -> Result<String, String> {
        let agent = agent_dir()?;
        let (root, _) = load_models_yaml(&models_file(&agent))?;
        omp_models_version(&root)
    }

    #[test]
    fn custom_provider_validation_checks_retained_metadata_before_save() {
        for (metadata, api) in [
            ("contextWindow: huge", "openai-responses"),
            ("contextWindow: 0", "openai-responses"),
            ("kind: chat", "xai-tts"),
        ] {
            let root: YamlValue = serde_yaml::from_str(&format!(
                "providers:\n  gateway:\n    auth: none\n    models:\n      - id: keep\n        {metadata}\n"
            )).unwrap();
            let provider = CustomProviderSave {
                id: "gateway".to_string(),
                base_url: "https://gateway.example/v1".to_string(),
                api: api.to_string(),
                api_key: None,
                models: vec![saved_model("keep", api)],
            };
            assert!(validate_custom_providers(&root, &[provider]).is_err());
        }
    }

    // `HOME` is process-global; tests that override it to isolate filesystem
    // effects must not run concurrently with each other or a parallel test
    // run can read the wrong test's HOME mid-flight. Cargo runs tests on
    // multiple threads by default, so every such test takes this lock first.
    static HOME_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn save_updates_role_assignments_without_clobbering_other_settings() {
        let _home_guard = HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = temporary_home();
        let agent = home.join(".omp").join("agent");
        fs::create_dir_all(&agent).expect("test config directory should be created");
        fs::write(
            agent.join("config.yml"),
            "modelRoles:\n  default: old/default\n  keep: keep/model\n  remove-me: old/remove\ndisabledProviders:\n  - ollama\n  - path: /repo\n    providers:\n      - scoped-only\ntask:\n  agentModelOverrides:\n    keep-agent: keep/agent\notherSetting: preserved\n",
        )
        .expect("test config should be written");

        let bin_dir = home.join(".bun").join("bin");
        fs::create_dir_all(&bin_dir).expect("mock bin directory should be created");
        let mock_omp = bin_dir.join("omp");
        fs::write(
            &mock_omp,
            "#!/bin/sh\necho '{\"models\":[{\"provider\":\"openai\",\"id\":\"default\",\"name\":\"Default\"},{\"provider\":\"openai\",\"id\":\"vision\",\"name\":\"Vision\"},{\"provider\":\"google\",\"id\":\"tester\",\"name\":\"Tester\"},{\"provider\":\"openai\",\"id\":\"release\",\"name\":\"Release\"}]}'\n",
        )
        .expect("mock omp script should be written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&mock_omp).expect("metadata").permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&mock_omp, permissions).expect("chmod");
        }

        let previous_home = env::var_os("HOME");
        env::set_var("HOME", &home);
        let result = save_snapshot(SaveRequest {
            model_roles: BTreeMap::from([
                ("smol".to_string(), "google/smol:low".to_string()),
                ("default".to_string(), "openai/default:max".to_string()),
                ("slow".to_string(), "anthropic/slow:xhigh".to_string()),
                ("plan".to_string(), "openai/plan:max".to_string()),
                ("task".to_string(), "openai/task:xhigh".to_string()),
                ("vision".to_string(), "openai/vision:xhigh".to_string()),
                ("designer".to_string(), "openai/designer:high".to_string()),
                ("commit".to_string(), "google/commit:low".to_string()),
                ("tiny".to_string(), "google/tiny:low".to_string()),
                ("advisor".to_string(), "openai/advisor:high".to_string()),
                ("tester".to_string(), "google/tester:medium".to_string()),
                (
                    "release-budget".to_string(),
                    "openai/release:high".to_string(),
                ),
            ]),
            removed_model_roles: vec!["remove-me".to_string()],
            agent_model_overrides: BTreeMap::from([
                (
                    "reviewer".to_string(),
                    "anthropic/reviewer:xhigh".to_string(),
                ),
                (
                    "security-reviewer".to_string(),
                    "anthropic/security:xhigh".to_string(),
                ),
            ]),
            agent_files: vec![AgentFileSave {
                file_name: "release-coordinator.md".to_string(),
                agent_markdown: "---\nname: release-coordinator\ndescription: Release verification agent.\nmodel: \"@slow\"\n---\n\nRun the release verification workflow.\n".to_string(),
            }],
            routing_settings: RoutingSettings {
                default_thinking_level: Some("auto".to_string()),
                auto_thinking_max_effort: Some("xhigh".to_string()),
                advisor_enabled: Some(true),
                advisor_sync_backlog: Some("1".to_string()),
                advisor_immune_turns: Some(2),
                prewalk_enabled: Some(true),
                retry_model_fallback: Some(true),
                retry_fallback_chains: vec![FallbackChain {
                    target: "default".to_string(),
                    selectors: vec!["openai/fallback:xhigh".to_string()],
                }],
                retry_fallback_revert_policy: Some("cooldown-expiry".to_string()),
            },
            omp_path: None,
        });
        if let Some(previous_home) = previous_home {
            env::set_var("HOME", previous_home);
        } else {
            env::remove_var("HOME");
        }

        let snapshot = result.expect("saving the isolated config should succeed");
        assert_eq!(
            snapshot.model_roles.get("default"),
            Some(&"openai/default:max".to_string())
        );
        assert_eq!(
            snapshot.model_roles.get("vision"),
            Some(&"openai/vision:xhigh".to_string())
        );
        assert_eq!(
            snapshot.model_roles.get("tester"),
            Some(&"google/tester:medium".to_string())
        );
        assert_eq!(
            snapshot.model_roles.get("release-budget"),
            Some(&"openai/release:high".to_string())
        );
        assert!(!snapshot.model_roles.contains_key("remove-me"));
        assert_eq!(
            snapshot
                .agent_files
                .iter()
                .find(|file| file.name == "release-coordinator")
                .map(|file| (file.file_name.as_str(), file.model.as_str())),
            Some(("release-coordinator.md", "@slow"))
        );
        assert_eq!(
            snapshot.agent_model_overrides.get("reviewer"),
            Some(&"anthropic/reviewer:xhigh".to_string())
        );
        assert_eq!(
            snapshot.agent_model_overrides.get("security-reviewer"),
            Some(&"anthropic/security:xhigh".to_string())
        );
        assert_eq!(
            snapshot.routing_settings.default_thinking_level.as_deref(),
            Some("auto")
        );
        assert_eq!(snapshot.routing_settings.advisor_enabled, Some(true));
        assert_eq!(
            snapshot.routing_settings.retry_fallback_chains[0].selectors,
            vec!["openai/fallback:xhigh".to_string()]
        );
        let agent_markdown = fs::read_to_string(agent.join("agents/release-coordinator.md"))
            .expect("independent agent markdown should be written");
        assert!(agent_markdown.contains("model: \"@slow\""));

        let written =
            fs::read_to_string(agent.join("config.yml")).expect("saved config should exist");
        let parsed: YamlValue =
            serde_yaml::from_str(&written).expect("saved config should remain valid YAML");
        assert_eq!(
            parsed.get("otherSetting").and_then(YamlValue::as_str),
            Some("preserved")
        );
        assert_eq!(
            nested_string_map(&parsed, &["task", "agentModelOverrides"])
                .get("keep-agent")
                .map(String::as_str),
            Some("keep/agent")
        );
        assert_eq!(
            nested_string_map(&parsed, &["modelRoles"])
                .get("release-budget")
                .map(String::as_str),
            Some("openai/release:high")
        );
        assert_eq!(
            string_list(parsed.get("disabledProviders")),
            vec!["ollama".to_string()],
            "Apply must preserve OMP's existing disabledProviders; provider shadow is UI-local"
        );
        let disabled_providers = parsed
            .get("disabledProviders")
            .and_then(YamlValue::as_sequence)
            .cloned()
            .unwrap_or_default();
        let scoped_entry_survived = disabled_providers.iter().any(|entry| {
            entry
                .as_mapping()
                .and_then(|map| map.get(YamlValue::String("providers".to_string())))
                .and_then(YamlValue::as_sequence)
                .is_some_and(|providers| {
                    providers
                        .iter()
                        .any(|provider| provider.as_str() == Some("scoped-only"))
                })
        });
        assert!(
            scoped_entry_survived,
            "path-scoped disabledProviders entries configured outside Model Studio must survive Apply"
        );
        assert!(
            !nested_string_map(&parsed, &["modelRoles"]).contains_key("remove-me"),
            "explicitly removed custom roles must be deleted from config.yml"
        );
        assert_eq!(
            optional_string(&parsed, &["defaultThinkingLevel"]).as_deref(),
            Some("auto")
        );
        assert_eq!(optional_bool(&parsed, &["prewalk", "enabled"]), Some(true));
        assert_eq!(
            fallback_chains(nested_value(&parsed, &["retry", "fallbackChains"]))[0].selectors,
            vec!["openai/fallback:xhigh".to_string()]
        );
        fs::remove_dir_all(home).expect("isolated test home should be removed");
    }

    #[test]
    fn custom_provider_save_writes_only_models_file() {
        let _home_guard = HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = temporary_home();
        let agent = home.join(".omp").join("agent");
        fs::create_dir_all(&agent).expect("test config directory should be created");
        let config_content = "otherSetting: preserved\n";
        fs::write(agent.join("config.yml"), config_content).expect("test config should be written");
        fs::write(
            agent.join("models.yml"),
            "providers:\n  gateway:\n    baseUrl: https://old.example.com/v1\n    api: openai-completions\n    apiKey: OLD_KEY\n    headers:\n      X-Team: keep\n    models:\n      - id: keep\n        name: Keep model\n        input: [text]\n        reasoning: false\n        contextWindow: 123\n        compat:\n          supportsStore: true\n      - id: drop\n        name: Drop model\n  discovery-only:\n    baseUrl: https://discovery.example.com/v1\n    api: openai-completions\n    apiKey: DISCOVERY_KEY\n    discovery:\n      type: openai-models-list\n",
        )
        .expect("test models.yml should be written");

        let previous_home = env::var_os("HOME");
        env::set_var("HOME", &home);
        let result = save_custom_providers(CustomProviderSaveRequest {
            expected_version: omp_store_version().expect("test models.yml should be readable"),
            custom_providers: vec![
                CustomProviderSave {
                    id: "gateway".to_string(),
                    base_url: "https://new.example.com/v1".to_string(),
                    api: "openai-responses".to_string(),
                    api_key: None,
                    models: vec![
                        saved_model("keep", "openai-responses"),
                        saved_model("new", "anthropic-messages"),
                        saved_model("new-openai", "openai-completions"),
                    ],
                },
                CustomProviderSave {
                    id: "added".to_string(),
                    base_url: "https://added.example.com/v1".to_string(),
                    api: "anthropic-messages".to_string(),
                    api_key: Some("ADDED_KEY".to_string()),
                    models: vec![
                        saved_model("alpha", "anthropic-messages"),
                        saved_model("beta", "anthropic-messages"),
                    ],
                },
            ],
            removed_custom_providers: Vec::new(),
        });
        if let Some(previous_home) = previous_home {
            env::set_var("HOME", previous_home);
        } else {
            env::remove_var("HOME");
        }

        let store = result.expect("custom provider save should succeed");
        assert_eq!(store.version.len(), 16, "version must be fixed-width hex");
        assert!(store
            .version
            .chars()
            .all(|character| character.is_ascii_digit() || ('a'..='f').contains(&character)));
        let providers = store.custom_providers;
        assert_eq!(providers.len(), 2);
        let gateway = providers
            .iter()
            .find(|provider| provider.id == "gateway")
            .expect("gateway should be visible");
        assert_eq!(gateway.api, "anthropic-messages");
        assert!(gateway.api_key_configured);
        assert_eq!(
            gateway
                .models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            vec!["keep", "new", "new-openai"]
        );
        assert!(
            providers
                .iter()
                .all(|provider| provider.id != "discovery-only"),
            "override-only providers must not be owned by the custom provider editor"
        );
        assert_eq!(
            fs::read_to_string(agent.join("config.yml"))
                .expect("config.yml should remain readable"),
            config_content,
            "custom provider persistence must not touch config.yml"
        );

        let written: YamlValue = serde_yaml::from_str(
            &fs::read_to_string(agent.join("models.yml")).expect("models.yml should exist"),
        )
        .expect("saved models.yml should remain valid YAML");
        let providers = nested_value(&written, &["providers"])
            .and_then(YamlValue::as_mapping)
            .expect("providers mapping should exist");
        let gateway_config = providers
            .get(YamlValue::String("gateway".to_string()))
            .and_then(YamlValue::as_mapping)
            .expect("gateway config should exist");
        assert_eq!(
            mapping_string(gateway_config, "apiKey").as_deref(),
            Some("OLD_KEY")
        );
        assert_eq!(
            mapping_string(gateway_config, "baseUrl").as_deref(),
            Some("https://new.example.com/v1")
        );
        assert_eq!(
            mapping_string(
                gateway_config
                    .get(YamlValue::String("headers".to_string()))
                    .and_then(YamlValue::as_mapping)
                    .expect("gateway headers should survive"),
                "X-Team"
            )
            .as_deref(),
            Some("keep")
        );
        let models = gateway_config
            .get(YamlValue::String("models".to_string()))
            .and_then(YamlValue::as_sequence)
            .expect("gateway models should exist");
        assert_eq!(yaml_model_id(&models[0]).as_deref(), Some("keep"));
        assert_eq!(
            models[0]
                .as_mapping()
                .and_then(|model| mapping_string(model, "name"))
                .as_deref(),
            Some("Keep model")
        );
        assert_eq!(
            models[0]
                .as_mapping()
                .and_then(|model| mapping_string(model, "api"))
                .as_deref(),
            Some("openai-responses")
        );
        let keep_compat = models[0]
            .as_mapping()
            .and_then(|model| model.get(YamlValue::String("compat".to_string())))
            .and_then(YamlValue::as_mapping)
            .expect("existing OpenAI model compat mapping should survive");
        assert_eq!(
            keep_compat.get(YamlValue::String("supportsStore".to_string())),
            Some(&YamlValue::Bool(true))
        );
        assert_eq!(
            mapping_string(keep_compat, "promptCacheSessionHeader").as_deref(),
            Some("X-Session-Id")
        );
        assert!(
            models[1]
                .as_mapping()
                .and_then(|model| model.get(YamlValue::String("compat".to_string())))
                .is_none(),
            "non-OpenAI models should not receive the OpenCode session header"
        );
        assert_eq!(
            models[2]
                .as_mapping()
                .and_then(|model| model.get(YamlValue::String("compat".to_string())))
                .and_then(YamlValue::as_mapping)
                .and_then(|compat| mapping_string(compat, "promptCacheSessionHeader"))
                .as_deref(),
            Some("X-Session-Id")
        );
        assert_eq!(yaml_model_id(&models[1]).as_deref(), Some("new"));
        assert_eq!(
            models[1]
                .as_mapping()
                .and_then(|model| mapping_string(model, "api"))
                .as_deref(),
            Some("anthropic-messages")
        );
        assert!(
            gateway_config
                .get(YamlValue::String("api".to_string()))
                .is_none(),
            "managed providers must store API types on each model"
        );
        assert!(models
            .iter()
            .all(|model| yaml_model_id(model).as_deref() != Some("drop")));
        assert!(
            nested_value(&written, &["providers", "discovery-only"]).is_some(),
            "unmanaged override-only provider must survive"
        );

        fs::remove_dir_all(home).expect("isolated test home should be removed");
    }

    #[test]
    fn custom_provider_save_refuses_a_stale_editor_version() {
        let _home_guard = HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = temporary_home();
        let agent = home.join(".omp").join("agent");
        fs::create_dir_all(&agent).expect("test config directory should be created");
        let models_path = agent.join("models.yml");
        fs::write(
            &models_path,
            "providers:\n  gateway:\n    baseUrl: https://old.example.com/v1\n    api: openai-completions\n    apiKey: OLD_KEY\n    models:\n      - id: keep\n        api: openai-completions\n",
        )
        .expect("test models.yml should be written");

        let previous_home = env::var_os("HOME");
        env::set_var("HOME", &home);
        let stale_version = omp_store_version().expect("test models.yml should be readable");
        // Outside edit: an existing model's API changes and a provider is added.
        let external = "providers:\n  gateway:\n    baseUrl: https://old.example.com/v1\n    api: openai-completions\n    apiKey: OLD_KEY\n    models:\n      - id: keep\n        api: anthropic-messages\n  outside:\n    baseUrl: https://outside.example.com/v1\n    api: anthropic-messages\n    apiKey: OUTSIDE_KEY\n    models:\n      - id: added\n        api: anthropic-messages\n";
        fs::write(&models_path, external).expect("external edit should be written");
        let external_bytes = external.as_bytes().to_vec();
        assert_ne!(
            stale_version,
            omp_store_version().expect("externally edited models.yml should be readable"),
            "an outside edit must change the store version"
        );

        let stale_attempts = [
            // update, delete and add all come from the same frozen editor snapshot
            CustomProviderSaveRequest {
                expected_version: stale_version.clone(),
                custom_providers: vec![CustomProviderSave {
                    id: "gateway".to_string(),
                    base_url: "https://stale.example.com/v1".to_string(),
                    api: "openai-completions".to_string(),
                    api_key: None,
                    models: vec![saved_model("keep", "openai-completions")],
                }],
                removed_custom_providers: Vec::new(),
            },
            CustomProviderSaveRequest {
                expected_version: stale_version.clone(),
                custom_providers: Vec::new(),
                removed_custom_providers: vec!["gateway".to_string()],
            },
            CustomProviderSaveRequest {
                expected_version: stale_version.clone(),
                custom_providers: vec![CustomProviderSave {
                    id: "stale-add".to_string(),
                    base_url: "https://stale.example.com/v1".to_string(),
                    api: "openai-responses".to_string(),
                    api_key: Some("STALE_KEY".to_string()),
                    models: vec![saved_model("stale", "openai-responses")],
                }],
                removed_custom_providers: Vec::new(),
            },
        ];
        for request in stale_attempts {
            let error =
                save_custom_providers(request).expect_err("a stale editor version must be refused");
            assert_eq!(error, STALE_CUSTOM_PROVIDER_STORE);
            assert_eq!(
                fs::read(&models_path).expect("models.yml should remain readable"),
                external_bytes,
                "a refused save must leave the externally edited bytes untouched"
            );
        }

        // Re-reading the store (fresh version) unblocks the retry.
        let store = save_custom_providers(CustomProviderSaveRequest {
            expected_version: omp_store_version().expect("test models.yml should be readable"),
            custom_providers: vec![CustomProviderSave {
                id: "retry".to_string(),
                base_url: "https://retry.example.com/v1".to_string(),
                api: "openai-responses".to_string(),
                api_key: Some("RETRY_KEY".to_string()),
                models: vec![saved_model("retry-model", "openai-responses")],
            }],
            removed_custom_providers: Vec::new(),
        })
        .expect("a retry on the fresh version should save");
        assert_ne!(store.version, stale_version);
        assert_eq!(
            store.version,
            omp_store_version().expect("saved models.yml should be readable"),
            "the returned version must describe what a fresh load would read"
        );
        let written =
            fs::read_to_string(&models_path).expect("saved models.yml should remain readable");
        assert!(
            !written.contains("stale.example.com"),
            "a refused save must never reach disk"
        );
        let parsed: YamlValue =
            serde_yaml::from_str(&written).expect("saved models.yml should be valid YAML");
        assert_eq!(
            parsed["providers"]["gateway"]["models"][0]["api"].as_str(),
            Some("anthropic-messages"),
            "the externally changed model API must not be reverted"
        );
        assert_eq!(
            parsed["providers"]["outside"]["apiKey"].as_str(),
            Some("OUTSIDE_KEY"),
            "the externally added provider must be retained"
        );
        assert_eq!(
            parsed["providers"]["retry"]["baseUrl"].as_str(),
            Some("https://retry.example.com/v1")
        );
        assert!(store
            .custom_providers
            .iter()
            .any(|provider| provider.id == "outside"));

        if let Some(previous_home) = previous_home {
            env::set_var("HOME", previous_home);
        } else {
            env::remove_var("HOME");
        }
        fs::remove_dir_all(home).expect("isolated test home should be removed");
    }

    #[test]
    fn clearing_nested_values_does_not_leave_empty_parent_mappings() {
        let mut root: YamlValue =
            serde_yaml::from_str("otherSetting: preserved\nadvisor:\n  enabled: true\n")
                .expect("fixture should be valid YAML");

        set_nested_optional_bool(&mut root, &["advisor", "enabled"], None);
        set_nested_optional_bool(&mut root, &["prewalk", "enabled"], None);

        assert!(nested_value(&root, &["advisor"]).is_none());
        assert!(nested_value(&root, &["prewalk"]).is_none());
        assert_eq!(
            root.get("otherSetting").and_then(YamlValue::as_str),
            Some("preserved")
        );
    }

    #[test]
    fn custom_provider_discovery_parser_reads_all_models() {
        let models = parse_custom_provider_model_list(
            r#"{"data":[{"id":"model-a","name":"Model A","supported_endpoint_types":["openai"]},{"id":"model-b","supported_endpoint_types":["anthropic"]},{"id":"model-a"}]}"#,
        )
        .expect("provider model list should parse");

        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "model-a");
        assert_eq!(models[0].name, "Model A");
        assert_eq!(models[0].api.as_deref(), Some("openai-completions"));
        assert_eq!(models[1].id, "model-b");
        assert_eq!(models[1].name, "model-b");
        assert_eq!(models[1].api.as_deref(), Some("anthropic-messages"));
        assert_eq!(
            custom_provider_models_url("https://example.com/v1").expect("model URL should build"),
            "https://example.com/v1/models"
        );
        assert_eq!(
            custom_provider_models_url("https://example.com/v1/")
                .expect("trailing slash should normalize"),
            "https://example.com/v1/models"
        );
        assert_eq!(
            custom_provider_model_urls("https://example.com/api")
                .expect("version fallback URLs should build"),
            vec![
                "https://example.com/api/v1/models".to_string(),
                "https://example.com/api/v3/models".to_string()
            ]
        );
    }

    #[test]
    fn custom_provider_rich_catalog_overlays_authoritative_models() {
        let regular = parse_custom_provider_model_list(
            r#"{"data":[{"id":"gpt-5.6-sol","name":"gpt-5.6-sol"},{"id":"keep"}]}"#,
        )
        .expect("regular model list should parse");
        let rich = parse_custom_provider_model_list(
            r#"{"models":[{"slug":"gpt-5.6-sol","display_name":"GPT 5.6 Sol","default_reasoning_level":"low","supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"},{"effort":"max"},{"effort":"ultra"}]},{"slug":"hidden","visibility":"hide","supported_reasoning_levels":[{"effort":"high"}]},{"slug":"not-in-regular"}]}"#,
        )
        .expect("rich model catalog should parse");
        let merged = merge_custom_provider_model_capabilities(regular, rich);

        assert_eq!(merged.len(), 2, "ordinary /models remains authoritative");
        assert_eq!(merged[0].id, "gpt-5.6-sol");
        assert_eq!(merged[0].name, "GPT 5.6 Sol");
        assert_eq!(merged[0].thinking, vec!["low", "medium", "max"]);
        assert_eq!(merged[0].default_thinking_level.as_deref(), Some("low"));
        assert_eq!(
            custom_provider_rich_models_url("https://example.com/v1/models"),
            "https://example.com/v1/models?client_version=0.144.1"
        );

        let model = CustomProviderSaveModel {
            id: "gpt-5.6-sol".to_string(),
            api: "openai-responses".to_string(),
            thinking: vec!["low".to_string(), "medium".to_string(), "max".to_string()],
            default_thinking_level: Some("medium".to_string()),
            configuration: model_metadata::Configuration::new(),
        };
        let provider = CustomProviderSave {
            id: "gateway".to_string(),
            base_url: "https://example.com/v1".to_string(),
            api: "openai-responses".to_string(),
            api_key: None,
            models: vec![model],
        };

        let yaml = custom_provider_yaml(None, &provider);
        assert_eq!(yaml["models"][0]["reasoning"], YamlValue::Bool(true));
        assert_eq!(
            yaml["models"][0]["thinking"]["mode"],
            YamlValue::String("effort".to_string())
        );
        assert_eq!(
            yaml["models"][0]["thinking"]["efforts"],
            YamlValue::Sequence(vec![
                YamlValue::String("low".to_string()),
                YamlValue::String("medium".to_string()),
                YamlValue::String("max".to_string()),
            ])
        );
        assert_eq!(
            yaml["models"][0]["thinking"]["defaultLevel"],
            YamlValue::String("medium".to_string())
        );
    }

    #[test]
    fn custom_provider_discovery_handles_unavailable_and_failed_rich_catalogs() {
        use std::{io::{Read, Write}, net::TcpListener};

        let _home_guard = HOME_ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = temporary_home();
        let agent = home.join(".omp/agent");
        fs::create_dir_all(&agent).unwrap();
        fs::write(agent.join("edge-token"), "test-edge-token\n").unwrap();
        for (status, rich_body, optional) in [
            ("404 Not Found", "", true),
            ("405 Method Not Allowed", "", true),
            ("302 Found", "", false),
            ("503 Service Unavailable", "", false),
            ("200 OK", "invalid JSON", false),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let base_url = format!("http://{address}/v1");
            fs::write(agent.join("models.yml"), format!(
                "providers:\n  test-provider:\n    baseUrl: {base_url}\n    apiKey: test-key\n    headers:\n      X-CLI-Proxy-Access: '!/bin/cat ~/.omp/agent/edge-token'\n"
            )).unwrap();
            let server = std::thread::spawn(move || {
                for index in 0..2 {
                    let (mut stream, _) = listener.accept().unwrap();
                    let mut request_bytes = Vec::new();
                    loop {
                        let mut chunk = [0_u8; 1024];
                        let size = stream.read(&mut chunk).unwrap();
                        if size == 0 {
                            break;
                        }
                        request_bytes.extend_from_slice(&chunk[..size]);
                        if request_bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let request = String::from_utf8_lossy(&request_bytes).to_ascii_lowercase();
                    assert!(request.contains("accept: application/json"));
                    assert!(request.contains("authorization: bearer test-key"));
                    assert!(request.contains("x-api-key: test-key"));
                    assert!(request.contains("anthropic-version: 2023-06-01"));
                    assert!(request.contains("x-cli-proxy-access: test-edge-token\r\n"));
                    let (status, body) = if index == 0 {
                        assert!(request.contains("get /v1/models http/1.1"));
                        ("200 OK", r#"{"data":[{"id":"model-a"},{"id":"model-b"}]}"#)
                    } else {
                        assert!(request.contains("get /v1/models?client_version=0.144.1 http/1.1"));
                        (status, rich_body)
                    };
                    let redirect = if status == "302 Found" {
                        "Location: http://127.0.0.1:0/models\r\n"
                    } else {
                        ""
                    };
                    write!(stream,
                        "HTTP/1.1 {status}\r\n{redirect}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()
                    ).unwrap();
                }
            });
            let previous_home = env::var_os("HOME");
            env::set_var("HOME", &home);
            let discover = |base_url| tauri::async_runtime::block_on(discover_custom_provider_models(
                CustomProviderDiscoveryRequest {
                    provider_id: "test-provider".to_string(),
                    base_url,
                    api: "anthropic-messages".to_string(),
                    api_key: None,
                },
            ));
            let mismatched = discover(format!("{base_url}/other"));
            let result = discover(format!("{base_url}/"));
            if let Some(previous_home) = previous_home {
                env::set_var("HOME", previous_home);
            } else {
                env::remove_var("HOME");
            }
            server.join().unwrap();
            assert!(matches!(mismatched, Err(message) if message.contains("API key")));
            if optional {
                let models = result.expect("unsupported rich endpoint should retain the ordinary catalog");
                assert_eq!(models.iter().map(|model| model.id.as_str()).collect::<Vec<_>>(), ["model-a", "model-b"]);
            } else if status == "302 Found" {
                assert!(matches!(result, Err(message) if message.contains("HTTP 302")));
            } else {
                assert!(result.is_err(), "{status} must not silently discard model metadata");
            }
        }
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn custom_provider_discovery_merges_rich_catalog_response() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };

        let _home_guard = HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = temporary_home();
        fs::create_dir_all(home.join(".omp").join("agent"))
            .expect("test agent directory should be created");
        let listener = TcpListener::bind("127.0.0.1:0").expect("test listener should bind");
        let address = listener
            .local_addr()
            .expect("test listener address should exist");
        let server = std::thread::spawn(move || {
            for request_index in 0..2 {
                let (mut stream, _) = listener.accept().expect("discovery request should connect");
                let mut request_bytes = Vec::new();
                loop {
                    let mut chunk = [0_u8; 1024];
                    let size = stream.read(&mut chunk).expect("request should be readable");
                    if size == 0 {
                        break;
                    }
                    request_bytes.extend_from_slice(&chunk[..size]);
                    if request_bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&request_bytes).to_ascii_lowercase();
                assert!(request.contains("authorization: bearer test-key"));
                let body = if request_index == 0 {
                    assert!(request.contains("get /v1/models http/1.1"));
                    let mut body: JsonValue = serde_json::from_str(r#"{"data":[{"id":"model-a","name":"model-a","context_length":240000,"max_output_tokens":32000,"input":["text"],"reasoning":false,"supportsTools":false,"compat":{"supportsStore":false}},{"id":"model-b"}]}"#).unwrap();
                    for id in ["uni/claude-fable-5.2", "uni/claude-haiku-5.5", "uni/claude-opus-5.5", "uni/claude-sonnet-5.5"] {
                        body["data"].as_array_mut().unwrap().push(serde_json::json!({"id": id}));
                    }
                    body.to_string()
                } else {
                    assert!(request.contains("get /v1/models?client_version=0.144.1 http/1.1"));
                    // The real rich catalog exceeded the ordinary 15-second discovery deadline.
                    std::thread::sleep(Duration::from_secs(16));
                    let mut body: JsonValue = serde_json::from_str(r#"{"models":[{"slug":"model-a","display_name":"Model A","contextWindow":999999,"maxContextWindow":1000000,"cost":{"input":0,"output":2,"cacheRead":0,"cacheWrite":0},"default_reasoning_level":"high","supported_reasoning_levels":[{"effort":"low"},{"effort":"high"},{"effort":"ultra"}]}]}"#).unwrap();
                    for id in ["uni/claude-fable-5.2", "uni/claude-haiku-5.5", "uni/claude-opus-5.5", "uni/claude-sonnet-5.5"] {
                        body["models"].as_array_mut().unwrap().push(serde_json::json!({
                            "slug": id, "context_window": 500000, "max_context_window": 872000,
                            "input_modalities": ["text", "image"], "default_reasoning_level": "medium",
                            "supported_reasoning_levels": [{"effort": "low"}, {"effort": "medium"}, {"effort": "high"}]
                        }));
                    }
                    body.to_string()
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .expect("test response should be written");
            }
        });

        let previous_home = env::var_os("HOME");
        env::set_var("HOME", &home);
        let result = tauri::async_runtime::block_on(discover_custom_provider_models(
            CustomProviderDiscoveryRequest {
                provider_id: "test-provider".to_string(),
                base_url: format!("http://{address}/v1"),
                api: "openai-responses".to_string(),
                api_key: Some("test-key".to_string()),
            },
        ));
        let result = result.and_then(|models| {
            let request: CustomProviderSaveRequest = serde_json::from_value(serde_json::json!({
                "expectedVersion": omp_store_version()?,
                "customProviders": [{
                    "id": "test-provider", "baseUrl": format!("http://{address}/v1"),
                    "api": "openai-responses", "apiKey": "test-key",
                    "models": models.iter().map(|model| serde_json::json!({
                        "id": model.id, "api": "openai-responses", "thinking": model.thinking,
                        "defaultThinkingLevel": model.default_thinking_level,
                        "configuration": model.configuration
                    })).collect::<Vec<_>>()
                }]
            })).map_err(|error| error.to_string())?;
            save_custom_providers(request)?;
            let (written, _) = load_yaml(&home.join(".omp/agent/models.yml"))?;
            Ok((models, written))
        });
        if let Some(previous_home) = previous_home {
            env::set_var("HOME", previous_home);
        } else {
            env::remove_var("HOME");
        }
        server.join().expect("discovery server should finish");

        let (models, written) = result.expect("discovery and save should succeed");
        assert_eq!(models.len(), 6);
        assert_eq!(models[0].id, "model-a");
        assert_eq!(models[0].name, "Model A");
        assert_eq!(models[0].thinking, vec!["low", "high"]);
        assert_eq!(models[0].default_thinking_level.as_deref(), Some("high"));
        let saved = &written["providers"]["test-provider"]["models"][0];
        assert_eq!(saved["contextWindow"].as_u64(), Some(240000));
        assert_eq!(saved["maxTokens"].as_u64(), Some(32000));
        assert_eq!(saved["maxContextWindow"].as_u64(), Some(1000000));
        assert_eq!(saved["reasoning"].as_bool(), Some(false));
        assert_eq!(saved["supportsTools"].as_bool(), Some(false));
        assert_eq!(saved["input"], serde_yaml::to_value(["text"]).unwrap());
        assert_eq!(saved["cost"]["input"].as_u64(), Some(0));
        let reloaded = load_custom_providers(&written);
        assert_eq!(reloaded[0].models[0].configuration["contextWindow"], 240000);
        assert!(!reloaded[0].models[0].configuration.contains_key("compat"));
        assert!(models[1].thinking.is_empty());
        for saved in written["providers"]["test-provider"]["models"].as_sequence().unwrap().iter().skip(2) {
            assert_eq!(saved["api"].as_str(), Some("openai-responses"));
            assert_eq!(saved["contextWindow"].as_u64(), Some(500000));
            assert_eq!(saved["maxContextWindow"].as_u64(), Some(872000));
            assert_eq!(saved["input"], serde_yaml::to_value(["text", "image"]).unwrap());
            assert_eq!(saved["reasoning"].as_bool(), Some(true));
            assert_eq!(saved["thinking"]["mode"].as_str(), Some("effort"));
            assert_eq!(saved["thinking"]["efforts"], serde_yaml::to_value(["low", "medium", "high"]).unwrap());
            assert_eq!(saved["thinking"]["defaultLevel"].as_str(), Some("medium"));
        }
        fs::remove_dir_all(home).expect("isolated test home should be removed");
    }

    #[test]
    fn custom_provider_anthropic_catalog_survives_save_round_trip() {
        let ids = [
            "uni/claude-fable-5.2", "uni/claude-haiku-5.5",
            "uni/claude-opus-5.5", "uni/claude-sonnet-5.5",
        ];
        let ordinary = serde_json::json!({
            "data": ids.iter().map(|id| serde_json::json!({"id": id})).collect::<Vec<_>>()
        });
        let rich = serde_json::json!({
            "models": ids.iter().map(|id| serde_json::json!({
                "slug": id, "display_name": id, "context_window": 500000,
                "input_modalities": ["text", "image"], "default_reasoning_level": "medium",
                "supported_reasoning_levels": [{"effort": "low"}, {"effort": "medium"}, {"effort": "high"}]
            })).collect::<Vec<_>>()
        });
        let models = merge_custom_provider_model_capabilities(
            parse_custom_provider_model_list(&ordinary.to_string()).unwrap(),
            parse_custom_provider_model_list(&rich.to_string()).unwrap(),
        );
        let _home_guard = HOME_ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = temporary_home();
        let agent = home.join(".omp/agent");
        fs::create_dir_all(&agent).unwrap();
        fs::write(agent.join("models.yml"),
            "providers:\n  gateway:\n    baseUrl: https://example.com/v1\n    apiKey: test-key\n    models:\n      - id: uni/claude-fable-5.2\n        api: anthropic-messages\n        reasoning: false\n        maxTokens: 64000\n        headers:\n          X-Team: keep\n"
        ).unwrap();
        let previous_home = env::var_os("HOME");
        env::set_var("HOME", &home);
        let request: CustomProviderSaveRequest = serde_json::from_value(serde_json::json!({
            "expectedVersion": omp_store_version().expect("test models.yml should be readable"),
            "customProviders": [{
                "id": "gateway", "baseUrl": "https://example.com/v1",
                "api": "anthropic-messages", "apiKey": null,
                "models": models.iter().map(|model| serde_json::json!({
                    "id": model.id, "api": "anthropic-messages", "thinking": model.thinking,
                    "defaultThinkingLevel": model.default_thinking_level,
                    "configuration": model.configuration
                })).collect::<Vec<_>>()
            }]
        })).unwrap();
        let result = save_custom_providers(request);
        if let Some(previous_home) = previous_home {
            env::set_var("HOME", previous_home);
        } else {
            env::remove_var("HOME");
        }
        let providers = result
            .expect("Anthropic catalog should save")
            .custom_providers;
        let (written, _) = load_models_yaml(&agent.join("models.yml")).unwrap();
        let saved = written["providers"]["gateway"]["models"].as_sequence().unwrap();
        for (index, id) in ids.iter().enumerate() {
            let model = &saved[index];
            assert_eq!(model["id"].as_str(), Some(*id));
            assert_eq!(model["api"].as_str(), Some("anthropic-messages"));
            assert_eq!(model["contextWindow"].as_u64(), Some(500000));
            assert_eq!(model["input"], serde_yaml::to_value(["text", "image"]).unwrap());
            assert_eq!(model["reasoning"].as_bool(), Some(index != 0));
            assert_eq!(model["thinking"]["mode"].as_str(), Some("anthropic-adaptive"));
            assert_eq!(model["thinking"]["efforts"], serde_yaml::to_value(["low", "medium", "high"]).unwrap());
            assert_eq!(model["thinking"]["defaultLevel"].as_str(), Some("medium"));
            assert_eq!(providers[0].models[index].thinking, ["low", "medium", "high"]);
            assert_eq!(providers[0].models[index].configuration["contextWindow"], 500000);
            assert!(model["cost"].is_null(), "unadvertised prices must not be invented");
        }
        assert_eq!(saved[0]["maxTokens"].as_u64(), Some(64000));
        assert_eq!(saved[0]["headers"]["X-Team"].as_str(), Some("keep"));
        assert!(saved[1]["maxTokens"].is_null());

        let mut budget: Mapping = serde_yaml::from_str("reasoning: false\nthinking:\n  mode: budget\n  efforts: [high]\n").unwrap();
        let mut model = saved_model("uni/claude-sonnet-5.5", "anthropic-messages");
        model.thinking = vec!["low".to_string(), "high".to_string()];
        apply_omp_model_capabilities(&mut budget, &model);
        let budget = YamlValue::Mapping(budget);
        assert_eq!(budget["thinking"]["mode"].as_str(), Some("budget"));
        assert_eq!(budget["thinking"]["efforts"], serde_yaml::to_value(["high"]).unwrap());
        assert_eq!(budget["reasoning"].as_bool(), Some(false));
        for (api, id, expected) in [
            ("anthropic-messages", "claude-opus-5.5", Some("anthropic-adaptive")),
            ("anthropic-messages", "uni/claude-custom", None),
            ("anthropic-messages", "uni/claude-custom-5", None),
            ("anthropic-messages", "uni/claude-sonnet-4-6", Some("anthropic-adaptive")),
            ("anthropic-messages", "uni/claude-opus-4.6", Some("anthropic-adaptive")),
            ("anthropic-messages", "uni/claude-sonnet-4-5", None),
            ("anthropic-messages", "uni/claude-haiku-4.5", None),
            ("anthropic-messages", "uni/claude-opus-4-5", None),
            ("anthropic-messages", "uni/claude-sonnet-4-20250514", None),
            ("anthropic-messages", "uni/claude-3-7-sonnet-20250219", None),
            ("anthropic-messages", "uni/not-claude-opus-5.5", None),
            ("anthropic-messages", "uni/opus-5.5", None),
            ("openai-responses", "uni/claude-opus-5.5", Some("effort")),
            ("google-generative-ai", "uni/claude-opus-5.5", None),
        ] {
            let mut config = Mapping::new();
            let mut model = saved_model(id, api);
            model.thinking = vec!["low".to_string(), "high".to_string()];
            apply_omp_model_capabilities(&mut config, &model);
            assert_eq!(YamlValue::Mapping(config)["thinking"]["mode"].as_str(), expected, "{api}/{id}");
        }
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn model_catalog_parser_keeps_limits_and_pricing() {
        let models = parse_models_json(
            r#"{"models":[{"provider":"openai-codex","id":"gpt-6-astra","selector":"openai-codex/gpt-6-astra","name":"GPT-6-Astra","reasoning":true,"thinking":["low","medium","high","xhigh","max"],"input":["text","image"],"contextWindow":272000,"maxTokens":128000,"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0}}]}"#,
            false,
        )
        .expect("model catalog should parse");

        assert_eq!(models.len(), 1);
        let model = &models[0];
        assert_eq!(model.selector, "openai-codex/gpt-6-astra");
        assert_eq!(model.context_window, Some(272_000));
        assert_eq!(model.max_tokens, Some(128_000));
        let cost = model.cost.as_ref().expect("pricing should be retained");
        assert_eq!(cost.input, 0.0);
        assert_eq!(cost.output, 0.0);
        assert_eq!(cost.cache_read, 0.0);
        assert_eq!(cost.cache_write, 0.0);
    }

    #[test]
    fn yaml_round_trip_preserves_omp_string_enums() {
        // OMP parses config with Bun's YAML: bare on/off are strings, but bare
        // digits are numbers, so digit-like enum values must stay quoted on write.
        let mut root = YamlValue::Mapping(Mapping::new());
        set_nested_optional_string(&mut root, &["advisor", "syncBacklog"], Some("1"));
        set_nested_string(&mut root, &["completion", "notify"], "on");

        let written = serde_yaml::to_string(&root).expect("config should serialize");
        let digit_line = written
            .lines()
            .find(|line| line.contains("syncBacklog"))
            .expect("syncBacklog should be written");
        assert!(
            digit_line.contains("'1'") || digit_line.contains("\"1\""),
            "digit enum strings must stay quoted: {written}"
        );

        let reparsed: YamlValue =
            serde_yaml::from_str(&written).expect("written config should reparse");
        assert_eq!(
            optional_string(&reparsed, &["advisor", "syncBacklog"]).as_deref(),
            Some("1")
        );
        assert_eq!(
            optional_string(&reparsed, &["completion", "notify"]).as_deref(),
            Some("on"),
            "bare on must survive serde_yaml parse as a string for Bun/OMP"
        );
    }

    #[test]
    fn build_subprocess_path_puts_custom_parent_first_and_includes_system_paths() {
        let custom = Path::new("/custom/bin");
        let result = build_subprocess_path(Some(custom));
        let result_str = result.to_string_lossy();
        assert!(result_str.starts_with("/custom/bin:"));
        #[cfg(target_os = "macos")]
        assert!(result_str.contains("/opt/homebrew/bin"));
    }

    #[test]
    fn custom_omp_path_takes_precedence_in_candidates() {
        let candidates = executable_candidates(Some("/opt/custom/omp"));
        assert_eq!(candidates.first(), Some(&PathBuf::from("/opt/custom/omp")));
    }

    #[test]
    fn accepts_kind_role_matches_omp_role_definitions() {
        assert_eq!(accepts_kind_role("image", "image"), Some(true));
        assert_eq!(accepts_kind_role("image", "chat"), Some(false));
        assert_eq!(accepts_kind_role("speech", "tts"), Some(true));
        assert_eq!(accepts_kind_role("speech", "chat"), Some(false));
        assert_eq!(accepts_kind_role("dictation", "stt"), Some(true));
        assert_eq!(accepts_kind_role("judge", "judge"), Some(true));
        assert_eq!(accepts_kind_role("judge", "tiny"), Some(true));
        assert_eq!(accepts_kind_role("judge", "chat"), Some(true));
        assert_eq!(accepts_kind_role("judge", "image"), Some(false));
        assert_eq!(accepts_kind_role("web", "search"), Some(true));
        assert_eq!(accepts_kind_role("web", "image"), Some(false));
        // An empty kind means the `--kind all` metadata pass failed, so the role
        // must not be judged. Skipping beats rejecting a config that is valid.
        assert_eq!(accepts_kind_role("image", ""), None);
        // `omp models --json` emits no `webSearch` field, so a chat model's
        // grounding is unknowable here and must never be reported as a mismatch.
        assert_eq!(accepts_kind_role("web", "chat"), None);
        assert_eq!(accepts_kind_role("default", "chat"), None);
    }

    #[test]
    fn selector_base_strips_alias_and_thinking_suffix() {
        assert_eq!(selector_base("vps2/gpt-6-sol:high"), "vps2/gpt-6-sol");
        // An alias resolves through modelRoles, not the kind index, so stripping
        // it leaves a selector that simply won't be found and validation skips.
        assert_eq!(selector_base("  @slow "), "slow");
        assert_eq!(selector_base("vps2-image/gpt-image-2.5"), "vps2-image/gpt-image-2.5");
    }

    #[test]
    fn kind_role_validation_rejects_chat_model_for_image_role() {
        let mut index = HashMap::new();
        index.insert("vps2/gpt-image-2.5".to_string(), "chat".to_string());
        index.insert("vps2-image/gpt-image-2.5".to_string(), "image".to_string());
        let error = validate_kind_role_selector("image", "vps2/gpt-image-2.5", &index)
            .expect_err("a chat model cannot satisfy the image role");
        assert!(error.contains("image"), "{error}");
        assert!(error.contains("chat"), "{error}");
        validate_kind_role_selector("image", "vps2-image/gpt-image-2.5", &index)
            .expect("a kind=image model satisfies the image role");
        // Unknown models are left to OMP's own "unknown model" warning.
        validate_kind_role_selector("image", "vps2/does-not-exist", &index)
            .expect("unknown model must not be blocked here");
        // An empty index means metadata was unavailable; nothing is rejected.
        validate_kind_role_selector("image", "vps2/gpt-image-2.5", &HashMap::new())
            .expect("no metadata must not block a save");
    }

    #[test]
    fn kind_roles_are_system_roles_not_custom_names() {
        for role in ["default", "smol", "image", "web", "speech", "dictation", "judge"] {
            assert!(is_system_role(role), "{role} should be a system role");
            assert!(
                !valid_custom_role(role),
                "{role} must not be classified as a user-invented custom role"
            );
        }
        assert!(is_system_role("designer"));
        assert!(!is_system_role("tester"));
        assert!(valid_custom_role("tester"));
    }

    #[test]
    fn image_role_warns_only_when_its_tool_is_gated_off() {
        // Configured role + tool off (the default) => dead config, warn.
        assert!(image_role_blocked_by_tool_gate(Some("vps2-image/gpt-image-2.5"), None));
        assert!(image_role_blocked_by_tool_gate(
            Some("vps2-image/gpt-image-2.5"),
            Some(false)
        ));
        // Tool explicitly on => working config, stay quiet.
        assert!(!image_role_blocked_by_tool_gate(
            Some("vps2-image/gpt-image-2.5"),
            Some(true)
        ));
        // No role configured => nothing to warn about either way.
        assert!(!image_role_blocked_by_tool_gate(None, None));
        assert!(!image_role_blocked_by_tool_gate(Some(""), None));
        assert!(!image_role_blocked_by_tool_gate(Some("   "), Some(false)));
    }

    #[test]
    fn snapshot_enriches_kind_and_flags_image_role_with_tool_disabled() {
        let _home_guard = HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = temporary_home();
        let agent = home.join(".omp").join("agent");
        fs::create_dir_all(&agent).expect("agent directory should be created");
        fs::write(
            agent.join("config.yml"),
            "modelRoles:\n  image: openai/chat-model\n",
        )
        .expect("test config should be written");

        // The mock answers the chat list and the `--kind all` metadata pass
        // separately, exactly as omp does.
        let bin_dir = home.join(".bun").join("bin");
        fs::create_dir_all(&bin_dir).expect("mock bin directory should be created");
        let mock_omp = bin_dir.join("omp");
        fs::write(
            &mock_omp,
            // The chat list carries no `kind` and no image rows; only the
            // `--kind all` pass does. If enrichment regressed, chat.kind would
            // stay empty and this test fails.
            r#"#!/bin/sh
for arg in "$@"; do
  if [ "$arg" = "all" ]; then
    echo '{"models":[{"provider":"openai","id":"chat-model","kind":"chat"},{"provider":"openai","id":"img","kind":"image"},{"provider":"web","id":"exa","kind":"search"},{"provider":"openai","id":"embedding","kind":"embedding"}]}'
    exit 0
  fi
done
echo '{"models":[{"provider":"openai","id":"chat-model"}]}'
"#,
        )
        .expect("mock omp script should be written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&mock_omp).expect("metadata").permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&mock_omp, permissions).expect("chmod");
        }

        let previous_home = env::var_os("HOME");
        env::set_var("HOME", &home);
        let snapshot = load_snapshot_from_disk(Some(&mock_omp.to_string_lossy()));
        if let Some(previous) = previous_home {
            env::set_var("HOME", previous);
        }

        let snapshot = snapshot.expect("snapshot should load");
        let chat = snapshot
            .models
            .iter()
            .find(|model| model.selector == "openai/chat-model")
            .expect("chat model should be present");
        assert_eq!(chat.kind, "chat", "kind must be enriched from --kind all");
        assert!(
            snapshot
                .warnings
                .iter()
                .any(|warning| warning.contains("generate_image.enabled")),
            "expected a tool-gate warning, got {:?}",
            snapshot.warnings
        );
        // Kind-role validation must retain search models absent from the chat
        // catalog, without admitting kinds that no managed role accepts.
        let candidates: Vec<&str> = snapshot
            .kind_models
            .iter()
            .map(|model| model.selector.as_str())
            .collect();
        assert!(candidates.contains(&"openai/chat-model"));
        assert!(candidates.contains(&"openai/img"));
        assert!(candidates.contains(&"web/exa"), "web fallback must be selectable");
        assert!(!candidates.contains(&"openai/embedding"));
    }

    #[test]
    fn fallback_candidates_prefer_local_bin_over_bun_bin() {
        let _home_guard = HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous_path = env::var_os("PATH");
        env::set_var("PATH", "");
        let omp_candidates = executable_candidates(None);
        match previous_path {
            Some(path) => env::set_var("PATH", path),
            None => env::remove_var("PATH"),
        }

        let home = home_dir().expect("home directory should be available");
        let local_omp = home.join(".local").join("bin").join("omp");
        let bun_omp = home.join(".bun").join("bin").join("omp");
        assert!(
            omp_candidates
                .iter()
                .position(|candidate| candidate == &local_omp)
                .expect("local omp fallback should be present")
                < omp_candidates
                    .iter()
                    .position(|candidate| candidate == &bun_omp)
                    .expect("bun omp fallback should be present")
        );
    }

    /// Install a mock `omp` that reports one chat row and one kind=image row.
    fn install_kind_role_mock(home: &Path) -> PathBuf {
        let agent = home.join(".omp").join("agent");
        fs::create_dir_all(&agent).expect("agent directory should be created");
        fs::write(
            agent.join("config.yml"),
            "modelRoles:\n  default: openai/chat-model\n",
        )
        .expect("test config should be written");
        let bin_dir = home.join(".bun").join("bin");
        fs::create_dir_all(&bin_dir).expect("mock bin directory should be created");
        let mock_omp = bin_dir.join("omp");
        fs::write(
            &mock_omp,
            r#"#!/bin/sh
for arg in "$@"; do
  if [ "$arg" = "all" ]; then
    echo '{"models":[{"provider":"openai","id":"chat-model","kind":"chat"},{"provider":"openai","id":"img","kind":"image"}]}'
    exit 0
  fi
done
echo '{"models":[{"provider":"openai","id":"chat-model"}]}'
"#,
        )
        .expect("mock omp script should be written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&mock_omp).expect("metadata").permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&mock_omp, permissions).expect("chmod");
        }
        mock_omp
    }

    fn empty_routing_settings() -> RoutingSettings {
        RoutingSettings {
            default_thinking_level: None,
            auto_thinking_max_effort: None,
            advisor_enabled: None,
            advisor_sync_backlog: None,
            advisor_immune_turns: None,
            prewalk_enabled: None,
            retry_model_fallback: None,
            retry_fallback_chains: Vec::new(),
            retry_fallback_revert_policy: None,
        }
    }

    #[test]
    fn save_refuses_an_image_role_pointed_at_a_chat_model() {
        let _home_guard = HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = temporary_home();
        let mock_omp = install_kind_role_mock(&home);
        let previous_home = env::var_os("HOME");
        env::set_var("HOME", &home);

        let result = save_snapshot(SaveRequest {
            model_roles: BTreeMap::from([("image".to_string(), "openai/chat-model".to_string())]),
            removed_model_roles: Vec::new(),
            agent_model_overrides: BTreeMap::new(),
            agent_files: Vec::new(),
            routing_settings: empty_routing_settings(),
            omp_path: Some(mock_omp.to_string_lossy().into_owned()),
        });

        if let Some(previous) = previous_home {
            env::set_var("HOME", previous);
        }
        fs::remove_dir_all(home).ok();

        let error = result.expect_err("a chat model cannot be saved as the image role");
        assert!(error.contains("image"), "{error}");
        assert!(
            error.contains("oh-my-pi#13272"),
            "the error should point at the upstream limitation: {error}"
        );
    }

    #[test]
    fn save_accepts_an_image_role_pointed_at_an_image_model() {
        let _home_guard = HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = temporary_home();
        let mock_omp = install_kind_role_mock(&home);
        let previous_home = env::var_os("HOME");
        env::set_var("HOME", &home);

        let result = save_snapshot(SaveRequest {
            model_roles: BTreeMap::from([("image".to_string(), "openai/img".to_string())]),
            removed_model_roles: Vec::new(),
            agent_model_overrides: BTreeMap::new(),
            agent_files: Vec::new(),
            routing_settings: empty_routing_settings(),
            omp_path: Some(mock_omp.to_string_lossy().into_owned()),
        });

        let saved = result.expect("a kind=image model must be accepted");
        assert_eq!(saved.model_roles.get("image").map(String::as_str), Some("openai/img"));

        if let Some(previous) = previous_home {
            env::set_var("HOME", previous);
        }
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn save_does_not_inject_empty_keys_for_unconfigured_kind_roles() {
        let _home_guard = HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = temporary_home();
        let mock_omp = install_kind_role_mock(&home);
        let previous_home = env::var_os("HOME");
        env::set_var("HOME", &home);

        // The frontend always submits every system role; the untouched ones
        // arrive as "". Those must not appear in a hand-curated config.yml.
        let result = save_snapshot(SaveRequest {
            model_roles: BTreeMap::from([
                ("image".to_string(), "openai/img".to_string()),
                ("speech".to_string(), String::new()),
                ("dictation".to_string(), String::new()),
                ("judge".to_string(), String::new()),
            ]),
            removed_model_roles: Vec::new(),
            agent_model_overrides: BTreeMap::new(),
            agent_files: Vec::new(),
            routing_settings: empty_routing_settings(),
            omp_path: Some(mock_omp.to_string_lossy().into_owned()),
        });
        let saved = result.expect("save should succeed");
        let parsed: YamlValue = serde_yaml::from_str(
            &fs::read_to_string(home.join(".omp").join("agent").join("config.yml"))
                .expect("config should be readable"),
        )
        .expect("config should parse");
        let roles = nested_string_map(&parsed, &["modelRoles"]);

        assert_eq!(roles.get("image").map(String::as_str), Some("openai/img"));
        for role in ["speech", "dictation", "judge"] {
            assert!(
                !roles.contains_key(role),
                "unset {role} must not be written into config.yml, got {roles:?}"
            );
        }
        assert_eq!(saved.model_roles.get("image").map(String::as_str), Some("openai/img"));

        if let Some(previous) = previous_home {
            env::set_var("HOME", previous);
        }
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn save_still_clears_an_existing_role_left_empty() {
        let _home_guard = HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = temporary_home();
        let mock_omp = install_kind_role_mock(&home);
        let previous_home = env::var_os("HOME");
        env::set_var("HOME", &home);

        // `default` exists in the fixture config, so emptying it must clear it
        // rather than be skipped as "never configured".
        save_snapshot(SaveRequest {
            model_roles: BTreeMap::from([("default".to_string(), String::new())]),
            removed_model_roles: Vec::new(),
            agent_model_overrides: BTreeMap::new(),
            agent_files: Vec::new(),
            routing_settings: empty_routing_settings(),
            omp_path: Some(mock_omp.to_string_lossy().into_owned()),
        })
        .expect("save should succeed");

        let content =
            fs::read_to_string(home.join(".omp").join("agent").join("config.yml")).expect("readable");
        let parsed: YamlValue = serde_yaml::from_str(&content).expect("config should parse");
        assert!(
            !model_role_is_set(&parsed, "default"),
            "an existing role cleared to empty must be written through, got {content}"
        );

        if let Some(previous) = previous_home {
            env::set_var("HOME", previous);
        }
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn kind_roles_cannot_be_removed_as_custom_roles() {
        let _home_guard = HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = temporary_home();
        let mock_omp = install_kind_role_mock(&home);
        let previous_home = env::var_os("HOME");
        env::set_var("HOME", &home);

        let result = save_snapshot(SaveRequest {
            model_roles: BTreeMap::new(),
            removed_model_roles: vec!["image".to_string()],
            agent_model_overrides: BTreeMap::new(),
            agent_files: Vec::new(),
            routing_settings: empty_routing_settings(),
            omp_path: Some(mock_omp.to_string_lossy().into_owned()),
        });

        if let Some(previous) = previous_home {
            env::set_var("HOME", previous);
        }
        fs::remove_dir_all(home).ok();

        let error = result.expect_err("a built-in kind role is not a deletable custom role");
        assert!(error.contains("image"), "{error}");
    }
}
