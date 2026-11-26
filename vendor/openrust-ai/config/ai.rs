use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

pub fn load(env: &Environment) -> Result<Value, ConfigError> {
    let provider = |driver, url_env, default_url, key_env, model_env| {
        let url = env.get_utf8(url_env)?.unwrap_or(default_url);
        Ok::<_, ConfigError>(json!({
            "driver": driver, "url": url, "key_env": key_env,
            "models": {"text": {
                "default": env.get_utf8(model_env)?,
                "default_env": model_env,
                "cheapest": env.get_utf8(&format!("{model_env}_CHEAPEST"))?,
                "smartest": env.get_utf8(&format!("{model_env}_SMARTEST"))?,
            }}
        }))
    };

    let retrieval = |driver, url_env, default_url, key_env, rank_env, embedding_env| {
        Ok::<_, ConfigError>(json!({
            "driver": driver,
            "url": env.get_utf8(url_env)?.unwrap_or(default_url),
            "key_env": key_env,
            "models": {
                "reranking": {"default": env.get_utf8(rank_env)?},
                "embeddings": {"default": env.get_utf8(embedding_env)?},
            },
        }))
    };
    let mut openai = provider(
        "openai",
        "OPENAI_URL",
        "https://api.openai.com/v1",
        "OPENAI_API_KEY",
        "OPENAI_MODEL",
    )?;
    let mut xai = provider(
        "xai",
        "XAI_URL",
        "https://api.x.ai/v1",
        "XAI_API_KEY",
        "XAI_MODEL",
    )?;
    xai["models"]["images"] = json!({"default": env.get_utf8("XAI_IMAGE_MODEL")?});
    let mut groq = provider(
        "groq",
        "GROQ_URL",
        "https://api.groq.com/openai/v1",
        "GROQ_API_KEY",
        "GROQ_MODEL",
    )?;
    groq["models"]["transcription"] = json!({"default": env.get_utf8("GROQ_TRANSCRIPTION_MODEL")?});
    let mut mistral = provider(
        "mistral",
        "MISTRAL_URL",
        "https://api.mistral.ai/v1",
        "MISTRAL_API_KEY",
        "MISTRAL_MODEL",
    )?;
    mistral["models"]["embeddings"] = json!({"default": env.get_utf8("MISTRAL_EMBEDDING_MODEL")?});
    mistral["models"]["audio"] = json!({"default": env.get_utf8("MISTRAL_AUDIO_MODEL")?});
    mistral["models"]["transcription"] =
        json!({"default": env.get_utf8("MISTRAL_TRANSCRIPTION_MODEL")?});
    let mut ollama = provider(
        "ollama",
        "OLLAMA_URL",
        "http://127.0.0.1:11434",
        "OLLAMA_API_KEY",
        "OLLAMA_MODEL",
    )?;
    if env
        .get_utf8("OLLAMA_API_KEY")?
        .is_none_or(|key| key.trim().is_empty())
    {
        ollama["key_env"] = Value::Null;
    }
    ollama["models"]["embeddings"] = json!({"default": env.get_utf8("OLLAMA_EMBEDDING_MODEL")?});
    openai["models"]["embeddings"] = json!({"default": env.get_utf8("OPENAI_EMBEDDING_MODEL")?});

    openai["models"]["images"] = json!({"default": env.get_utf8("OPENAI_IMAGE_MODEL")?});

    openai["models"]["audio"] = json!({"default": env.get_utf8("OPENAI_AUDIO_MODEL")?});
    openai["models"]["transcription"] =
        json!({"default": env.get_utf8("OPENAI_TRANSCRIPTION_MODEL")?});

    openai["models"]["classification"] =
        json!({"default": env.get_utf8("OPENAI_CLASSIFICATION_MODEL")?});

    let mut azure = provider(
        "openai",
        "AZURE_OPENAI_URL",
        "",
        "AZURE_OPENAI_API_KEY",
        "AZURE_OPENAI_DEPLOYMENT",
    )?;
    if let Some(url) = azure["url"].as_str().filter(|url| !url.is_empty()) {
        let url = url.trim_end_matches('/');
        if !url.ends_with("/openai/v1") {
            azure["url"] = json!(format!("{url}/openai/v1"));
        }
    }
    azure["auth"] = json!("api-key");
    azure["default_file_purpose"] = json!("assistants");
    azure["models"]["embeddings"] =
        json!({"default": env.get_utf8("AZURE_OPENAI_EMBEDDING_DEPLOYMENT")?});
    azure["models"]["images"] = json!({"default": env.get_utf8("AZURE_OPENAI_IMAGE_DEPLOYMENT")?});

    let mut openrouter = provider(
        "openai-compatible",
        "OPENROUTER_URL",
        "https://openrouter.ai/api/v1",
        "OPENROUTER_API_KEY",
        "OPENROUTER_MODEL",
    )?;
    openrouter["files"] = json!("openrouter");
    openrouter["models"]["reranking"] =
        json!({"default": env.get_utf8("OPENROUTER_RERANK_MODEL")?});
    openrouter["models"]["embeddings"] =
        json!({"default": env.get_utf8("OPENROUTER_EMBEDDING_MODEL")?});

    openrouter["models"]["classification"] = json!({
        "default": env.get_utf8("OPENROUTER_CLASSIFICATION_MODEL")?,
        "protocol": "openrouter-decisions",
    });

    let mut gemini = provider(
        "gemini",
        "GEMINI_URL",
        "https://generativelanguage.googleapis.com/v1beta",
        "GEMINI_API_KEY",
        "GEMINI_MODEL",
    )?;
    gemini["models"]["audio"] = json!({"default": env.get_utf8("GEMINI_AUDIO_MODEL")?});
    gemini["models"]["transcription"] =
        json!({"default": env.get_utf8("GEMINI_TRANSCRIPTION_MODEL")?});
    gemini["models"]["images"] = json!({"default": env.get_utf8("GEMINI_IMAGE_MODEL")?});
    gemini["models"]["embeddings"] = json!({"default":env.get_utf8("GEMINI_EMBEDDING_MODEL")?});

    gemini["models"]["classification"] =
        json!({"default": env.get_utf8("GEMINI_CLASSIFICATION_MODEL")?});

    const TITLE_ENV: &str = "AI_CONVERSATIONS_GENERATE_TITLE";
    let generate_title = match env.get_utf8(TITLE_ENV)? {
        None | Some("true" | "1") => true,
        Some("false" | "0") => false,
        _ => {
            return Err(ConfigError::InvalidType {
                key: TITLE_ENV.into(),
                expected: "a boolean",
            });
        }
    };

    Ok(json!({
        "default": env.get_utf8("AI_PROVIDER")?,
        "conversations": {
            "connection": env.get_utf8("AI_CONVERSATIONS_CONNECTION")?,
            "max_messages": 100,
            "generate_title": generate_title,
            "attachments": {
                "disk": env.get_utf8("AI_CONVERSATION_ATTACHMENTS_DISK")?.unwrap_or("local"),
            },
            "tables": {
                "conversations": env.get_utf8("AI_CONVERSATIONS_TABLE")?.unwrap_or("agent_conversations"),
                "messages": env.get_utf8("AI_CONVERSATION_MESSAGES_TABLE")?.unwrap_or("agent_conversation_messages"),
            },
        },
        "default_for_images": env.get_utf8("AI_IMAGE_PROVIDER")?,
        "default_for_audio": env.get_utf8("AI_AUDIO_PROVIDER")?,
        "default_for_transcription": env.get_utf8("AI_TRANSCRIPTION_PROVIDER")?,
        "default_for_embeddings": env.get_utf8("AI_EMBEDDINGS_PROVIDER")?,
        "default_for_reranking": env.get_utf8("AI_RERANKING_PROVIDER")?,
        "default_for_classification": env.get_utf8("AI_CLASSIFICATION_PROVIDER")?,
        "default_for_files": env.get_utf8("AI_FILE_PROVIDER")?,
        "default_for_stores": env.get_utf8("AI_STORE_PROVIDER")?,
        "caching": {
            "embeddings": {
                "cache": false,
                "store": env.get_utf8("CACHE_STORE")?.unwrap_or("database"),
                "individually": true,
                "seconds": 2592000
            }
        },
        "providers": {
            "deepseek": provider("openai-compatible", "DEEPSEEK_URL", "https://api.deepseek.com", "DEEPSEEK_API_KEY", "DEEPSEEK_MODEL")?,
            "qwen": provider("openai-compatible", "QWEN_URL", "https://dashscope.aliyuncs.com/compatible-mode/v1", "DASHSCOPE_API_KEY", "QWEN_MODEL")?,
            "tencent": provider("openai-compatible", "TOKENHUB_URL", "https://tokenhub.tencentmaas.com/v1", "TOKENHUB_API_KEY", "TOKENHUB_MODEL")?,
            "qianfan": provider("openai-compatible", "QIANFAN_URL", "https://qianfan.baidubce.com/v2", "QIANFAN_API_KEY", "QIANFAN_MODEL")?,
            "ark": provider("openai-compatible", "ARK_URL", "https://ark.cn-beijing.volces.com/api/v3", "ARK_API_KEY", "ARK_MODEL")?,
            "openai": openai,
            "xai": xai,
            "ollama": ollama,
            "eleven": {
                "driver": "eleven",
                "url": env.get_utf8("ELEVENLABS_URL")?.unwrap_or("https://api.elevenlabs.io/v1"),
                "key_env": "ELEVENLABS_API_KEY",
                "models": {
                    "audio": {"default": env.get_utf8("ELEVENLABS_AUDIO_MODEL")?},
                    "transcription": {"default": env.get_utf8("ELEVENLABS_TRANSCRIPTION_MODEL")?},
                },
            },
            "mistral": mistral,
            "groq": groq,
            "azure": azure,
            "openrouter": openrouter,
            "cohere": retrieval("cohere", "COHERE_URL", "https://api.cohere.com/v2", "COHERE_API_KEY", "COHERE_RERANK_MODEL", "COHERE_EMBEDDING_MODEL")?,
            "jina": retrieval("jina", "JINA_URL", "https://api.jina.ai/v1", "JINA_API_KEY", "JINA_RERANK_MODEL", "JINA_EMBEDDING_MODEL")?,
            "voyageai": retrieval("voyageai", "VOYAGE_URL", "https://api.voyageai.com/v1", "VOYAGE_API_KEY", "VOYAGE_RERANK_MODEL", "VOYAGE_EMBEDDING_MODEL")?,
            "anthropic": provider("anthropic", "ANTHROPIC_URL", "https://api.anthropic.com/v1", "ANTHROPIC_API_KEY", "ANTHROPIC_MODEL")?,
            "gemini": gemini,
            "typesafe": {
                "driver": "system-one",
                "url": env.get_utf8("TYPESAFE_URL")?.unwrap_or("https://api.typesafe.ai/v1"),
                "key_env": "TYPESAFE_API_KEY",
                "models": {"classification": {"default": env.get_utf8("TYPESAFE_CLASSIFICATION_MODEL")?}}
            },
        }
    }))
}
