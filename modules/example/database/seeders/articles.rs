//! 固定隔离主键用于可解释的幂等；示例种子只写 example_* 表。

use crate::modules::example::http::requests::article::ArticleInput;
use crate::modules::example::models::article;
use openrust::db::sea_orm::{ActiveValue::Set, EntityTrait};
use openrust::{Db, Error, Result};

pub(crate) const SEED_ARTICLE_IDS: [i64; 2] = [-7_301_001, -7_301_002];
const SEED_OWNER_ID: i64 = -7_300_001;

fn conflict() -> Error {
    Error::custom(
        "example_seed_conflict",
        "示例种子标识与已有数据冲突，未覆盖任何数据",
    )
}

pub(super) async fn run(database: Db) -> Result<()> {
    database.transaction(move |transaction| {
        Box::pin(async move {
            for (id, title, body) in [
                (
                    SEED_ARTICLE_IDS[0],
                    "种子文章：模块入口",
                    "参考 modules/example 的 Provider、路由与请求校验；这条数据只存在于 example_articles。",
                ),
                (
                    SEED_ARTICLE_IDS[1],
                    "种子文章：业务测试",
                    "参考模块内 TestApp 测试，通过公开框架 API 创建和测试自己的业务模块。",
                ),
            ] {
                if let Some(existing) = article::Entity::find_by_id(id)
                    .one(transaction)
                    .await
                    .map_err(Error::internal)?
                {
                    if existing.owner_id != SEED_OWNER_ID {
                        return Err(conflict());
                    }
                    // 不修复或覆盖种子执行之后的业务修改；删除后显式再 seed 会重建该缺失行。
                    continue;
                }
                let input = openrust::validation::validate(ArticleInput {
                    title: title.into(),
                    body: body.into(),
                })?;
                article::Entity::insert(article::ActiveModel {
                    id: Set(id),
                    owner_id: Set(SEED_OWNER_ID),
                    title: Set(input.title),
                    body: Set(input.body),
                    private_note_ciphertext: Set(None),
                })
                .exec_without_returning(transaction)
                .await
                .map_err(Error::internal)?;
            }
            Ok(())
        })
    })
    .await
}
