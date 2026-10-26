use serde_json::{Map, Value};

pub(super) type Configuration = Map<String, Value>;

// Intrinsic ModelDefinitionSchema fields from OMP. Transport, credentials and
// compaction routing remain local-only; a remote catalog must not redirect them.
const MODEL_FIELDS: &[&str] = &[
    "name",
    "kind",
    "reasoning",
    "thinking",
    "input",
    "imageInputDecoder",
    "imageCountBudget",
    "tokenizer",
    "supportsTools",
    "cost",
    "promptCache",
    "premiumMultiplier",
    "contextWindow",
    "maxContextWindow",
    "maxTokens",
    "omitMaxOutputTokens",
    "preferWebsockets",
];
const EFFORTS: &[&str] = &["minimal", "low", "medium", "high", "xhigh", "max"];

fn one_of(value: &Value, choices: &[&str]) -> bool {
    value.as_str().is_some_and(|value| choices.contains(&value))
}

fn strings(value: &Value, valid: impl Fn(&Value) -> bool) -> bool {
    value
        .as_array()
        .is_some_and(|values| values.iter().all(valid))
}

fn object_fields(value: &Value, valid: impl Fn(&str, &Value) -> bool) -> bool {
    value
        .as_object()
        .is_some_and(|map| map.iter().all(|(key, value)| valid(key, value)))
}

fn nonempty(value: &Value) -> bool {
    value.as_str().is_some_and(|value| !value.is_empty())
}

fn positive_integer(value: &Value) -> bool {
    value
        .as_u64()
        .is_some_and(|value| value > 0 && value <= 9_007_199_254_740_991)
}

fn effort_map(value: &Value) -> bool {
    object_fields(value, |key, value| {
        EFFORTS.contains(&key) && value.is_string()
    })
}

fn thinking(value: &Value) -> bool {
    let Some(map) = value.as_object() else {
        return false;
    };
    map.get("mode").is_some_and(|value| {
        one_of(
            value,
            &[
                "effort",
                "budget",
                "google-level",
                "anthropic-adaptive",
                "anthropic-budget-effort",
            ],
        )
    }) && (map.contains_key("efforts")
        || map.contains_key("levels")
        || (map.contains_key("minLevel") && map.contains_key("maxLevel")))
        && object_fields(value, |key, value| match key {
            "mode" => true,
            "efforts" | "levels" => strings(value, |value| one_of(value, EFFORTS)),
            "defaultLevel" | "minLevel" | "maxLevel" => one_of(value, EFFORTS),
            "effortMap" => effort_map(value),
            "supportsDisplay" | "requiresEffort" => value.is_boolean(),
            _ => false,
        })
}

fn valid_field(key: &str, value: &Value) -> bool {
    match key {
        "name" => nonempty(value),
        "kind" => one_of(
            value,
            &[
                "chat",
                "tiny",
                "image",
                "tts",
                "stt",
                "judge",
                "embedding",
                "rerank",
                "video",
            ],
        ),
        "reasoning" | "supportsTools" | "omitMaxOutputTokens" | "preferWebsockets" => {
            value.is_boolean()
        }
        "thinking" => thinking(value),
        "input" => strings(value, |value| one_of(value, &["text", "image"])),
        "imageInputDecoder" => one_of(value, &["stb"]),
        "imageCountBudget" | "maxContextWindow" => positive_integer(value),
        "contextWindow" | "maxTokens" => value.as_f64().is_some_and(|value| value > 0.0),
        "premiumMultiplier" => value.is_number(),
        "tokenizer" => one_of(
            value,
            &[
                "claude-v3",
                "claude-v47",
                "claude-v5",
                "claude-v5-sonnet",
                "qwen3",
                "deepseek-v3",
                "kimi-k2",
                "glm5",
                "o200k",
            ],
        ),
        "cost" => value.as_object().is_some_and(|map| {
            ["input", "output", "cacheRead", "cacheWrite"]
                .iter()
                .all(|key| map.get(*key).is_some_and(Value::is_number))
                && map.len() == 4
        }),
        "promptCache" => object_fields(value, |key, value| {
            matches!(key, "short" | "long") && value.is_number()
        }),
        _ => false,
    }
}

pub(super) fn validate(configuration: &Configuration) -> Result<(), String> {
    for (key, value) in configuration {
        if !MODEL_FIELDS.contains(&key.as_str()) || !valid_field(key, value) {
            return Err(format!("不支持或无效的 model 参数：{key}"));
        }
    }
    if let (Some(maximum), Some(window)) = (
        configuration
            .get("maxContextWindow")
            .and_then(Value::as_f64),
        configuration.get("contextWindow").and_then(Value::as_f64),
    ) {
        if maximum < window {
            return Err("maxContextWindow 不能小于 contextWindow".to_string());
        }
    }
    Ok(())
}

pub(super) fn validate_api(configuration: &Configuration, api: &str) -> Result<(), String> {
    let Some(kind) = configuration.get("kind").and_then(Value::as_str) else {
        return Ok(());
    };
    let allowed: &[&str] = match api {
        "typesafe" | "openrouter-decisions" | "openai-decisions" | "workers-ai-systemone" => {
            &["judge"]
        }
        "openai-images" | "openrouter-images" => &["image"],
        "xai-tts" | "openai-speech" => &["tts"],
        "openai-transcriptions" => &["stt"],
        "openai-embeddings" => &["embedding"],
        "openrouter-rerank" => &["rerank"],
        "openrouter-video" => &["video"],
        "google-generative-ai"
        | "google-gemini-cli"
        | "openai-responses"
        | "openai-codex-responses" => &["chat", "tiny", "image"],
        _ => &["chat", "tiny"],
    };
    if allowed.contains(&kind) {
        Ok(())
    } else {
        Err(format!("model kind “{kind}”与 API type “{api}”不兼容"))
    }
}

pub(super) fn merge(target: &mut Configuration, source: &Configuration) {
    for (key, value) in source {
        match (target.get_mut(key), value) {
            (Some(Value::Object(target)), Value::Object(source)) => merge(target, source),
            _ => {
                target.insert(key.clone(), value.clone());
            }
        }
    }
}
pub(super) fn apply_yaml(target: &mut serde_yaml::Mapping, source: &Configuration) {
    for (key, value) in source {
        let key = serde_yaml::Value::String(key.clone());
        match (target.get_mut(&key), value) {
            (Some(serde_yaml::Value::Mapping(target)), Value::Object(source)) => {
                apply_yaml(target, source)
            }
            _ => {
                target.insert(
                    key,
                    serde_yaml::to_value(value).expect("JSON metadata is representable as YAML"),
                );
            }
        }
    }
}

pub(super) fn configured(value: &Value) -> Configuration {
    value
        .as_object()
        .map(|map| {
            map.iter()
                .filter(|(key, _)| MODEL_FIELDS.contains(&key.as_str()))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn snapshot(value: &Value) -> Configuration {
    let mut result = configured(value);
    // Only valid model capabilities cross IPC; invalid disk fields still fail save validation.
    result.retain(|key, value| valid_field(key, value));
    result
}

fn token_limit(value: &Value) -> Option<u64> {
    let number = value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse::<f64>().ok())?;
    (number.is_finite()
        && number > 0.0
        && number <= 9_007_199_254_740_991.0
        && number.fract() == 0.0)
        .then_some(number as u64)
}

pub(super) fn discover(value: &Value) -> Result<Configuration, String> {
    let mut result = Configuration::new();
    let metadata = value.get("metadata");
    for source in metadata.into_iter().chain(std::iter::once(value)) {
        if let Some(object) = source.as_object() {
            for key in MODEL_FIELDS {
                if let Some(value) = object.get(*key) {
                    // An array is the legacy UI thinking ladder, not OMP's ThinkingConfig.
                    if *key == "thinking" && value.is_array() {
                        continue;
                    }
                    result.insert((*key).to_string(), value.clone());
                }
            }
        }
    }
    let sources = [value, metadata.unwrap_or(&Value::Null)];
    for source in sources {
        if !result.contains_key("contextWindow") {
            let context = ["max_model_len", "context_length", "context_window"]
                .iter()
                .find_map(|key| source.get(*key).and_then(token_limit))
                .or_else(|| {
                    let limits = source.get("limits")?;
                    let input = token_limit(limits.get("max_input_tokens")?)?;
                    let output = token_limit(limits.get("max_output_tokens")?)?;
                    input
                        .checked_add(output)
                        .filter(|value| *value <= 9_007_199_254_740_991)
                });
            if let Some(context) = context {
                result.insert("contextWindow".to_string(), context.into());
            }
        }
        if !result.contains_key("maxTokens") {
            let output = source
                .get("limits")
                .and_then(|limits| limits.get("max_output_tokens"))
                .and_then(token_limit)
                .or_else(|| {
                    ["max_output_tokens", "max_completion_tokens"]
                        .iter()
                        .find_map(|key| source.get(*key).and_then(token_limit))
                })
                .or_else(|| {
                    source
                        .get("top_provider")
                        .and_then(|provider| provider.get("max_completion_tokens"))
                        .and_then(token_limit)
                })
                .or_else(|| source.get("max_tokens").and_then(token_limit));
            if let Some(output) = output {
                result.insert("maxTokens".to_string(), output.into());
            }
        }
        if !result.contains_key("input") {
            let modalities = source
                .get("input_modalities")
                .or_else(|| {
                    source
                        .get("architecture")
                        .and_then(|value| value.get("input_modalities"))
                })
                .and_then(Value::as_array);
            if let Some(modalities) = modalities {
                let input: Vec<_> = ["text", "image"]
                    .iter()
                    .filter(|kind| {
                        modalities.iter().any(|value| {
                            value
                                .as_str()
                                .is_some_and(|value| value.eq_ignore_ascii_case(kind))
                        })
                    })
                    .map(|kind| Value::String((*kind).to_string()))
                    .collect();
                if !input.is_empty() {
                    result.insert("input".to_string(), input.into());
                }
            }
        }
    }
    validate(&result)?;
    Ok(result)
}

pub(super) fn for_pi(configuration: &Configuration) -> Configuration {
    configuration
        .iter()
        .filter(|(key, _)| {
            matches!(
                key.as_str(),
                "name" | "reasoning" | "input" | "cost" | "contextWindow" | "maxTokens"
            )
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn aliases_preserve_limits_and_reject_invalid_canonical_metadata() {
        let metadata = discover(&json!({
            "limits": {"max_input_tokens": 200000, "max_output_tokens": 32000},
            "context_length": "invalid", "max_output_tokens": -1,
            "input_modalities": ["text", "image"], "reasoning": false,
            "cost": {"input": 0, "output": 1, "cacheRead": 0, "cacheWrite": 0},
            "object": "model", "created": 1
        }))
        .unwrap();
        assert_eq!(metadata["contextWindow"], 232000);
        assert_eq!(metadata["maxTokens"], 32000);
        assert_eq!(metadata["input"], json!(["text", "image"]));
        assert_eq!(metadata["reasoning"], false);
        assert_eq!(metadata["cost"]["input"], 0);
        assert!(!metadata.contains_key("created"));
        assert!(!discover(&json!({"id": "unknown"}))
            .unwrap()
            .contains_key("contextWindow"));
        let strings =
            discover(&json!({"context_length": " 200000 ", "max_output_tokens": "32000"})).unwrap();
        assert_eq!(strings["contextWindow"], 200000);
        assert_eq!(strings["maxTokens"], 32000);
        assert!(validate(json!({"api": "openai-responses"}).as_object().unwrap()).is_err());
        for invalid in [
            json!({"contextWindow": "huge"}),
            json!({"contextWindow": 200, "maxContextWindow": 100}),
            json!({"imageCountBudget": 0}),
            json!({"reasoning": "false"}),
            json!({"contextWindow": 0}),
            json!({"maxTokens": -1}),
        ] {
            assert!(
                discover(&invalid).is_err(),
                "invalid model metadata must not be written"
            );
        }
        assert!(validate_api(json!({"kind": "chat"}).as_object().unwrap(), "xai-tts").is_err());
        assert!(validate_api(
            json!({"kind": "image"}).as_object().unwrap(),
            "anthropic-messages"
        )
        .is_err());
        assert!(validate_api(json!({"kind": "tts"}).as_object().unwrap(), "xai-tts").is_ok());
        assert!(validate_api(
            json!({"kind": "image"}).as_object().unwrap(),
            "openai-responses"
        )
        .is_ok());
    }

    #[test]
    fn nested_metadata_preserves_local_settings_without_exposing_credentials() {
        let mut existing: serde_yaml::Mapping = serde_yaml::from_value(
            serde_yaml::to_value(json!({
                "input": ["text"], "reasoning": false,
                "headers": {"Authorization": "local-secret"},
                "baseUrl": "https://local.example/v1",
                "remoteCompaction": {"enabled": false, "endpoint": "https://local.example/compact"},
                "compat": {"supportsStore": true, "extraBody": {"local": 1, "keep": false}},
                "thinking": {"mode": "budget", "efforts": ["high"], "supportsDisplay": false}
            }))
            .unwrap(),
        )
        .unwrap();
        let update = discover(&json!({
            "contextWindow": 200000, "maxTokens": 64000,
            "compat": {"supportsStore": false, "extraBody": {"local": 2}},
            "baseUrl": "https://attacker.example/v1", "headers": {"Authorization": "attacker"},
            "remoteCompaction": {"enabled": true, "endpoint": "https://attacker.example/compact"},
            "compactionModel": "attacker/model", "contextPromotionTarget": "attacker/model",
            "thinking": {"mode": "budget", "efforts": ["high", "max"], "effortMap": {"max": "highest"}}
        })).unwrap();
        apply_yaml(&mut existing, &update);
        let merged = serde_json::to_value(&existing).unwrap();
        assert_eq!(
            merged["compat"]["extraBody"],
            json!({"local": 1, "keep": false})
        );
        assert_eq!(merged["baseUrl"], "https://local.example/v1");
        assert_eq!(merged["remoteCompaction"]["enabled"], false);
        assert!(!merged.as_object().unwrap().contains_key("compactionModel"));
        assert_eq!(merged["thinking"]["mode"], "budget");
        assert_eq!(merged["thinking"]["supportsDisplay"], false);
        assert_eq!(merged["reasoning"], false);
        assert_eq!(merged["input"], json!(["text"]));
        assert_eq!(merged["headers"]["Authorization"], "local-secret");
        let snapshot = snapshot(&merged);
        assert!(!snapshot.contains_key("headers"));
        assert!(!snapshot.contains_key("compat"));
        assert!(!snapshot.contains_key("baseUrl"));
        assert!(!snapshot.contains_key("remoteCompaction"));
        assert_eq!(snapshot["contextWindow"], 200000);
        assert_eq!(snapshot["thinking"]["effortMap"]["max"], "highest");
    }
}
