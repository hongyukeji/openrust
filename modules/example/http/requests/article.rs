//! 文章输入，在进入数据库之前完成字段校验。

use openrust::validation::{Rule, Rules, Validate};

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct ArticleInput {
    pub(crate) title: String,

    pub(crate) body: String,
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
