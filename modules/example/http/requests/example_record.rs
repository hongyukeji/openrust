//! 示例记录 CRUD 的输入类型，覆盖生成器提供的可移植字段类型。

use openrust::validation::{Rule, Rules, Validate};

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct ExampleRecordInput {
    pub(crate) name: String,

    pub(crate) description: String,

    pub(crate) priority: i32,

    pub(crate) view_count: i64,
    pub(crate) enabled: bool,
    pub(crate) published_at: Option<openrust::database::sea_orm::prelude::DateTimeUtc>,
    pub(crate) metadata: Option<serde_json::Value>,
    pub(crate) category_id: Option<i64>,
}

#[derive(serde::Deserialize)]
pub(crate) struct ExampleRecordPreviewOptions {
    #[serde(default)]
    pub(crate) discard: bool,
    #[serde(default)]
    pub(crate) include_owner_records: bool,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct ExampleRecordPage {
    #[serde(default = "default_page")]
    pub(crate) page: u64,
    #[serde(default = "default_per_page")]
    pub(crate) per_page: u64,
    #[serde(default)]
    pub(crate) metadata_contains: Option<String>,
}

const fn default_page() -> u64 {
    1
}

const fn default_per_page() -> u64 {
    20
}

impl Validate for ExampleRecordInput {
    fn rules() -> Rules {
        Rules::new()
            .field(
                "name",
                [
                    Rule::required(),
                    Rule::string(),
                    Rule::length(Some(1), Some(160)),
                ],
            )
            .field(
                "description",
                [
                    Rule::present(),
                    Rule::string(),
                    Rule::length(None, Some(10000)),
                ],
            )
            .field(
                "priority",
                [
                    Rule::required(),
                    Rule::integer(),
                    Rule::range(Some(-100_000), Some(100_000)),
                ],
            )
            .field(
                "view_count",
                [
                    Rule::required(),
                    Rule::integer(),
                    Rule::range(Some(0), None),
                ],
            )
            .field("enabled", [Rule::required(), Rule::boolean()])
            .field("category_id", [Rule::nullable(), Rule::integer()])
    }
}

impl Validate for ExampleRecordPage {
    fn rules() -> Rules {
        Rules::new()
            .field(
                "page",
                [Rule::integer(), Rule::range(Some(1), Some(1_000_000))],
            )
            .field(
                "per_page",
                [Rule::integer(), Rule::range(Some(1), Some(100))],
            )
            .field(
                "metadata_contains",
                [
                    Rule::nullable(),
                    Rule::string(),
                    Rule::length(None, Some(4096)),
                ],
            )
    }
}
