//! 文章输入与分页参数，在进入数据库之前完成字段校验。

use openrust::validation::{Rule, Rules, Validate};

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct ArticleInput {
    pub(crate) title: String,

    pub(crate) body: String,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct ArticlePage {
    #[serde(default = "default_page")]
    pub(crate) page: u64,
    #[serde(default = "default_per_page")]
    pub(crate) per_page: u64,
}

const fn default_page() -> u64 {
    1
}

const fn default_per_page() -> u64 {
    20
}

fn not_blank(value: &str) -> Result<(), validator::ValidationError> {
    if value.trim().is_empty() {
        return Err(validator::ValidationError::new("blank"));
    }
    Ok(())
}

#[cfg(feature = "signing")]
#[derive(serde::Deserialize)]
pub(crate) struct ShareTokenInput {
    pub(crate) token: String,
}

#[cfg(feature = "encryption")]
#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct PrivateNoteInput {
    pub(crate) note: String,
}

impl Validate for ArticleInput {
    fn rules() -> Rules {
        Rules::new()
            .field(
                "title",
                [
                    Rule::required(),
                    Rule::string(),
                    Rule::length(Some(3), Some(160)),
                    Rule::custom(|field| {
                        field
                            .value
                            .and_then(serde_json::Value::as_str)
                            .map_or(Ok(()), not_blank)
                    }),
                ],
            )
            .field(
                "body",
                [
                    Rule::required(),
                    Rule::string(),
                    Rule::length(Some(1), Some(10000)),
                    Rule::custom(|field| {
                        field
                            .value
                            .and_then(serde_json::Value::as_str)
                            .map_or(Ok(()), not_blank)
                    }),
                ],
            )
    }
}

impl Validate for ArticlePage {
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
    }
}

#[cfg(feature = "encryption")]
impl Validate for PrivateNoteInput {
    fn rules() -> Rules {
        Rules::new().field(
            "note",
            [
                Rule::required(),
                Rule::string(),
                Rule::length(Some(1), Some(10000)),
                Rule::custom(|field| {
                    field
                        .value
                        .and_then(serde_json::Value::as_str)
                        .map_or(Ok(()), not_blank)
                }),
            ],
        )
    }
}
