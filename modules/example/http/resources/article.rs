//! 文章 API 的公开业务字段；所有者 ID 不作为响应正文泄露。

use crate::modules::example::models::article;
use openrust::db::sea_orm::QueryResult;
use openrust::{Error, Result};

#[derive(serde::Serialize)]
pub(crate) struct ArticleResource {
    pub(crate) id: i64,
    pub(crate) title: String,
    pub(crate) body: String,
}

impl From<article::Model> for ArticleResource {
    fn from(model: article::Model) -> Self {
        Self {
            id: model.id,
            title: model.title,
            body: model.body,
        }
    }
}

impl ArticleResource {
    pub(crate) fn from_query_row(row: &QueryResult) -> Result<Self> {
        Ok(Self {
            id: row.try_get("", "id").map_err(Error::internal)?,
            title: row.try_get("", "title").map_err(Error::internal)?,
            body: row.try_get("", "body").map_err(Error::internal)?,
        })
    }
}

#[cfg(feature = "signing")]
#[derive(serde::Serialize)]
pub(crate) struct ArticleShare {
    pub(crate) token: String,
}

#[cfg(feature = "encryption")]
#[derive(serde::Serialize)]
pub(crate) struct PrivateNoteResource {
    pub(crate) note: String,
}
