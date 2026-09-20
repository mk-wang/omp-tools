use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use serde_yaml::{Mapping, Value as YamlValue};
use std::{
    collections::{BTreeMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
    process::Command,
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
const CUSTOM_PROVIDER_APIS: [&str; 9] = [
    "openai-completions",
    "openai-responses",
    "openai-codex-responses",
    "azure-openai-responses",
    "anthropic-messages",
    "bedrock-converse-stream",
    "google-generative-ai",
    "google-gemini-cli",
    "google-vertex",
];
const PI_CUSTOM_PROVIDER_APIS: [&str; 4] = [
    "openai-completions",
    "openai-responses",
    "anthropic-messages",
    "google-generative-ai",
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
    #[serde(default)]
    custom_providers: Vec<CustomProviderSave>,
    #[serde(default)]
    removed_custom_providers: Vec<String>,
    /// `omp` is the legacy/default store; `pi` targets ~/.pi/agent/models.json.
    #[serde(default)]
    target: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CustomProviderDiscoveryRequest {
    provider_id: String,
    base_url: String,
    api: String,
    #[serde(default)]
    api_key: Option<String>,
    /// `omp` is the legacy/default store; `pi` reads existing keys from models.json.
    #[serde(default)]
    target: Option<String>,
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
    model_source: String,
    agent_files: Vec<AgentDefinitionFile>,
    routing_settings: RoutingSettings,
    custom_providers: Vec<CustomProvider>,
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

fn valid_custom_role(role: &str) -> bool {
    let mut characters = role.chars();
    matches!(characters.next(), Some(first) if first.is_ascii_lowercase())
        && characters.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '-' | '_')
        })
        && !SYSTEM_MODEL_ROLES.contains(&role)
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

fn json_model_api(value: &JsonValue) -> Option<String> {
    value
        .as_object()
        .and_then(|object| object.get("api"))
        .and_then(JsonValue::as_str)
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
        .get("supported_reasoning_levels")
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
        .and_then(|thinking| thinking.get("levels"))
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
    value
        .as_object()
        .and_then(|object| object.get("default_reasoning_level"))
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

fn custom_provider_models_json(value: Option<&JsonValue>) -> Vec<CustomProviderModel> {
    let Some(JsonValue::Array(values)) = value else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    let mut models = values
        .iter()
        .filter_map(|value| {
            let id = json_model_id(value)?;
            if !seen.insert(id.clone()) {
                return None;
            }
            let name = value
                .as_object()
                .and_then(|object| {
                    ["name", "display_name", "displayName"]
                        .iter()
                        .find_map(|key| object.get(*key).and_then(JsonValue::as_str))
                })
                .filter(|name| !name.trim().is_empty())
                .unwrap_or(&id)
                .to_string();
            Some(CustomProviderModel {
                id,
                name,
                api: json_model_api(value),
                thinking: json_model_thinking_levels(value),
                default_thinking_level: json_model_default_thinking_level(value),
            })
        })
        .collect::<Vec<_>>();
    models.sort_by(|left, right| left.id.cmp(&right.id));
    models
}

fn load_pi_custom_providers(root: &serde_json::Map<String, JsonValue>) -> Vec<CustomProvider> {
    let Some(providers) = root.get("providers").and_then(JsonValue::as_object) else {
        return Vec::new();
    };

    let mut result = providers
        .iter()
        .filter_map(|(id, value)| {
            let id = id.trim();
            let config = value.as_object()?;
            let models = custom_provider_models_json(config.get("models"));
            if models.is_empty() {
                return None;
            }
            let provider_api = config
                .get("api")
                .and_then(JsonValue::as_str)
                .map(str::to_string);
            Some(CustomProvider {
                id: id.to_string(),
                base_url: config
                    .get("baseUrl")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default()
                    .to_string(),
                api: inferred_custom_provider_api(provider_api, &models),
                api_key_configured: config
                    .get("apiKey")
                    .and_then(JsonValue::as_str)
                    .is_some_and(|key| !key.trim().is_empty()),
                // Pi's models.json has no OMP `auth: none` field. Authentication may
                // still come from auth.json or the environment, so this remains false.
                auth_none: false,
                models,
            })
        })
        .collect::<Vec<_>>();
    result.sort_by(|left, right| left.id.cmp(&right.id));
    result
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
    supported_apis: &[&str],
    product_label: &str,
) -> Result<(), String> {
    if models.is_empty() {
        return Err(format!(
            "{product_label} custom provider “{provider_id}”至少需要一个 model"
        ));
    }

    let mut seen_models = HashSet::new();
    for model in models {
        let model_id = model.id.trim();
        if model_id.is_empty() {
            return Err(format!(
                "{product_label} custom provider “{provider_id}”包含空 model"
            ));
        }
        if !seen_models.insert(model_id.to_string()) {
            return Err(format!(
                "{product_label} custom provider “{provider_id}”的 model “{model_id}”重复"
            ));
        }
        let api = model.api.trim();
        if !supported_apis.contains(&api) {
            return Err(format!(
                "{product_label} custom provider “{provider_id}”的 model “{model_id}” Type “{api}”不受支持"
            ));
        }
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
        validate_custom_provider_models(id, &provider.models, &CUSTOM_PROVIDER_APIS, "OMP")?;

        let existing = existing_custom_provider(root, id).and_then(YamlValue::as_mapping);
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

fn existing_pi_custom_provider<'a>(
    root: &'a serde_json::Map<String, JsonValue>,
    id: &str,
) -> Option<&'a JsonValue> {
    root.get("providers")
        .and_then(JsonValue::as_object)
        .and_then(|providers| providers.get(id))
}

fn validate_pi_custom_providers(providers: &[CustomProviderSave]) -> Result<(), String> {
    let mut seen_providers = HashSet::new();
    for provider in providers {
        let id = provider.id.trim();
        if !valid_custom_provider_id(id) {
            return Err(format!(
                "Pi custom provider “{id}”无效：只能包含字母、数字、-、_、.，且必须以字母或数字开头"
            ));
        }
        if !seen_providers.insert(id.to_string()) {
            return Err(format!("Pi custom provider “{id}”重复"));
        }

        let base_url = provider.base_url.trim();
        if !valid_custom_provider_base_url(base_url) {
            return Err(format!(
                "Pi custom provider “{id}”的 Base URL 必须以 http:// 或 https:// 开头"
            ));
        }
        let api = provider.api.trim();
        if !PI_CUSTOM_PROVIDER_APIS.contains(&api) {
            return Err(format!(
                "Pi custom provider “{id}”的 Type “{api}”不受 Pi 支持"
            ));
        }
        validate_custom_provider_models(id, &provider.models, &PI_CUSTOM_PROVIDER_APIS, "Pi")?;

        if provider
            .api_key
            .as_deref()
            .map(str::trim)
            .is_some_and(str::is_empty)
        {
            return Err(format!("Pi custom provider “{id}”的 API key 不能为空"));
        }
    }
    Ok(())
}

fn custom_provider_json(existing: Option<&JsonValue>, provider: &CustomProviderSave) -> JsonValue {
    let mut config = existing
        .and_then(JsonValue::as_object)
        .cloned()
        .unwrap_or_default();
    config.insert(
        "baseUrl".to_string(),
        JsonValue::String(provider.base_url.trim().to_string()),
    );
    config.remove("api");

    if let Some(api_key) = provider
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
    {
        config.insert("apiKey".to_string(), JsonValue::String(api_key.to_string()));
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
    if let Some(existing_models) = config.get("models").and_then(JsonValue::as_array) {
        for model in existing_models {
            let Some(id) = json_model_id(model) else {
                continue;
            };
            let Some(api) = model_apis.get(id.as_str()) else {
                continue;
            };
            if selected_models.contains(id.as_str()) && retained_ids.insert(id.clone()) {
                let mut model_config = model.as_object().cloned().unwrap_or_default();
                model_config.insert("id".to_string(), JsonValue::String(id.clone()));
                model_config.insert("api".to_string(), JsonValue::String((*api).to_string()));
                if let Some(metadata) = model_metadata.get(id.as_str()) {
                    apply_pi_model_capabilities(&mut model_config, metadata);
                }
                retained_models.push(JsonValue::Object(model_config));
            }
        }
    }
    for model in &provider.models {
        let id = model.id.trim();
        if retained_ids.insert(id.to_string()) {
            let mut model_value = serde_json::Map::new();
            model_value.insert("id".to_string(), JsonValue::String(id.to_string()));
            model_value.insert(
                "api".to_string(),
                JsonValue::String(model.api.trim().to_string()),
            );
            apply_pi_model_capabilities(&mut model_value, model);
            retained_models.push(JsonValue::Object(model_value));
        }
    }
    retained_models.sort_by(|left, right| json_model_id(left).cmp(&json_model_id(right)));
    config.insert("models".to_string(), JsonValue::Array(retained_models));
    JsonValue::Object(config)
}

fn set_pi_custom_providers(
    root: &mut serde_json::Map<String, JsonValue>,
    providers: &[CustomProviderSave],
    removed: &[String],
) {
    let mut next = root
        .get("providers")
        .and_then(JsonValue::as_object)
        .cloned()
        .unwrap_or_default();
    for provider in removed {
        next.remove(provider.trim());
    }
    for provider in providers {
        let id = provider.id.trim();
        let existing = next.get(id).cloned();
        next.insert(
            id.to_string(),
            custom_provider_json(existing.as_ref(), provider),
        );
    }
    root.insert("providers".to_string(), JsonValue::Object(next));
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

fn custom_provider_request(
    client: &reqwest::Client,
    url: &str,
    api: &str,
    api_key: &str,
) -> reqwest::RequestBuilder {
    let mut request_builder = client
        .get(url)
        .header("Accept", "application/json")
        .header("Authorization", format!("Bearer {api_key}"));
    if api == "anthropic-messages" {
        request_builder = request_builder
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01");
    }
    request_builder
}

async fn fetch_optional_custom_provider_rich_catalog(
    client: &reqwest::Client,
    models_url: &str,
    api: &str,
    api_key: &str,
    supported_apis: &[&str],
) -> Option<Vec<CustomProviderModel>> {
    let rich_url = custom_provider_rich_models_url(models_url);
    let response = custom_provider_request(client, &rich_url, api, api_key)
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body = response.text().await.ok()?;
    parse_custom_provider_model_list(&body, supported_apis)
        .ok()
        .filter(|models| !models.is_empty())
}

fn resolve_custom_provider_api_key(raw_key: &str) -> Option<String> {
    let raw_key = raw_key.trim();
    if raw_key.is_empty() {
        return None;
    }
    let resolved = env::var(raw_key).unwrap_or_else(|_| raw_key.to_string());
    (!resolved.trim().is_empty()).then(|| resolved.trim().to_string())
}
fn discovered_custom_provider_model_api(
    value: &JsonValue,
    supported_apis: &[&str],
) -> Option<String> {
    let object = value.as_object()?;
    if let Some(api) = object
        .get("api")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|api| supported_apis.contains(api))
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
    supported_apis
        .contains(&candidate)
        .then(|| candidate.to_string())
}

fn parse_custom_provider_model_list(
    content: &str,
    supported_apis: &[&str],
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
                let api = discovered_custom_provider_model_api(value, supported_apis);
                (id, name, api)
            }
            _ => continue,
        };
        if id.is_empty() || !seen.insert(id.clone()) {
            continue;
        }
        let hidden = json_model_is_hidden(value);
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
        if model.name == model.id && rich_model.name != rich_model.id {
            model.name = rich_model.name.clone();
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

fn supports_effort_model_metadata(api: &str) -> bool {
    matches!(
        api,
        "openai-completions"
            | "openai-responses"
            | "openai-codex-responses"
            | "azure-openai-responses"
    )
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
    if !supports_effort_model_metadata(model.api.trim()) {
        return;
    }
    let efforts = model_effort_levels(model);
    if efforts.is_empty() {
        return;
    }
    config.insert(
        YamlValue::String("reasoning".to_string()),
        YamlValue::Bool(true),
    );
    let mut thinking = config
        .get(YamlValue::String("thinking".to_string()))
        .and_then(YamlValue::as_mapping)
        .cloned()
        .unwrap_or_default();
    thinking.insert(
        YamlValue::String("mode".to_string()),
        YamlValue::String("effort".to_string()),
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
    } else {
        thinking.remove(&YamlValue::String("defaultLevel".to_string()));
    }
    config.insert(
        YamlValue::String("thinking".to_string()),
        YamlValue::Mapping(thinking),
    );
}

fn apply_pi_model_capabilities(
    config: &mut serde_json::Map<String, JsonValue>,
    model: &CustomProviderSaveModel,
) {
    let levels = normalize_custom_provider_thinking_levels(&model.thinking);
    if levels.is_empty() {
        return;
    }
    config.insert("reasoning".to_string(), JsonValue::Bool(true));
    let present = levels.iter().cloned().collect::<HashSet<_>>();
    let mut level_map = config
        .get("thinkingLevelMap")
        .and_then(JsonValue::as_object)
        .cloned()
        .unwrap_or_default();
    for level in THINKING_LEVELS {
        if level == "off" {
            if present.contains(level) {
                level_map.insert(level.to_string(), JsonValue::String("none".to_string()));
            }
        } else if present.contains(level) {
            level_map.insert(level.to_string(), JsonValue::String(level.to_string()));
        } else {
            level_map.insert(level.to_string(), JsonValue::Null);
        }
    }
    config.insert("thinkingLevelMap".to_string(), JsonValue::Object(level_map));
}

fn parse_models_json(content: &str) -> Result<Vec<ModelOption>, String> {
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
        let models = parse_models_json(&String::from_utf8_lossy(&output.stdout))?;
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
    let (models, model_executable) = load_omp_models(custom_omp_path)?;

    Ok(ConfigSnapshot {
        config_path: display_path(&config),
        config_exists,
        model_roles,
        agent_model_overrides,
        models,
        model_source: format!("{} · omp models --json", display_path(&model_executable)),
        agent_files,
        routing_settings,
        custom_providers: load_custom_providers(&models_root),
        warnings,
    })
}

fn write_yaml_atomically(path: &Path, value: &YamlValue) -> Result<(), String> {
    let content = serde_yaml::to_string(value)
        .map_err(|error| format!("生成 {} 失败: {error}", display_path(path)))?;
    fs::create_dir_all(
        path.parent()
            .ok_or_else(|| "无法确定配置目录".to_string())?,
    )
    .map_err(|error| format!("创建配置目录失败: {error}"))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config.yml");
    let temporary = path.with_file_name(format!(".{file_name}.omp-model-studio.tmp"));
    fs::write(&temporary, content).map_err(|error| format!("写入临时配置失败: {error}"))?;
    fs::rename(&temporary, path).map_err(|error| format!("替换配置文件失败: {error}"))?;
    Ok(())
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
            .ok_or_else(|| "无法确定 Agent 配置目录".to_string())?,
    )
    .map_err(|error| format!("创建 Agent 配置目录失败: {error}"))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Agent Markdown 文件名无效".to_string())?;
    let temporary = path.with_file_name(format!(".{file_name}.omp-model-studio.tmp"));
    fs::write(&temporary, content)
        .map_err(|error| format!("写入 Agent Markdown 临时文件失败: {error}"))?;
    fs::rename(&temporary, path).map_err(|error| format!("替换 Agent Markdown 失败: {error}"))?;
    Ok(())
}

fn selector_provider(selector: &str) -> Option<&str> {
    let trimmed = selector.trim();
    if trimmed.is_empty() || trimmed.starts_with('@') {
        return None;
    }
    let base = trimmed
        .rsplit_once(':')
        .filter(|(_, suffix)| THINKING_LEVELS.contains(suffix))
        .map_or(trimmed, |(base, _)| base);
    base.split_once('/').map(|(provider, _)| provider)
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PiModelOption {
    provider: String,
    id: String,
    selector: String,
    context_window: Option<u64>,
    max_tokens: Option<u64>,
    reasoning: bool,
    images: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    thinking: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_thinking_level: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PiTier {
    /// Complete `provider/model-id:thinking` spec for the primary attempt.
    model: String,
    #[serde(default)]
    /// Complete model specs tried after the primary, each with its own thinking suffix.
    fallback_models: Vec<String>,
}

fn default_max_recursion_depth() -> i64 {
    2
}

/// Studio's UI-facing model tier shape. The persisted pi-roster block uses
/// `defaultModelProfile` and `modelProfiles`; conversion helpers below keep
/// that external contract at the settings boundary while preserving the
/// existing Tier-oriented UI model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PiTiersFile {
    #[serde(default)]
    default: String,
    #[serde(default)]
    tiers: BTreeMap<String, PiTier>,
    #[serde(default)]
    assignments: BTreeMap<String, Vec<String>>,
    /// Agent names pi-roster must never dispatch to, regardless of tier
    /// assignment. Unlike pi-subagents' `disableBuiltins`, pi-roster ships no
    /// bundled personas, so there is nothing else to "turn off defaults" for.
    #[serde(default)]
    disabled_agents: Vec<String>,
    /// Negative means unlimited, matching oh-my-pi's own semantics that
    /// pi-roster's spawn-depth guard is ported from.
    #[serde(default = "default_max_recursion_depth")]
    max_recursion_depth: i64,
}

impl Default for PiTiersFile {
    fn default() -> Self {
        Self {
            default: String::new(),
            tiers: BTreeMap::new(),
            assignments: BTreeMap::new(),
            disabled_agents: Vec::new(),
            max_recursion_depth: default_max_recursion_depth(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PiRosterProfile {
    model: String,
    #[serde(default)]
    fallback_models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PiRosterConfig {
    #[serde(default)]
    default_model_profile: String,
    #[serde(default)]
    model_profiles: BTreeMap<String, PiRosterProfile>,
    #[serde(default)]
    assignments: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    disabled_agents: Vec<String>,
    #[serde(default = "default_max_recursion_depth")]
    max_recursion_depth: i64,
}

fn pi_roster_config_to_pi_tiers(config: PiRosterConfig) -> PiTiersFile {
    let tiers = config
        .model_profiles
        .into_iter()
        .map(|(name, profile)| {
            (
                name,
                PiTier {
                    model: profile.model,
                    fallback_models: profile.fallback_models,
                },
            )
        })
        .collect();
    PiTiersFile {
        default: config.default_model_profile,
        tiers,
        assignments: config.assignments,
        disabled_agents: config.disabled_agents,
        max_recursion_depth: config.max_recursion_depth,
    }
}

fn pi_tiers_to_roster_config(tiers: &PiTiersFile) -> PiRosterConfig {
    let model_profiles = tiers
        .tiers
        .iter()
        .map(|(name, tier)| {
            (
                name.clone(),
                PiRosterProfile {
                    model: tier.model.clone(),
                    fallback_models: tier.fallback_models.clone(),
                },
            )
        })
        .collect();
    PiRosterConfig {
        default_model_profile: tiers.default.clone(),
        model_profiles,
        assignments: tiers.assignments.clone(),
        disabled_agents: tiers.disabled_agents.clone(),
        max_recursion_depth: tiers.max_recursion_depth,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PiSnapshot {
    settings_path: String,
    roster_config_exists: bool,
    tiers: PiTiersFile,
    models: Vec<PiModelOption>,
    custom_providers: Vec<CustomProvider>,
    agent_names: Vec<String>,
    shadowed_providers: Vec<String>,
    warnings: Vec<String>,
}

fn pi_agent_dir() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".pi").join("agent"))
}
fn pi_models_file(agent: &Path) -> PathBuf {
    agent.join("models.json")
}

fn pi_executable_candidates(custom_path: Option<&str>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(custom) = custom_path.map(str::trim).filter(|value| !value.is_empty()) {
        candidates.push(PathBuf::from(custom));
    }
    if let Some(path) = env::var_os("PATH") {
        candidates.extend(env::split_paths(&path).map(|directory| directory.join("pi")));
    }
    #[cfg(target_os = "macos")]
    candidates.push(PathBuf::from("/opt/homebrew/bin/pi"));
    if let Ok(home) = home_dir() {
        candidates.push(home.join(".local").join("bin").join("pi"));
        candidates.push(home.join(".bun").join("bin").join("pi"));
    }
    candidates.push(PathBuf::from("pi"));
    let mut seen = HashSet::new();
    candidates.retain(|candidate| seen.insert(candidate.clone()));
    candidates
}
fn load_pi_models(custom_path: Option<&str>) -> Result<(Vec<PiModelOption>, PathBuf), String> {
    let candidates = pi_executable_candidates(custom_path);
    let mut failure_reasons = Vec::new();
    for candidate in &candidates {
        let mut command = Command::new(candidate);
        command.arg("--list-models").env("NO_COLOR", "1");
        command.env("PATH", build_subprocess_path(candidate.parent()));
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
        let models = parse_pi_models_table(&String::from_utf8_lossy(&output.stdout));
        return Ok((models, candidate.clone()));
    }
    Err(format!(
        "未找到可用的 pi 可执行文件或无法通过 `pi --list-models` 读取模型。\n尝试详情：\n{}",
        failure_reasons.join("\n")
    ))
}

fn parse_size_token(token: &str) -> Option<u64> {
    let trimmed = token.trim();
    let (numeric, multiplier) = if let Some(prefix) = trimmed.strip_suffix(['M', 'm']) {
        (prefix, 1_000_000.0)
    } else if let Some(prefix) = trimmed.strip_suffix(['K', 'k']) {
        (prefix, 1_000.0)
    } else {
        (trimmed, 1.0)
    };
    numeric
        .parse::<f64>()
        .ok()
        .map(|value| (value * multiplier).round() as u64)
}

fn parse_pi_models_table(content: &str) -> Vec<PiModelOption> {
    let mut models = Vec::new();
    for line in content.lines() {
        let columns: Vec<&str> = line.split_whitespace().collect();
        if columns.len() != 6 || columns[0].eq_ignore_ascii_case("provider") {
            continue;
        }
        let [provider, id, context, max_out, thinking, images] = [
            columns[0], columns[1], columns[2], columns[3], columns[4], columns[5],
        ];
        models.push(PiModelOption {
            provider: provider.to_string(),
            id: id.to_string(),
            selector: format!("{provider}/{id}"),
            context_window: parse_size_token(context),
            max_tokens: parse_size_token(max_out),
            reasoning: thinking.eq_ignore_ascii_case("yes"),
            images: images.eq_ignore_ascii_case("yes"),
            thinking: Vec::new(),
            default_thinking_level: None,
        });
    }
    models
}

fn agent_name_from_markdown(content: &str) -> Option<String> {
    let block = content.strip_prefix("---")?;
    let end = block.find("\n---")?;
    for line in block[..end].lines() {
        if let Some(value) = line.trim().strip_prefix("name:") {
            let value = value.trim().trim_matches('"').trim_matches('\'');
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// pi-roster ships zero bundled personas (unlike pi-subagents' six builtins),
/// so the roster is exactly whatever `.md` files exist here — no seed list.
fn discover_pi_agent_names(agent: &Path) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let agents_dir = agent.join("agents");
    let mut stack = vec![agents_dir];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("md") {
                if let Ok(content) = fs::read_to_string(&path) {
                    if let Some(name) = agent_name_from_markdown(&content) {
                        if !names.contains(&name) {
                            names.push(name);
                        }
                    }
                }
            }
        }
    }
    names
}

const ROSTER_CONFIG_KEY: &str = "pi-roster";

/// Extracts and parses the `"pi-roster"` block from an already-loaded
/// settings map. The persisted keys follow pi-roster's model-profile schema;
/// the result is translated into the Studio-facing Tier shape.
fn load_pi_roster_config(
    settings: &serde_json::Map<String, JsonValue>,
) -> Result<(PiTiersFile, bool), String> {
    let Some(block) = settings.get(ROSTER_CONFIG_KEY) else {
        return Ok((PiTiersFile::default(), false));
    };
    let parsed: PiRosterConfig = serde_json::from_value(block.clone()).map_err(|error| {
        format!("解析 settings.json 中的 “{ROSTER_CONFIG_KEY}” 配置块失败: {error}")
    })?;
    Ok((pi_roster_config_to_pi_tiers(parsed), true))
}

/// `pi`'s settings.json shares the same coding-agent core as OMP's config.yml,
/// so top-level `disabledProviders` filters model discovery pi-wide (not just
/// the `"pi-roster"` block) exactly like OMP's does.
fn load_pi_settings_map(path: &Path) -> Result<serde_json::Map<String, JsonValue>, String> {
    if !path.exists() {
        return Ok(serde_json::Map::new());
    }
    let content = fs::read_to_string(path)
        .map_err(|error| format!("无法读取 {}: {error}", display_path(path)))?;
    let parsed: JsonValue = serde_json::from_str(&content)
        .map_err(|error| format!("解析 {} 失败: {error}", display_path(path)))?;
    match parsed {
        JsonValue::Object(map) => Ok(map),
        _ => Err(format!("{} 不是合法的 JSON 对象", display_path(path))),
    }
}

fn strip_json_comments(content: &str) -> String {
    let chars = content.chars().collect::<Vec<_>>();
    let mut without_comments = String::with_capacity(content.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut line_comment = false;
    let mut index = 0;
    while index < chars.len() {
        let character = chars[index];
        if line_comment {
            if character == '\n' {
                line_comment = false;
                without_comments.push(character);
            }
            index += 1;
            continue;
        }
        if in_string {
            without_comments.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if character == '"' {
            in_string = true;
            without_comments.push(character);
            index += 1;
        } else if character == '/' && chars.get(index + 1) == Some(&'/') {
            line_comment = true;
            index += 2;
        } else {
            without_comments.push(character);
            index += 1;
        }
    }

    let chars = without_comments.chars().collect::<Vec<_>>();
    let mut result = String::with_capacity(without_comments.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut index = 0;
    while index < chars.len() {
        let character = chars[index];
        if in_string {
            result.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if character == '"' {
            in_string = true;
            result.push(character);
            index += 1;
            continue;
        }
        if character == ',' {
            let mut next = index + 1;
            while chars
                .get(next)
                .is_some_and(|character| character.is_whitespace())
            {
                next += 1;
            }
            if matches!(chars.get(next), Some(&'}') | Some(&']')) {
                index += 1;
                continue;
            }
        }
        result.push(character);
        index += 1;
    }
    result
}
fn load_pi_models_map(path: &Path) -> Result<(serde_json::Map<String, JsonValue>, bool), String> {
    if !path.exists() {
        return Ok((serde_json::Map::new(), false));
    }
    let content = fs::read_to_string(path)
        .map_err(|error| format!("无法读取 {}: {error}", display_path(path)))?;
    if content.trim().is_empty() {
        return Ok((serde_json::Map::new(), true));
    }
    let sanitized = strip_json_comments(&content);
    let parsed: JsonValue = serde_json::from_str(&sanitized)
        .map_err(|error| format!("解析 {} 失败: {error}", display_path(path)))?;
    match parsed {
        JsonValue::Object(map) => Ok((map, true)),
        _ => Err(format!("{} 不是合法的 JSON 对象", display_path(path))),
    }
}

fn ensure_pi_models_file(path: &Path) -> Result<(), String> {
    let needs_initialization = match fs::read_to_string(path) {
        Ok(content) => content.trim().is_empty(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(error) => {
            return Err(format!("无法读取 {}: {error}", display_path(path)));
        }
    };
    if needs_initialization {
        write_text_atomically(path, "{\n  \"providers\": {}\n}\n")?;
    }
    Ok(())
}

fn json_string_list(value: Option<&JsonValue>) -> Vec<String> {
    let Some(JsonValue::Array(items)) = value else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(JsonValue::as_str)
        .map(str::to_string)
        .collect()
}

/// Mirrors `set_nested_disabled_providers` for pi's JSON settings: only the
/// global provider-id string entries are managed here; path-scoped entries
/// (`{path, providers}`) configured outside this app are preserved verbatim.
fn set_json_disabled_providers(
    settings: &mut serde_json::Map<String, JsonValue>,
    shadowed: &[String],
) {
    let scoped_entries: Vec<JsonValue> = settings
        .get("disabledProviders")
        .and_then(JsonValue::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter(|entry| !entry.is_string())
                .cloned()
                .collect()
        })
        .unwrap_or_default();

    let mut globals: Vec<String> = shadowed
        .iter()
        .map(|provider| provider.trim().to_string())
        .filter(|provider| !provider.is_empty())
        .collect();
    globals.sort();
    globals.dedup();

    let mut sequence: Vec<JsonValue> = globals.into_iter().map(JsonValue::String).collect();
    sequence.extend(scoped_entries);

    if sequence.is_empty() {
        settings.remove("disabledProviders");
    } else {
        settings.insert("disabledProviders".to_string(), JsonValue::Array(sequence));
    }
}

fn overlay_pi_model_capabilities(models: &mut [PiModelOption], providers: &[CustomProvider]) {
    for model in models {
        let Some(custom_model) = providers
            .iter()
            .find(|provider| provider.id == model.provider)
            .and_then(|provider| {
                provider
                    .models
                    .iter()
                    .find(|candidate| candidate.id == model.id)
            })
        else {
            continue;
        };
        if custom_model.thinking.is_empty() {
            continue;
        }
        model.reasoning = true;
        model.thinking = custom_model.thinking.clone();
        model.default_thinking_level = custom_model.default_thinking_level.clone();
    }
}

fn load_pi_snapshot_from_disk(custom_pi_path: Option<&str>) -> Result<PiSnapshot, String> {
    let agent = pi_agent_dir()?;
    let settings_path = agent.join("settings.json");
    let models_path = pi_models_file(&agent);
    let settings = load_pi_settings_map(&settings_path)?;
    let (models_config, _) = load_pi_models_map(&models_path)?;
    let (tiers, roster_config_exists) = load_pi_roster_config(&settings)?;
    let mut warnings = Vec::new();
    if !roster_config_exists {
        warnings.push(format!(
            "{} 中没有 “{ROSTER_CONFIG_KEY}” 配置块，将从空白配置开始",
            display_path(&settings_path)
        ));
    }
    let shadowed_providers = json_string_list(settings.get("disabledProviders"));
    let (mut models, model_executable) = load_pi_models(custom_pi_path)?;
    let custom_providers = load_pi_custom_providers(&models_config);
    overlay_pi_model_capabilities(&mut models, &custom_providers);
    let agent_names = discover_pi_agent_names(&agent);
    warnings.push(format!(
        "模型目录来源：{} · pi --list-models",
        display_path(&model_executable)
    ));

    Ok(PiSnapshot {
        settings_path: display_path(&settings_path),
        roster_config_exists,
        tiers,
        models,
        custom_providers,
        agent_names,
        shadowed_providers,
        warnings,
    })
}

fn valid_pi_roster_model_spec(selector: &str) -> bool {
    let trimmed = selector.trim();
    if trimmed.is_empty() || trimmed.starts_with('@') {
        return false;
    }
    let Some((base_model, thinking)) = trimmed.rsplit_once(':') else {
        return false;
    };
    if !THINKING_LEVELS.contains(&thinking) {
        return false;
    }
    let Some((provider, model_id)) = base_model.split_once('/') else {
        return false;
    };
    !provider.is_empty() && !model_id.is_empty()
}

fn validate_pi_tiers(tiers: &PiTiersFile, shadowed: &HashSet<&str>) -> Result<(), String> {
    if tiers.tiers.is_empty() {
        return Err("至少需要一个 model profile".to_string());
    }
    let default_profile = tiers.default.trim();
    if default_profile.is_empty() {
        return Err("必须设置 default model profile".to_string());
    }
    if !tiers.tiers.contains_key(default_profile) {
        return Err(format!("default model profile “{}”不存在", tiers.default));
    }

    for (tier_name, tier) in &tiers.tiers {
        if tier_name.trim().is_empty() {
            return Err("model profile 名称不能为空".to_string());
        }
        if !valid_pi_roster_model_spec(&tier.model) {
            return Err(format!(
                "Model profile “{tier_name}”的主模型必须是 provider/model-id:thinking 格式"
            ));
        }
        if selector_provider(&tier.model).is_some_and(|provider| shadowed.contains(provider)) {
            return Err(format!(
                "Model profile “{tier_name}”的主模型仍使用 shadowed provider"
            ));
        }
        for selector in &tier.fallback_models {
            if !valid_pi_roster_model_spec(selector) {
                return Err(format!(
                    "Model profile “{tier_name}”的 fallback 必须是 provider/model-id:thinking 格式"
                ));
            }
            if selector_provider(selector).is_some_and(|provider| shadowed.contains(provider)) {
                return Err(format!(
                    "Model profile “{tier_name}”的 fallback 仍使用 shadowed provider"
                ));
            }
        }
    }

    let mut seen_agents = HashSet::new();
    for (tier_name, agents) in &tiers.assignments {
        if !tiers.tiers.contains_key(tier_name) {
            return Err(format!(
                "assignments 引用了不存在的 model profile “{tier_name}”"
            ));
        }
        let mut seen_in_profile = HashSet::new();
        for agent in agents {
            let agent_name = agent.trim();
            if agent_name.is_empty() {
                return Err(format!("assignments.{tier_name} 包含空 Agent 名称"));
            }
            if !seen_in_profile.insert(agent_name.to_string()) {
                continue;
            }
            if !seen_agents.insert(agent_name.to_string()) {
                return Err(format!("Agent “{agent_name}”被分配到多个 model profile"));
            }
        }
    }
    Ok(())
}

#[tauri::command]
fn load_pi_snapshot(pi_path: Option<String>) -> Result<PiSnapshot, String> {
    load_pi_snapshot_from_disk(pi_path.as_deref())
}

#[tauri::command]
fn save_pi_tiers(
    tiers: PiTiersFile,
    shadowed_providers: Vec<String>,
    pi_path: Option<String>,
) -> Result<PiSnapshot, String> {
    let shadowed = shadowed_providers
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    validate_pi_tiers(&tiers, &shadowed)?;
    let agent = pi_agent_dir()?;
    let settings_path = agent.join("settings.json");

    let mut settings = load_pi_settings_map(&settings_path)?;
    set_json_disabled_providers(&mut settings, &shadowed_providers);

    // Pi-roster reads the model-profile block directly. Translate the
    // Studio's Tier-oriented editing model at this persistence boundary
    // instead of writing the obsolete `default`/`tiers` keys.
    let roster_config = pi_tiers_to_roster_config(&tiers);
    let roster_value = serde_json::to_value(&roster_config)
        .map_err(|error| format!("序列化 “{ROSTER_CONFIG_KEY}” 配置块失败: {error}"))?;
    settings.insert(ROSTER_CONFIG_KEY.to_string(), roster_value);

    let settings_json = serde_json::to_string_pretty(&JsonValue::Object(settings))
        .map_err(|error| format!("序列化 settings.json 失败: {error}"))?;
    write_text_atomically(&settings_path, &format!("{settings_json}\n"))?;

    load_pi_snapshot_from_disk(pi_path.as_deref())
}
#[tauri::command]
async fn discover_custom_provider_models(
    request: CustomProviderDiscoveryRequest,
) -> Result<Vec<CustomProviderModel>, String> {
    let target = request.target.as_deref().unwrap_or("omp");
    let (supported_apis, product_label) = match target {
        "omp" => (&CUSTOM_PROVIDER_APIS[..], "OMP"),
        "pi" => (&PI_CUSTOM_PROVIDER_APIS[..], "Pi"),
        _ => return Err(format!("不支持的 custom provider 配置目标 “{target}”")),
    };
    let provider_id = request.provider_id.trim();
    if !valid_custom_provider_id(provider_id) {
        return Err("Provider ID 无效".to_string());
    }
    let api = request.api.trim();
    if !supported_apis.contains(&api) {
        return Err(format!(
            "Custom provider Type “{api}”不受 {product_label} 支持"
        ));
    }

    let existing_key = match target {
        "omp" => {
            let agent = agent_dir()?;
            let models_path = models_file(&agent);
            let (models_root, _) = load_models_yaml(&models_path)?;
            existing_custom_provider(&models_root, provider_id)
                .and_then(YamlValue::as_mapping)
                .and_then(|config| mapping_string(config, "apiKey"))
        }
        "pi" => {
            let agent = pi_agent_dir()?;
            let models_path = pi_models_file(&agent);
            let (models_root, _) = load_pi_models_map(&models_path)?;
            existing_pi_custom_provider(&models_root, provider_id)
                .and_then(JsonValue::as_object)
                .and_then(|config| config.get("apiKey"))
                .and_then(JsonValue::as_str)
                .map(str::to_string)
        }
        _ => unreachable!("custom provider target was validated above"),
    };
    let raw_key = request
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(str::to_string)
        .or(existing_key);
    let api_key = raw_key
        .as_deref()
        .and_then(resolve_custom_provider_api_key)
        .ok_or_else(|| format!("Custom provider “{provider_id}”需要 API key"))?;
    let urls = custom_provider_model_urls(&request.base_url)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| format!("创建 model discovery 客户端失败: {error}"))?;
    for (index, url) in urls.iter().enumerate() {
        let response = custom_provider_request(&client, url, api, &api_key)
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
                "provider model 列表鉴权失败（HTTP {status}），请检查 API key"
            ));
        }
        let body = response
            .text()
            .await
            .map_err(|error| format!("读取 provider model 响应失败：{error}"))?;
        if !status.is_success() {
            return Err(format!("读取 provider model 列表失败（HTTP {status}）"));
        }
        let models = parse_custom_provider_model_list(&body, supported_apis)?;
        if models.is_empty() {
            return Err("接口未返回模型列表，请确认 Base URL 与 API key".to_string());
        }
        let rich_models = fetch_optional_custom_provider_rich_catalog(
            &client,
            url,
            api,
            &api_key,
            supported_apis,
        )
        .await
        .unwrap_or_default();
        return Ok(merge_custom_provider_model_capabilities(
            models,
            rich_models,
        ));
    }
    Err("provider model discovery 没有可用 endpoint".to_string())
}

fn save_pi_custom_providers_to_disk(
    request: &CustomProviderSaveRequest,
) -> Result<Vec<CustomProvider>, String> {
    let agent = pi_agent_dir()?;
    let models_path = pi_models_file(&agent);
    let (mut root, models_exists) = load_pi_models_map(&models_path)?;
    validate_pi_custom_providers(&request.custom_providers)?;
    validate_removed_custom_providers(
        &request.custom_providers,
        &request.removed_custom_providers,
    )?;
    set_pi_custom_providers(
        &mut root,
        &request.custom_providers,
        &request.removed_custom_providers,
    );
    if models_exists
        || !request.custom_providers.is_empty()
        || !request.removed_custom_providers.is_empty()
    {
        let json = serde_json::to_string_pretty(&JsonValue::Object(root.clone()))
            .map_err(|error| format!("序列化 models.json 失败: {error}"))?;
        write_text_atomically(&models_path, &format!("{json}\n"))?;
    }
    Ok(load_pi_custom_providers(&root))
}
#[tauri::command]
fn save_custom_providers(
    request: CustomProviderSaveRequest,
) -> Result<Vec<CustomProvider>, String> {
    match request.target.as_deref().unwrap_or("omp") {
        "pi" => save_pi_custom_providers_to_disk(&request),
        "omp" => {
            let agent = agent_dir()?;
            let models_path = models_file(&agent);
            let (mut root, models_exists) = load_models_yaml(&models_path)?;
            validate_custom_providers(&root, &request.custom_providers)?;
            validate_removed_custom_providers(
                &request.custom_providers,
                &request.removed_custom_providers,
            )?;
            set_custom_providers(
                &mut root,
                &request.custom_providers,
                &request.removed_custom_providers,
            );
            if models_exists
                || !request.custom_providers.is_empty()
                || !request.removed_custom_providers.is_empty()
            {
                write_yaml_atomically(&models_path, &root)?;
            }
            Ok(load_custom_providers(&root))
        }
        target => Err(format!("不支持的 custom provider 配置目标 “{target}”")),
    }
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
fn open_pi_models_file() -> Result<(), String> {
    let agent = pi_agent_dir()?;
    let path = pi_models_file(&agent);
    ensure_pi_models_file(&path)?;
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
        if !SYSTEM_MODEL_ROLES.contains(&role) && !valid_custom_role(role) {
            return Err(format!(
                "自定义 Role “{role}”无效：仅允许小写字母开头及小写字母、数字、-、_，且不能覆盖 OMP 系统 Role"
            ));
        }
        if !SYSTEM_MODEL_ROLES.contains(&role) && selector.trim().is_empty() {
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
            open_pi_models_file,
            discover_custom_provider_models,
            load_pi_snapshot,
            save_pi_tiers
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
            "providers:\n  gateway:\n    baseUrl: https://old.example.com/v1\n    api: openai-completions\n    apiKey: OLD_KEY\n    headers:\n      X-Team: keep\n    models:\n      - id: keep\n        name: Keep model\n        contextWindow: 123\n        compat:\n          supportsStore: true\n      - id: drop\n        name: Drop model\n  discovery-only:\n    baseUrl: https://discovery.example.com/v1\n    api: openai-completions\n    apiKey: DISCOVERY_KEY\n    discovery:\n      type: openai-models-list\n",
        )
        .expect("test models.yml should be written");

        let previous_home = env::var_os("HOME");
        env::set_var("HOME", &home);
        let result = save_custom_providers(CustomProviderSaveRequest {
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
            target: None,
        });
        if let Some(previous_home) = previous_home {
            env::set_var("HOME", previous_home);
        } else {
            env::remove_var("HOME");
        }

        let providers = result.expect("custom provider save should succeed");
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
            &CUSTOM_PROVIDER_APIS,
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
            &CUSTOM_PROVIDER_APIS,
        )
        .expect("regular model list should parse");
        let rich = parse_custom_provider_model_list(
            r#"{"models":[{"slug":"gpt-5.6-sol","display_name":"GPT 5.6 Sol","default_reasoning_level":"low","supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"},{"effort":"max"},{"effort":"ultra"}]},{"slug":"hidden","visibility":"hide","supported_reasoning_levels":[{"effort":"high"}]},{"slug":"not-in-regular"}]}"#,
            &CUSTOM_PROVIDER_APIS,
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

        let json = custom_provider_json(None, &provider);
        assert_eq!(json["models"][0]["reasoning"], JsonValue::Bool(true));
        assert_eq!(
            json["models"][0]["thinkingLevelMap"]["minimal"],
            JsonValue::Null
        );
        assert_eq!(
            json["models"][0]["thinkingLevelMap"]["low"],
            JsonValue::String("low".to_string())
        );
        assert_eq!(
            json["models"][0]["thinkingLevelMap"]["max"],
            JsonValue::String("max".to_string())
        );
    }

    #[test]
    fn custom_provider_discovery_sends_bearer_header() {
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
            assert!(request.contains("get /v1/models http/1.1"));
            assert!(request.contains("accept: application/json"));
            assert!(request.contains("authorization: bearer test-key"));
            assert!(request.contains("x-api-key: test-key"));
            assert!(request.contains("anthropic-version: 2023-06-01"));
            let body = r#"{"data":[{"id":"model-a"},{"id":"model-b"}]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("test response should be written");
        });

        let previous_home = env::var_os("HOME");
        env::set_var("HOME", &home);
        let result = tauri::async_runtime::block_on(discover_custom_provider_models(
            CustomProviderDiscoveryRequest {
                provider_id: "test-provider".to_string(),
                base_url: format!("http://{address}/v1"),
                api: "anthropic-messages".to_string(),
                api_key: Some("test-key".to_string()),
                target: None,
            },
        ));
        if let Some(previous_home) = previous_home {
            env::set_var("HOME", previous_home);
        } else {
            env::remove_var("HOME");
        }
        server.join().expect("discovery server should finish");

        let models = result.expect("discovery should succeed");
        assert_eq!(
            models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            vec!["model-a", "model-b"]
        );
        fs::remove_dir_all(home).expect("isolated test home should be removed");
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
                    r#"{"data":[{"id":"model-a","name":"model-a"},{"id":"model-b"}]}"#
                } else {
                    assert!(request.contains("get /v1/models?client_version=0.144.1 http/1.1"));
                    r#"{"models":[{"slug":"model-a","display_name":"Model A","default_reasoning_level":"high","supported_reasoning_levels":[{"effort":"low"},{"effort":"high"},{"effort":"ultra"}]}]}"#
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
                target: None,
            },
        ));
        if let Some(previous_home) = previous_home {
            env::set_var("HOME", previous_home);
        } else {
            env::remove_var("HOME");
        }
        server.join().expect("discovery server should finish");

        let models = result.expect("discovery should succeed");
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "model-a");
        assert_eq!(models[0].name, "Model A");
        assert_eq!(models[0].thinking, vec!["low", "high"]);
        assert_eq!(models[0].default_thinking_level.as_deref(), Some("high"));
        assert!(models[1].thinking.is_empty());
        fs::remove_dir_all(home).expect("isolated test home should be removed");
    }
    #[test]
    fn model_catalog_parser_keeps_limits_and_pricing() {
        let models = parse_models_json(
            r#"{"models":[{"provider":"openai-codex","id":"gpt-6-astra","selector":"openai-codex/gpt-6-astra","name":"GPT-6-Astra","reasoning":true,"thinking":["low","medium","high","xhigh","max"],"input":["text","image"],"contextWindow":272000,"maxTokens":128000,"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0}}]}"#,
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
    fn fallback_candidates_prefer_local_bin_over_bun_bin() {
        let _home_guard = HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous_path = env::var_os("PATH");
        env::set_var("PATH", "");
        let omp_candidates = executable_candidates(None);
        let pi_candidates = pi_executable_candidates(None);
        match previous_path {
            Some(path) => env::set_var("PATH", path),
            None => env::remove_var("PATH"),
        }

        let home = home_dir().expect("home directory should be available");
        let local_omp = home.join(".local").join("bin").join("omp");
        let bun_omp = home.join(".bun").join("bin").join("omp");
        let local_pi = home.join(".local").join("bin").join("pi");
        let bun_pi = home.join(".bun").join("bin").join("pi");
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
        assert!(
            pi_candidates
                .iter()
                .position(|candidate| candidate == &local_pi)
                .expect("local pi fallback should be present")
                < pi_candidates
                    .iter()
                    .position(|candidate| candidate == &bun_pi)
                    .expect("bun pi fallback should be present")
        );
    }

    #[test]
    fn parse_size_token_handles_k_and_m_suffixes() {
        assert_eq!(parse_size_token("128K"), Some(128_000));
        assert_eq!(parse_size_token("272K"), Some(272_000));
        assert_eq!(parse_size_token("1.0M"), Some(1_000_000));
        assert_eq!(parse_size_token("262.1K"), Some(262_100));
        assert_eq!(parse_size_token("131.1K"), Some(131_100));
    }

    #[test]
    fn parse_pi_models_table_parses_rows_and_skips_header() {
        let table = "provider      model                context  max-out  thinking  images\nkimi-coding   k3                   1.0M     131.1K   yes       yes   \nopenai-codex  gpt-5.6-luna         272K     128K     yes       yes   \nopenai-codex  gpt-5.3-codex-spark  128K     128K     yes       no    \n";
        let models = parse_pi_models_table(table);
        assert_eq!(models.len(), 3);
        assert_eq!(models[0].selector, "kimi-coding/k3");
        assert_eq!(models[0].context_window, Some(1_000_000));
        assert_eq!(models[0].max_tokens, Some(131_100));
        assert!(models[0].reasoning);
        assert!(models[0].images);
        assert_eq!(models[2].selector, "openai-codex/gpt-5.3-codex-spark");
        assert!(!models[2].images);
    }

    #[test]
    fn validate_pi_tiers_rejects_unknown_default_and_duplicate_agent() {
        let mut tiers = BTreeMap::new();
        tiers.insert(
            "fast".to_string(),
            PiTier {
                model: "openai-codex/gpt-5.6-luna:low".to_string(),
                fallback_models: vec![],
                ..Default::default()
            },
        );
        let mut assignments = BTreeMap::new();
        assignments.insert("fast".to_string(), vec!["scout".to_string()]);

        let no_shadow: HashSet<&str> = HashSet::new();
        let unknown_default = PiTiersFile {
            default: "missing".to_string(),
            tiers: tiers.clone(),
            assignments: assignments.clone(),
            ..Default::default()
        };
        assert!(validate_pi_tiers(&unknown_default, &no_shadow).is_err());

        assignments.insert("also-fast".to_string(), vec!["scout".to_string()]);
        tiers.insert(
            "also-fast".to_string(),
            PiTier {
                model: "openai-codex/gpt-5.5:low".to_string(),
                fallback_models: vec![],
                ..Default::default()
            },
        );
        let duplicate_agent = PiTiersFile {
            default: "fast".to_string(),
            tiers,
            assignments,
            ..Default::default()
        };
        assert!(validate_pi_tiers(&duplicate_agent, &no_shadow).is_err());
    }

    #[test]
    fn validate_pi_tiers_rejects_shadowed_provider_in_model_or_fallback() {
        let mut tiers = BTreeMap::new();
        tiers.insert(
            "fast".to_string(),
            PiTier {
                model: "mistral/small:low".to_string(),
                fallback_models: vec!["openai/gpt:high".to_string()],
                ..Default::default()
            },
        );
        let data = PiTiersFile {
            default: "fast".to_string(),
            tiers,
            assignments: BTreeMap::new(),
            ..Default::default()
        };
        let shadowed: HashSet<&str> = HashSet::from(["mistral"]);
        assert!(validate_pi_tiers(&data, &shadowed).is_err());

        let mut fallback_shadowed = data;
        fallback_shadowed.tiers.get_mut("fast").unwrap().model = "openai/gpt:high".to_string();
        fallback_shadowed
            .tiers
            .get_mut("fast")
            .unwrap()
            .fallback_models = vec!["mistral/backup:low".to_string()];
        assert!(validate_pi_tiers(&fallback_shadowed, &shadowed).is_err());
    }

    #[test]
    fn validate_pi_tiers_rejects_missing_or_unknown_thinking_suffixes() {
        let no_shadow: HashSet<&str> = HashSet::new();
        for invalid_model in [
            "openai-codex/gpt-5.6-luna",
            "openai-codex/gpt-5.6-luna:urgent",
        ] {
            let mut tiers = BTreeMap::new();
            tiers.insert(
                "fast".to_string(),
                PiTier {
                    model: invalid_model.to_string(),
                    fallback_models: vec![],
                    ..Default::default()
                },
            );
            let invalid = PiTiersFile {
                default: "fast".to_string(),
                tiers,
                assignments: BTreeMap::new(),
                ..Default::default()
            };
            assert!(validate_pi_tiers(&invalid, &no_shadow).is_err());
        }

        let mut tiers = BTreeMap::new();
        tiers.insert(
            "fast".to_string(),
            PiTier {
                model: "openai-codex/gpt-5.6-luna:high".to_string(),
                fallback_models: vec!["kimi-coding/k3".to_string()],
                ..Default::default()
            },
        );
        let invalid_fallback = PiTiersFile {
            default: "fast".to_string(),
            tiers,
            assignments: BTreeMap::new(),
            ..Default::default()
        };
        assert!(validate_pi_tiers(&invalid_fallback, &no_shadow).is_err());
    }

    #[test]
    fn load_pi_roster_config_reads_model_owned_thinking_schema() {
        let settings: serde_json::Map<String, JsonValue> = serde_json::from_str(
            r#"{
                "pi-roster": {
                    "defaultModelProfile": "standard",
                    "modelProfiles": {
                        "standard": {
                            "model": "openai-codex/gpt-5.6-luna:high",
                            "fallbackModels": ["kimi-coding/k3:medium"]
                        }
                    },
                    "assignments": {"standard": ["reviewer"]},
                    "disabledAgents": ["worker"],
                    "maxRecursionDepth": -1
                }
            }"#,
        )
        .expect("model-profile settings should parse");

        let (tiers, exists) =
            load_pi_roster_config(&settings).expect("model-profile config should load");
        assert!(exists);
        assert_eq!(tiers.default, "standard");
        assert_eq!(
            tiers.tiers["standard"].model,
            "openai-codex/gpt-5.6-luna:high"
        );
        assert_eq!(
            tiers.tiers["standard"].fallback_models,
            vec!["kimi-coding/k3:medium"]
        );
        assert_eq!(tiers.disabled_agents, vec!["worker"]);
        assert_eq!(tiers.max_recursion_depth, -1);
    }

    #[test]
    fn load_pi_roster_config_rejects_removed_profile_thinking_field() {
        let settings: serde_json::Map<String, JsonValue> = serde_json::from_str(
            r#"{
                "pi-roster": {
                    "defaultModelProfile": "standard",
                    "modelProfiles": {
                        "standard": {
                            "model": "openai-codex/gpt-5.6-luna:high",
                            "thinking": "high"
                        }
                    },
                    "assignments": {}
                }
            }"#,
        )
        .expect("legacy settings should remain valid JSON");

        let error =
            load_pi_roster_config(&settings).expect_err("profile-level thinking must be rejected");
        assert!(error.contains("unknown field"));
        assert!(error.contains("thinking"));
    }

    #[test]
    fn save_pi_tiers_preserves_unrelated_settings_keys() {
        let _home_guard = HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = temporary_home();
        let agent = home.join(".pi").join("agent");
        fs::create_dir_all(&agent).expect("test pi agent directory should be created");
        fs::write(
            agent.join("settings.json"),
            "{\"theme\":\"dark\",\"packages\":[\"npm:pi-subagents\"],\"disabledProviders\":[\"ollama\",{\"path\":\"/repo\",\"providers\":[\"scoped-only\"]}],\"subagents\":{\"modelScope\":{\"enforce\":false}}}",
        )
        .expect("test settings.json should be written");

        let bin_dir = home.join(".bun").join("bin");
        fs::create_dir_all(&bin_dir).expect("mock bin directory should be created");
        let mock_pi = bin_dir.join("pi");
        fs::write(
            &mock_pi,
            "#!/bin/sh\necho 'provider      model  context  max-out  thinking  images'\necho 'openai-codex  gpt-5.6-luna  272K  128K  yes  yes'\n",
        )
        .expect("mock pi script should be written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&mock_pi).expect("metadata").permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&mock_pi, permissions).expect("chmod");
        }

        let mut tiers = BTreeMap::new();
        tiers.insert(
            "fast".to_string(),
            PiTier {
                model: "openai-codex/gpt-5.6-luna:low".to_string(),
                fallback_models: vec!["openai-codex/gpt-5.5:medium".to_string()],
                ..Default::default()
            },
        );
        let mut assignments = BTreeMap::new();
        assignments.insert("fast".to_string(), vec!["scout".to_string()]);
        let request = PiTiersFile {
            default: "fast".to_string(),
            tiers,
            assignments,
            disabled_agents: vec!["worker".to_string()],
            max_recursion_depth: 3,
        };

        let previous_home = env::var_os("HOME");
        env::set_var("HOME", &home);
        let result = save_pi_tiers(request, vec!["mistral".to_string()], None);
        if let Some(previous_home) = previous_home {
            env::set_var("HOME", previous_home);
        }
        let snapshot =
            result.expect("save_pi_tiers should succeed against the mocked pi executable");

        assert_eq!(snapshot.tiers.default, "fast");
        assert_eq!(snapshot.tiers.disabled_agents, vec!["worker".to_string()]);
        assert_eq!(snapshot.tiers.max_recursion_depth, 3);
        assert_eq!(
            snapshot.shadowed_providers,
            vec!["mistral".to_string()],
            "shadowed providers must round-trip through pi's real disabledProviders"
        );
        let written: JsonValue = serde_json::from_str(
            &fs::read_to_string(agent.join("settings.json")).expect("settings.json should exist"),
        )
        .expect("settings.json should stay valid JSON");
        assert_eq!(written["theme"], JsonValue::String("dark".to_string()));
        assert_eq!(
            written["packages"][0],
            JsonValue::String("npm:pi-subagents".to_string())
        );
        assert_eq!(
            written["subagents"]["modelScope"]["enforce"], JsonValue::Bool(false),
            "unrelated pre-existing settings keys (even ones a different extension owns) must survive Apply"
        );
        assert_eq!(
            written["pi-roster"]["defaultModelProfile"],
            JsonValue::String("fast".to_string())
        );
        assert_eq!(
            written["pi-roster"]["modelProfiles"]["fast"]["model"],
            JsonValue::String("openai-codex/gpt-5.6-luna:low".to_string()),
            "pi-roster persists the complete model spec, not the obsolete tiers schema"
        );
        assert_eq!(
            written["pi-roster"]["modelProfiles"]["fast"]["fallbackModels"][0],
            JsonValue::String("openai-codex/gpt-5.5:medium".to_string())
        );
        assert!(
            written["pi-roster"]["modelProfiles"]["fast"]
                .get("thinking")
                .is_none(),
            "profile-level thinking must not be written"
        );
        assert!(
            written["pi-roster"].get("tiers").is_none(),
            "the obsolete tiers key must not be written to settings.json"
        );
        assert_eq!(
            written["pi-roster"]["disabledAgents"][0],
            JsonValue::String("worker".to_string())
        );
        assert_eq!(
            written["pi-roster"]["maxRecursionDepth"],
            JsonValue::from(3)
        );
        assert_eq!(
            json_string_list(written.get("disabledProviders")),
            vec!["mistral".to_string()],
            "shadow toggles must write through to pi's real disabledProviders, fully replacing the prior global entries"
        );
        let disabled_providers = written["disabledProviders"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let scoped_entry_survived = disabled_providers.iter().any(|entry| {
            entry
                .get("providers")
                .and_then(JsonValue::as_array)
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

        fs::remove_dir_all(home).expect("isolated test home should be removed");
    }
    #[test]
    fn pi_provider_discovery_rejects_omp_only_api() {
        let result = tauri::async_runtime::block_on(discover_custom_provider_models(
            CustomProviderDiscoveryRequest {
                provider_id: "gateway".to_string(),
                base_url: "https://example.com/v1".to_string(),
                api: "google-vertex".to_string(),
                api_key: None,
                target: Some("pi".to_string()),
            },
        ));
        let error = result.expect_err("Pi should reject OMP-only provider APIs");
        assert!(error.contains("不受 Pi 支持"));
    }
    #[test]
    fn pi_custom_provider_save_preserves_models_json_fields() {
        let _home_guard = HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = temporary_home();
        let agent = home.join(".pi").join("agent");
        fs::create_dir_all(&agent).expect("test Pi agent directory should be created");
        fs::write(
            agent.join("models.json"),
            r#"{
  // Pi accepts JSONC models files.
  "providers": {
    "gateway": {
      "baseUrl": "https://old.example.com/v1",
      "api": "openai-completions",
      "apiKey": "OLD_KEY",
      "headers": {"X-Team": "keep"},
      "models": [
        {"id": "keep", "name": "Keep model", "contextWindow": 123},
        {"id": "drop", "name": "Drop model"},
      ],
    },
    "unmanaged": {"discovery": {"type": "openai-models-list"}},
  },
  "otherSetting": true,
}"#,
        )
        .expect("test models.json should be written");

        let previous_home = env::var_os("HOME");
        env::set_var("HOME", &home);
        let result = save_custom_providers(CustomProviderSaveRequest {
            custom_providers: vec![CustomProviderSave {
                id: "gateway".to_string(),
                base_url: "https://new.example.com/v1".to_string(),
                api: "openai-responses".to_string(),
                api_key: None,
                models: vec![
                    saved_model("keep", "openai-responses"),
                    saved_model("new", "anthropic-messages"),
                ],
            }],
            removed_custom_providers: Vec::new(),
            target: Some("pi".to_string()),
        });
        if let Some(previous_home) = previous_home {
            env::set_var("HOME", previous_home);
        } else {
            env::remove_var("HOME");
        }

        let providers = result.expect("Pi custom provider save should succeed");
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, "gateway");
        assert!(providers[0].api_key_configured);
        assert_eq!(providers[0].models[0].id, "keep");
        assert_eq!(providers[0].models[0].name, "Keep model");

        let written: JsonValue = serde_json::from_str(
            &fs::read_to_string(agent.join("models.json")).expect("saved models.json should exist"),
        )
        .expect("saved models.json should remain valid JSON");
        assert_eq!(written["otherSetting"], JsonValue::Bool(true));
        assert!(written["providers"]["unmanaged"].is_object());
        assert!(
            written["providers"]["gateway"].get("api").is_none(),
            "managed providers must store API types on each model"
        );
        assert_eq!(
            written["providers"]["gateway"]["apiKey"],
            JsonValue::String("OLD_KEY".to_string())
        );
        assert_eq!(
            written["providers"]["gateway"]["headers"]["X-Team"],
            JsonValue::String("keep".to_string())
        );
        assert_eq!(
            written["providers"]["gateway"]["models"][0]["name"],
            JsonValue::String("Keep model".to_string())
        );
        assert_eq!(
            written["providers"]["gateway"]["models"][0]["api"],
            JsonValue::String("openai-responses".to_string())
        );
        assert_eq!(
            written["providers"]["gateway"]["models"][1]["id"],
            JsonValue::String("new".to_string())
        );
        assert_eq!(
            written["providers"]["gateway"]["models"][1]["api"],
            JsonValue::String("anthropic-messages".to_string())
        );
        assert!(written["providers"]["gateway"]["models"]
            .as_array()
            .unwrap()
            .iter()
            .all(|model| model["id"] != JsonValue::String("drop".to_string())));

        fs::remove_dir_all(home).expect("isolated Pi home should be removed");
    }

    #[test]
    fn pi_custom_provider_validation_rejects_omp_only_api() {
        let provider = CustomProviderSave {
            id: "gateway".to_string(),
            base_url: "https://example.com/v1".to_string(),
            api: "google-vertex".to_string(),
            api_key: None,
            models: vec![saved_model("model", "google-vertex")],
        };
        assert!(validate_pi_custom_providers(&[provider]).is_err());
    }
    #[test]
    fn pi_empty_custom_provider_store_keeps_providers_object() {
        let mut root = serde_json::Map::new();
        set_pi_custom_providers(&mut root, &[], &[]);
        assert_eq!(
            root.get("providers"),
            Some(&JsonValue::Object(serde_json::Map::new()))
        );
    }
    #[test]
    fn pi_models_file_initialization_writes_valid_empty_json() {
        let _home_guard = HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = temporary_home();
        let agent = home.join(".pi").join("agent");
        let path = agent.join("models.json");
        let previous_home = env::var_os("HOME");
        env::set_var("HOME", &home);

        ensure_pi_models_file(&path).expect("missing models.json should be initialized");
        let written: JsonValue = serde_json::from_str(
            &fs::read_to_string(&path).expect("initialized models.json should exist"),
        )
        .expect("initialized models.json should be valid JSON");
        assert_eq!(
            written["providers"],
            JsonValue::Object(serde_json::Map::new())
        );

        fs::write(&path, " \n\t").expect("blank models.json should be writable");
        ensure_pi_models_file(&path).expect("blank models.json should be repaired");
        let repaired: JsonValue = serde_json::from_str(
            &fs::read_to_string(&path).expect("repaired models.json should exist"),
        )
        .expect("repaired models.json should be valid JSON");
        assert_eq!(
            repaired["providers"],
            JsonValue::Object(serde_json::Map::new())
        );

        if let Some(previous_home) = previous_home {
            env::set_var("HOME", previous_home);
        } else {
            env::remove_var("HOME");
        }
        fs::remove_dir_all(home).expect("isolated test home should be removed");
    }
}
