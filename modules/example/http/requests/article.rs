//! 文章输入与分页参数，在进入数据库之前完成字段校验。

use openrust::validation::Validate;

#[derive(serde::Deserialize, Validate)]
pub(crate) struct ArticleInput {
    #[validate(length(min = 3, max = 160), custom(function = "not_blank"))]
    pub(crate) title: String,
    #[validate(length(min = 1, max = 10000), custom(function = "not_blank"))]
    pub(crate) body: String,
}

#[derive(serde::Deserialize, Validate)]
pub(crate) struct ArticlePage {
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
#[derive(serde::Deserialize, Validate)]
pub(crate) struct PrivateNoteInput {
    #[validate(length(min = 1, max = 10000), custom(function = "not_blank"))]
    pub(crate) note: String,
}
