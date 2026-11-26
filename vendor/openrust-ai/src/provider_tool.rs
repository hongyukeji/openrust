use serde::{Deserialize, Serialize};

use crate::ProviderOptions;

/// 由模型供应商执行的工具。按连接通过 `Agent::provider_options` 启用。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderTool {
    WebSearch {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        allowed_domains: Vec<String>,
    },
    #[serde(rename = "code_interpreter")]
    CodeExecution,
    FileSearch {
        vector_store_ids: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_num_results: Option<u16>,
    },
}

impl ProviderTool {
    pub fn options(tools: impl IntoIterator<Item = Self>) -> ProviderOptions {
        let tools: Vec<_> = tools.into_iter().collect();
        if tools.is_empty() {
            return ProviderOptions::new();
        }
        // 枚举字段均为 JSON 原生类型。
        let tools = serde_json::to_value(tools).expect("供应商工具可序列化");
        ProviderOptions::from_iter([("tools".into(), tools)])
    }

    pub(crate) fn is_valid(&self) -> bool {
        let nonblank = |value: &String| !value.trim().is_empty() && value.trim() == value;
        match self {
            Self::WebSearch { allowed_domains } => {
                allowed_domains.len() <= 5 && allowed_domains.iter().all(nonblank)
            }
            Self::CodeExecution => true,
            Self::FileSearch {
                vector_store_ids,
                max_num_results,
            } => {
                !vector_store_ids.is_empty()
                    && vector_store_ids.iter().all(nonblank)
                    && *max_num_results != Some(0)
            }
        }
    }
}
