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
const SYSTEM_MODEL_ROLES: [&str; 10] = [
    "default", "smol", "slow", "vision", "plan", "designer", "commit", "tiny", "task", "advisor",
];

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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigSnapshot {
    config_path: String,
    config_exists: bool,
    model_roles: BTreeMap<String, String>,
    agent_model_overrides: BTreeMap<String, String>,
    disabled_providers: Vec<String>,
    models: Vec<ModelOption>,
    model_source: String,
    agent_files: Vec<AgentDefinitionFile>,
    warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CustomRoleSave {
    role: String,
    selector: String,
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
    smol: String,
    default: String,
    slow: String,
    vision: String,
    advisor: String,
    custom_roles: Vec<CustomRoleSave>,
    agent_files: Vec<AgentFileSave>,
    reviewer: String,
    security_reviewer: String,
    disabled_providers: Vec<String>,
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

fn set_nested_string(root: &mut YamlValue, path: &[&str], value: &str) {
    if path.is_empty() {
        return;
    }

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
    if value.trim().is_empty() {
        map.remove(&key);
    } else {
        map.insert(key, YamlValue::String(value.trim().to_string()));
    }
}
fn set_nested_string_list(root: &mut YamlValue, path: &[&str], values: &[String]) {
    if path.is_empty() {
        return;
    }

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
    if values.is_empty() {
        map.remove(&key);
    } else {
        map.insert(
            key,
            YamlValue::Sequence(values.iter().cloned().map(YamlValue::String).collect()),
        );
    }
}

fn model_from_parts(
    provider: String,
    id: String,
    name: String,
    reasoning: bool,
    thinking: Vec<String>,
    input: Vec<String>,
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

        if let Some(model) = model_from_parts(provider, id, name, reasoning, thinking, input) {
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

fn executable_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
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

fn load_omp_models() -> Result<Vec<ModelOption>, String> {
    let mut last_error = "找不到 omp 可执行文件".to_string();
    for candidate in executable_candidates() {
        let output = match Command::new(&candidate)
            .args(["models", "--json"])
            .env("NO_COLOR", "1")
            .output()
        {
            Ok(output) => output,
            Err(error) => {
                last_error = error.to_string();
                continue;
            }
        };

        if !output.status.success() {
            last_error = "omp models --json 执行失败".to_string();
            continue;
        }
        return parse_models_json(&String::from_utf8_lossy(&output.stdout));
    }
    Err(last_error)
}

fn parse_custom_models(path: &Path) -> Vec<ModelOption> {
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(YamlValue::Mapping(root)) = serde_yaml::from_str::<YamlValue>(&content) else {
        return Vec::new();
    };
    let providers_key = YamlValue::String("providers".to_string());
    let Some(YamlValue::Mapping(providers)) = root.get(&providers_key) else {
        return Vec::new();
    };

    let mut result = Vec::new();
    for (provider_key, provider_value) in providers {
        let YamlValue::String(provider) = provider_key else {
            continue;
        };
        let YamlValue::Mapping(provider_map) = provider_value else {
            continue;
        };
        let models_key = YamlValue::String("models".to_string());
        let Some(YamlValue::Sequence(models)) = provider_map.get(&models_key) else {
            continue;
        };
        for model_value in models {
            let YamlValue::Mapping(model_map) = model_value else {
                continue;
            };
            let id_key = YamlValue::String("id".to_string());
            let Some(YamlValue::String(id)) = model_map.get(&id_key) else {
                continue;
            };
            let name = match model_map.get(&YamlValue::String("name".to_string())) {
                Some(YamlValue::String(name)) => name.clone(),
                _ => id.clone(),
            };
            if let Some(model) = model_from_parts(
                provider.clone(),
                id.clone(),
                name,
                false,
                Vec::new(),
                vec!["text".to_string()],
            ) {
                result.push(model);
            }
        }
    }
    result
}

fn selector_base(value: &str) -> Option<(String, String)> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.starts_with('@') {
        return None;
    }
    let (base, effort) = trimmed
        .rsplit_once(':')
        .filter(|(_, suffix)| THINKING_LEVELS.contains(suffix))
        .map_or((trimmed, ""), |(base, suffix)| (base, suffix));
    let (provider, id) = base.split_once('/')?;
    if provider.is_empty() || id.is_empty() {
        return None;
    }
    Some((format!("{provider}/{id}"), effort.to_string()))
}

fn add_selector_models(models: &mut Vec<ModelOption>, selectors: impl IntoIterator<Item = String>) {
    let known = models
        .iter()
        .map(|model| model.selector.clone())
        .collect::<HashSet<_>>();
    let mut additions = Vec::new();
    for selector in selectors {
        let Some((base, _)) = selector_base(&selector) else {
            continue;
        };
        if known.contains(&base)
            || additions
                .iter()
                .any(|model: &ModelOption| model.selector == base)
        {
            continue;
        }
        let Some((provider, id)) = base.split_once('/') else {
            continue;
        };
        if let Some(model) = model_from_parts(
            provider.to_string(),
            id.to_string(),
            id.to_string(),
            false,
            Vec::new(),
            vec!["text".to_string()],
        ) {
            additions.push(model);
        }
    }
    models.extend(additions);
}

fn load_snapshot_from_disk() -> Result<ConfigSnapshot, String> {
    let agent = agent_dir()?;
    let config = config_file(&agent);
    let (root, config_exists) = load_yaml(&config)?;
    let model_roles = string_map(match &root {
        YamlValue::Mapping(map) => map.get(YamlValue::String("modelRoles".to_string())),
        _ => None,
    });
    let agent_model_overrides = nested_string_map(&root, &["task", "agentModelOverrides"]);
    let disabled_providers = string_list(match &root {
        YamlValue::Mapping(map) => map.get(YamlValue::String("disabledProviders".to_string())),
        _ => None,
    });

    let mut warnings = Vec::new();
    let agent_files = load_agent_files(&agent, &mut warnings);
    let (mut models, model_source) = match load_omp_models() {
        Ok(models) => (models, "omp models --json".to_string()),
        Err(error) => {
            warnings.push(format!(
                "无法读取 OMP 实时模型列表，已使用配置中的模型: {error}"
            ));
            let custom = ["models.yml", "models.yaml"]
                .iter()
                .map(|name| parse_custom_models(&agent.join(name)))
                .flatten()
                .collect::<Vec<_>>();
            (dedupe_models(custom), "config fallback".to_string())
        }
    };
    let selectors = model_roles
        .values()
        .chain(agent_model_overrides.values())
        .cloned()
        .collect::<Vec<_>>();
    add_selector_models(&mut models, selectors);
    models = dedupe_models(models);

    Ok(ConfigSnapshot {
        config_path: display_path(&config),
        config_exists,
        model_roles,
        agent_model_overrides,
        disabled_providers,
        models,
        model_source,
        agent_files,
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
    fs::rename(&temporary, path)
        .map_err(|error| format!("替换 Agent Markdown 失败: {error}"))?;
    Ok(())
}

#[tauri::command]
fn load_snapshot() -> Result<ConfigSnapshot, String> {
    load_snapshot_from_disk()
}

#[tauri::command]
fn save_snapshot(request: SaveRequest) -> Result<ConfigSnapshot, String> {
    let agent = agent_dir()?;
    let config = config_file(&agent);
    let (mut root, _) = load_yaml(&config)?;

    let mut seen_roles = HashSet::new();
    for custom in &request.custom_roles {
        let role = custom.role.trim();
        if !valid_custom_role(role) {
            return Err(format!(
                "自定义 Role “{role}”无效：仅允许小写字母开头及小写字母、数字、-、_，且不能覆盖 OMP 系统 Role"
            ));
        }
        if custom.selector.trim().is_empty() {
            return Err(format!("自定义 Role “{role}”尚未选择模型"));
        }
        if !seen_roles.insert(role.to_string()) {
            return Err(format!("自定义 Role “{role}”重复"));
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

    let roles = [
        ("smol", request.smol.as_str()),
        ("default", request.default.as_str()),
        ("slow", request.slow.as_str()),
        ("advisor", request.advisor.as_str()),
        ("vision", request.vision.as_str()),
    ];
    for (role, selector) in roles {
        set_nested_string(&mut root, &["modelRoles", role], selector);
    }
    for custom in &request.custom_roles {
        set_nested_string(
            &mut root,
            &["modelRoles", custom.role.trim()],
            &custom.selector,
        );
    }
    set_nested_string(
        &mut root,
        &["task", "agentModelOverrides", "reviewer"],
        &request.reviewer,
    );
    set_nested_string(
        &mut root,
        &["task", "agentModelOverrides", "security-reviewer"],
        &request.security_reviewer,
    );
    set_nested_string_list(
        &mut root,
        &["disabledProviders"],
        &request.disabled_providers,
    );
    write_yaml_atomically(&config, &root)?;
    load_snapshot_from_disk()
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
            "modelRoles:\n  default: old/default\n  keep: keep/model\ntask:\n  agentModelOverrides:\n    keep-agent: keep/agent\notherSetting: preserved\n",
        )
        .expect("test config should be written");

        let previous_home = env::var_os("HOME");
        env::set_var("HOME", &home);
        let result = save_snapshot(SaveRequest {
            smol: "google/smol:low".to_string(),
            default: "openai/default:max".to_string(),
            slow: "anthropic/slow:xhigh".to_string(),
            vision: "openai/vision:xhigh".to_string(),
            advisor: "openai/advisor:high".to_string(),
            custom_roles: vec![
                CustomRoleSave {
                    role: "tester".to_string(),
                    selector: "google/tester:medium".to_string(),
                },
                CustomRoleSave {
                    role: "release-budget".to_string(),
                    selector: "openai/release:high".to_string(),
                },
            ],
            agent_files: vec![AgentFileSave {
                file_name: "release-coordinator.md".to_string(),
                agent_markdown: "---\nname: release-coordinator\ndescription: Release verification agent.\nmodel: \"@slow\"\n---\n\nRun the release verification workflow.\n".to_string(),
            }],
            reviewer: "anthropic/reviewer:xhigh".to_string(),
            security_reviewer: "anthropic/security:xhigh".to_string(),
            disabled_providers: vec!["ollama".to_string()],
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
        assert_eq!(snapshot.disabled_providers, vec!["ollama".to_string()]);
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
        fs::remove_dir_all(home).expect("isolated test home should be removed");
    }
}
