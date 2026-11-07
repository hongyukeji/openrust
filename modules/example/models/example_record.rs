//! 字段类型演示实体，使用专属的 `example_records` 表。

use openrust::{
    Db, Error, Result as ModelResult,
    database::{
        eloquent::{
            HasRelationships, HydrateModel, ModelAttributes, ModelMetadata, ModelRelations,
            SaveModel,
        },
        sea_orm::{self, entity::prelude::*},
    },
};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "example_records")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub owner_id: i64,
    pub name: String,
    pub description: String,
    pub priority: i32,
    pub view_count: i64,
    pub enabled: bool,
    pub published_at: Option<DateTimeUtc>,
    pub metadata: Option<Json>,
    pub category_id: Option<i64>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

impl Model {
    /// 以当前业务字段建立独立预览快照，不改变实体或数据库记录。
    pub(crate) fn attribute_snapshot(
        &self,
    ) -> openrust::Result<openrust::database::eloquent::ModelAttributes> {
        let metadata = Self::metadata_snapshot_value(self.metadata.as_ref())?;
        Ok(openrust::database::eloquent::ModelAttributes::new(
            serde_json::Map::from_iter([
                ("name".into(), serde_json::json!(self.name)),
                ("description".into(), serde_json::json!(self.description)),
                ("priority".into(), serde_json::json!(self.priority)),
                ("view_count".into(), serde_json::json!(self.view_count)),
                ("enabled".into(), serde_json::json!(self.enabled)),
                (
                    "published_at".into(),
                    serde_json::to_value(self.published_at).map_err(openrust::Error::internal)?,
                ),
                ("metadata".into(), metadata),
                ("category_id".into(), serde_json::json!(self.category_id)),
            ]),
        ))
    }
    // 和现有SeaORM更新共享serde JSON值投影，保留整double的.0及原键顺序。
    pub(crate) fn metadata_snapshot_value(
        value: Option<&Json>,
    ) -> openrust::Result<serde_json::Value> {
        value
            .map(serde_json::to_string)
            .transpose()
            .map_err(openrust::Error::internal)
            .map(|text| text.map_or(serde_json::Value::Null, serde_json::Value::String))
    }
    pub(crate) fn attribute_metadata() -> openrust::database::eloquent::ModelMetadata {
        use openrust::database::eloquent::AttributeCast;
        let mut metadata = ModelMetadata::new(
            "example_records",
            "modules::example::models::example_record::Model",
        );
        metadata.timestamps = false;
        metadata.fillable(
            [
                "name",
                "description",
                "priority",
                "view_count",
                "enabled",
                "published_at",
                "metadata",
                "category_id",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        );
        metadata.casts = [
            ("priority".into(), AttributeCast::Integer),
            ("view_count".into(), AttributeCast::Integer),
            ("enabled".into(), AttributeCast::Boolean),
            ("metadata".into(), AttributeCast::Array),
        ]
        .into_iter()
        .collect();
        metadata
    }
}

/// 将示例实体的业务属性交给框架保存，保留列类型和原始基线。
pub(crate) struct RecordForSave {
    pub(crate) metadata: ModelMetadata,
    pub(crate) attributes: ModelAttributes,
    relations: ModelRelations,
    exists: bool,
}

impl RecordForSave {
    pub(crate) fn new(owner: i64) -> Self {
        Self {
            metadata: Model::attribute_metadata(),
            attributes: ModelAttributes::new(Map::from_iter([(
                "owner_id".into(),
                Value::from(owner),
            )])),
            relations: ModelRelations::default(),
            exists: false,
        }
    }

    pub(crate) fn from_existing(model: &Model) -> ModelResult<Self> {
        let attributes = model.attribute_snapshot()?;
        attributes.set_raw_attribute("id", Value::from(model.id))?;
        attributes.sync_original()?;
        Ok(Self {
            metadata: Model::attribute_metadata(),
            attributes,
            relations: ModelRelations::default(),
            exists: true,
        })
    }

    pub(crate) fn id(&self) -> ModelResult<i64> {
        self.attributes
            .raw_attribute("id")?
            .and_then(|value| value.as_i64())
            .ok_or_else(|| Error::custom("example_record_id_invalid", "示例记录主键不是预期整数"))
    }
}

impl HydrateModel for RecordForSave {
    fn hydrate(&self, raw: Map<String, Value>, connection: Option<&str>) -> ModelResult<Self> {
        let mut metadata = self.metadata.clone();
        metadata.connection = connection.map(str::to_owned);
        Ok(Self {
            metadata,
            attributes: ModelAttributes::new(raw),
            relations: ModelRelations::default(),
            exists: true,
        })
    }
}

impl HasRelationships for RecordForSave {
    fn relations(&self) -> &ModelRelations {
        &self.relations
    }

    fn relations_mut(&mut self) -> &mut ModelRelations {
        &mut self.relations
    }
}

impl SaveModel for RecordForSave {
    fn metadata(&self) -> &ModelMetadata {
        &self.metadata
    }

    fn attributes(&self) -> &ModelAttributes {
        &self.attributes
    }

    fn attributes_mut(&mut self) -> &mut ModelAttributes {
        &mut self.attributes
    }

    fn exists(&self) -> bool {
        self.exists
    }

    fn mark_created(&mut self) {
        self.exists = true;
    }

    fn bind_attribute(&self, name: &str, value: &Value) -> ModelResult<sea_orm::Value> {
        match name {
            "id" | "owner_id" | "view_count" => bind::<i64>(value),
            "name" | "description" => bind::<String>(value),
            "priority" => bind::<i32>(value),
            "enabled" => bind::<bool>(value),
            "published_at" => bind::<Option<DateTimeUtc>>(value),
            "category_id" => bind::<Option<i64>>(value),
            "metadata" => match value {
                Value::Null => Ok(Option::<Json>::None.into()),
                Value::String(text) => {
                    Ok(Some(serde_json::from_str::<Json>(text).map_err(Error::internal)?).into())
                }
                _ => Err(Error::custom(
                    "example_record_metadata_invalid",
                    "示例记录 metadata 原始值无效",
                )),
            },
            _ => Err(Error::custom(
                "example_record_column_invalid",
                "无法绑定示例记录字段",
            )),
        }
    }
}

// 记录迁移和 SeaORM 实体使用固定物理表名。
pub(crate) fn record_query_db(db: &Db) -> ModelResult<Db> {
    let query_db = Db::default();
    query_db.initialize(db.primary_connection()?)?;
    Ok(query_db)
}

fn bind<T: DeserializeOwned + Into<sea_orm::Value>>(value: &Value) -> ModelResult<sea_orm::Value> {
    T::deserialize(value)
        .map(Into::into)
        .map_err(Error::internal)
}
