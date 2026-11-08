//! 异步文章导入的持久来源与终态收据。

use openrust::database::eloquent::{AttributeCast, EnumCast, ModelAttributes, ModelMetadata};
use openrust::database::sea_orm::entity::prelude::*;
use openrust::{Error, Result};
use serde_json::{Map, Value};
use std::sync::LazyLock;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "example_article_imports")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub import_id: String,
    pub owner_id: i64,
    pub source_key: String,
    pub format: String,
    pub state: String,
    pub error_code: Option<String>,
    pub created_at_ms: i64,
    pub terminal_at_ms: Option<i64>,
    pub source_removed: bool,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// 导入收据在模型和任务中的状态。
#[derive(Clone, Copy)]
pub(crate) enum ImportState {
    Open,
    Completed,
    Failed,
}

impl openrust::support::BackedEnum for ImportState {
    type Backing = String;

    fn try_from_backing(value: String) -> Option<Self> {
        match value.as_str() {
            "open" => Some(Self::Open),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

static IMPORT_CASTS: LazyLock<ModelMetadata> = LazyLock::new(|| {
    let mut metadata = ModelMetadata::new("example_article_imports", "ArticleImport");
    metadata.casts.insert(
        "state".into(),
        AttributeCast::Enum(EnumCast::backed(|state| {
            match state {
                ImportState::Open => "open",
                ImportState::Completed => "completed",
                ImportState::Failed => "failed",
            }
            .to_owned()
        })),
    );
    metadata
});

impl Model {
    /// 将数据库状态解析为任务使用的枚举。
    pub(crate) fn state_case(&self) -> Result<ImportState> {
        let attributes = ModelAttributes::new(Map::from_iter([(
            "state".into(),
            Value::from(self.state.as_str()),
        )]));
        attributes
            .get_attribute_value(&IMPORT_CASTS, "state")?
            .and_then(|value| value.as_enum()?.downcast_ref::<ImportState>().copied())
            .ok_or_else(state_missing)
    }
}

/// 通过同一模型转换为原有字符串列准备状态。
pub(crate) async fn state_for_storage(state: &str) -> Result<String> {
    let attributes = ModelAttributes::default();
    attributes
        .set_attribute(&IMPORT_CASTS, "state", Value::from(state))
        .await?;
    match attributes.raw_attribute("state")? {
        Some(Value::String(value)) => Ok(value),
        _ => Err(state_missing()),
    }
}

fn state_missing() -> Error {
    Error::custom("example_import_state_missing", "导入收据缺少状态")
}
