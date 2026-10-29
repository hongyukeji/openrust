//! 示例记录 CRUD 的输入类型，覆盖生成器提供的可移植字段类型。

use openrust::validation::Validate;

#[derive(serde::Deserialize, Validate)]
pub(crate) struct ExampleRecordInput {
    #[validate(length(min = 1, max = 160))]
    pub(crate) name: String,
    #[validate(length(max = 10000))]
    pub(crate) description: String,
    #[validate(range(min = -100_000, max = 100_000))]
    pub(crate) priority: i32,
    #[validate(range(min = 0))]
    pub(crate) view_count: i64,
    pub(crate) enabled: bool,
    pub(crate) published_at: Option<openrust::db::sea_orm::prelude::DateTimeUtc>,
    pub(crate) metadata: Option<serde_json::Value>,
    pub(crate) category_id: Option<i64>,
}

#[derive(serde::Deserialize, Validate)]
pub(crate) struct ExampleRecordPage {
    #[serde(default = "default_page")]
    #[validate(range(min = 1, max = 1_000_000))]
    pub(crate) page: u64,
    #[serde(default = "default_per_page")]
    #[validate(range(min = 1, max = 100))]
    pub(crate) per_page: u64,
}

const fn default_page() -> u64 {
    1
}

const fn default_per_page() -> u64 {
    20
}
