use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use serde_yaml::{Mapping, Value as YamlValue};
use std::{
    collections::{BTreeMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

const THINKING_LEVELS: [&str; 7] = ["minimal", "low", "medium", "high", "xhigh", "max", "off"];
const CONFIGURED_THINKING_LEVELS: [&str; 8] = [
    "auto", "minimal", "low", "medium", "high", "xhigh", "max", "off",
];
const ADVISOR_SYNC_BACKLOGS: [&str; 4] = ["off", "1", "3", "5"];
const FALLBACK_REVERT_POLICIES: [&str; 2] = ["cooldown-expiry", "never"];
const AUTO_THINKING_MAX_EFFORTS: [&str; 2] = ["xhigh", "max"];
const SYSTEM_MODEL_ROLES: [&str; 10] = [
    "default", "smol", "slow", "vision", "plan", "designer", "commit", "tiny", "task", "advisor",
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
    shadowed_providers: Vec<String>,
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

        if let Some(model) = model_from_parts(
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
        paths.push(home.join(".bun").join("bin"));
        paths.push(home.join(".local").join("bin"));
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
        candidates.push(home.join(".bun").join("bin").join("omp"));
        candidates.push(home.join(".local").join("bin").join("omp"));
    }
    candidates.push(PathBuf::from("omp"));
    candidates
}

fn load_omp_models(custom_path: Option<&str>) -> Result<Vec<ModelOption>, String> {
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
        return parse_models_json(&String::from_utf8_lossy(&output.stdout));
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
    let (root, config_exists) = load_yaml(&config)?;
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
    let models = load_omp_models(custom_omp_path)?;

    Ok(ConfigSnapshot {
        config_path: display_path(&config),
        config_exists,
        model_roles,
        agent_model_overrides,
        models,
        model_source: "omp models --json".to_string(),
        agent_files,
        routing_settings,
        warnings,
    })
}

fn write_yaml_atomically(path: &Path, value: &YamlValue) -> Result<(), String> {
    let content =
        serde_yaml::to_string(value).map_err(|error| format!("生成 config.yml 失败: {error}"))?;
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

    let shadowed = request
        .shadowed_providers
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    for (label, selector) in request
        .model_roles
        .iter()
        .chain(request.agent_model_overrides.iter())
    {
        if selector_provider(selector).is_some_and(|provider| shadowed.contains(provider)) {
            return Err(format!("“{label}”仍使用 shadowed provider"));
        }
    }
    for chain in &request.routing_settings.retry_fallback_chains {
        for selector in &chain.selectors {
            if selector_provider(selector).is_some_and(|provider| shadowed.contains(provider)) {
                return Err(format!(
                    "Fallback target “{}”仍使用 shadowed provider",
                    chain.target.trim()
                ));
            }
        }
    }

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
        .invoke_handler(tauri::generate_handler![load_snapshot, save_snapshot])
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

    #[test]
    fn save_updates_role_assignments_without_clobbering_other_settings() {
        let home = temporary_home();
        let agent = home.join(".omp").join("agent");
        fs::create_dir_all(&agent).expect("test config directory should be created");
        fs::write(
            agent.join("config.yml"),
            "modelRoles:\n  default: old/default\n  keep: keep/model\n  remove-me: old/remove\ndisabledProviders:\n  - ollama\ntask:\n  agentModelOverrides:\n    keep-agent: keep/agent\notherSetting: preserved\n",
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
            shadowed_providers: vec!["ollama".to_string()],
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
            "OMP disabledProviders must remain untouched by Model Studio"
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
    fn model_catalog_parser_keeps_limits_and_pricing() {
        let models = parse_models_json(
            r#"{"models":[{"provider":"openai-codex","id":"gpt-5.6-terra","name":"GPT-5.6-Terra","reasoning":true,"thinking":["low","high","xhigh"],"input":["text","image"],"contextWindow":372000,"maxTokens":128000,"cost":{"input":2,"output":12,"cacheRead":0.2,"cacheWrite":2.5}}]}"#,
        )
        .expect("model catalog should parse");

        assert_eq!(models.len(), 1);
        let model = &models[0];
        assert_eq!(model.context_window, Some(372_000));
        assert_eq!(model.max_tokens, Some(128_000));
        let cost = model.cost.as_ref().expect("pricing should be retained");
        assert_eq!(cost.input, 2.0);
        assert_eq!(cost.output, 12.0);
        assert_eq!(cost.cache_read, 0.2);
        assert_eq!(cost.cache_write, 2.5);
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
}
