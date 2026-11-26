use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

pub fn load(env: &Environment) -> Result<Value, ConfigError> {
    let provider = |driver, url_env, default_url, key_env, model_env| {
        let url = env.get_utf8(url_env)?.unwrap_or(default_url);
        Ok::<_, ConfigError>(json!({
            "driver": driver, "url": url, "key_env": key_env,
            "models": {"text": {"default": env.get_utf8(model_env)?, "default_env": model_env}}
        }))
    };

    let mut openai = provider(
        "openai",
        "OPENAI_URL",
        "https://api.openai.com/v1",
        "OPENAI_API_KEY",
        "OPENAI_MODEL",
    )?;
    openai["models"]["embeddings"] = json!({"default": env.get_utf8("OPENAI_EMBEDDING_MODEL")?});

    Ok(json!({
        "default": env.get_utf8("AI_PROVIDER")?,
        "default_for_images": null,
        "default_for_audio": null,
        "default_for_transcription": null,
        "default_for_embeddings": env.get_utf8("AI_EMBEDDINGS_PROVIDER")?,
        "default_for_reranking": null,
        "default_for_classification": null,
        "caching": {
            "embeddings": {
                "cache": false,
                "store": env.get_utf8("CACHE_STORE")?.unwrap_or("database"),
                "individually": true
            }
        },
        "providers": {
            "deepseek": provider("openai-compatible", "DEEPSEEK_URL", "https://api.deepseek.com", "DEEPSEEK_API_KEY", "DEEPSEEK_MODEL")?,
            "qwen": provider("openai-compatible", "QWEN_URL", "https://dashscope.aliyuncs.com/compatible-mode/v1", "DASHSCOPE_API_KEY", "QWEN_MODEL")?,
            "tencent": provider("openai-compatible", "TOKENHUB_URL", "https://tokenhub.tencentmaas.com/v1", "TOKENHUB_API_KEY", "TOKENHUB_MODEL")?,
            "qianfan": provider("openai-compatible", "QIANFAN_URL", "https://qianfan.baidubce.com/v2", "QIANFAN_API_KEY", "QIANFAN_MODEL")?,
            "ark": provider("openai-compatible", "ARK_URL", "https://ark.cn-beijing.volces.com/api/v3", "ARK_API_KEY", "ARK_MODEL")?,
            "openai": openai,
            "anthropic": provider("anthropic", "ANTHROPIC_URL", "https://api.anthropic.com/v1", "ANTHROPIC_API_KEY", "ANTHROPIC_MODEL")?,
            "gemini": provider("gemini", "GEMINI_URL", "https://generativelanguage.googleapis.com/v1beta", "GEMINI_API_KEY", "GEMINI_MODEL")?
        }
    }))
}
