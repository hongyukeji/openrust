use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{AiError, AiResult};

/// 当前命名连接的可选模型。能力字段仅反映目录实际返回的信息。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: Option<String>,
    pub input_modalities: Option<Vec<String>>,
    pub output_modalities: Option<Vec<String>>,
    pub configured: bool,
    pub discovered: bool,
}

#[derive(Debug)]
pub struct ModelCatalog {
    pub models: Vec<ModelInfo>,
    pub discovery_error: Option<AiError>,
}

#[derive(Deserialize)]
pub(crate) struct ListedModel {
    pub(crate) id: String,
    #[serde(alias = "display_name")]
    pub(crate) name: Option<String>,
    pub(crate) input_modalities: Option<Vec<String>>,
    pub(crate) output_modalities: Option<Vec<String>>,
}

impl ModelCatalog {
    pub(crate) fn merge(
        listed: AiResult<Vec<ListedModel>>,
        configured: impl Iterator<Item = String>,
    ) -> Self {
        let mut models = BTreeMap::new();
        let discovery_error = match listed {
            Ok(listed) => {
                for model in listed
                    .into_iter()
                    .filter(|model| !model.id.trim().is_empty())
                {
                    models.entry(model.id.clone()).or_insert(ModelInfo {
                        id: model.id,
                        name: model.name,
                        input_modalities: model.input_modalities,
                        output_modalities: model.output_modalities,
                        configured: false,
                        discovered: true,
                    });
                }
                None
            }
            Err(error) => Some(error),
        };
        for id in configured.filter(|id| !id.trim().is_empty()) {
            models
                .entry(id.clone())
                .and_modify(|model| model.configured = true)
                .or_insert(ModelInfo {
                    id,
                    name: None,
                    input_modalities: None,
                    output_modalities: None,
                    configured: true,
                    discovered: false,
                });
        }
        Self {
            models: models.into_values().collect(),
            discovery_error,
        }
    }
}
