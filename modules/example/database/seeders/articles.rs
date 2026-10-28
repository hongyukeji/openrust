//! 固定隔离身份和主键用于可解释的幂等；已有内容一律保留。

use crate::modules::example::http::requests::article::ArticleInput;
use crate::modules::example::models::article;
use openrust::db::sea_orm::{
    ActiveValue::Set, ConnectionTrait, DatabaseTransaction, DbBackend, EntityTrait, Statement,
};
use openrust::{Db, Error, Result};

pub(crate) const SEED_EMAIL: &str = "example-seed@seed.invalid";
pub(crate) const SEED_ARTICLE_IDS: [i64; 2] = [-7_301_001, -7_301_002];
// 不属于任何支持的密码哈希格式；即使误清除禁用标志也不是默认密码。
const DISABLED_HASH: &str = "!openrust-example-seed-disabled";

fn conflict() -> Error {
    Error::custom(
        "example_seed_conflict",
        "示例种子标识与已有数据冲突，未覆盖任何数据",
    )
}

pub(super) async fn run(database: Db) -> Result<()> {
    let seed_db = database.clone();
    database.transaction(move |transaction| {
        Box::pin(async move {
            let owner = disabled_owner(&seed_db, transaction).await?;
            for (id, title, body) in [
                (SEED_ARTICLE_IDS[0], "种子文章：模块入口", "参考 modules/example 的 Provider、路由与请求校验；这条种子不授予任何账户权限。"),
                (SEED_ARTICLE_IDS[1], "种子文章：业务测试", "参考模块内 TestApp 测试，通过公开框架 API 创建和测试自己的业务模块。"),
            ] {
                if let Some(existing) = article::Entity::find_by_id(id).one(transaction).await.map_err(Error::internal)? {
                    if existing.owner_id != owner { return Err(conflict()); }
                    // 不修复或覆盖种子执行之后的业务修改；删除后显式再 seed 会重建该缺失行。
                    continue;
                }
                let input = openrust::validation::validate(ArticleInput { title:title.into(), body:body.into() })?;
                article::Entity::insert(article::ActiveModel {
                    id:Set(id), owner_id:Set(owner), title:Set(input.title), body:Set(input.body),
                    private_note_ciphertext:Set(None),
                }).exec_without_returning(transaction).await.map_err(Error::internal)?;
            }
            Ok(())
        })
    }).await
}

async fn disabled_owner(database: &Db, transaction: &DatabaseTransaction) -> Result<i64> {
    let backend = database.backend()?;
    let users = database.quoted_table_name("users")?;
    let insert = match backend {
        DbBackend::Postgres => format!(
            "INSERT INTO {users} (email,password_hash,disabled_at,created_at,updated_at) VALUES ($1,$2,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP) ON CONFLICT (email) DO NOTHING"
        ),
        DbBackend::MySql => format!(
            "INSERT INTO {users} (email,password_hash,disabled_at,created_at,updated_at) VALUES (?,?,CURRENT_TIMESTAMP(6),CURRENT_TIMESTAMP(6),CURRENT_TIMESTAMP(6)) ON DUPLICATE KEY UPDATE id=id"
        ),
        _ => return Err(conflict()),
    };
    transaction
        .execute_raw(Statement::from_sql_and_values(
            backend,
            insert,
            [SEED_EMAIL.into(), DISABLED_HASH.into()],
        ))
        .await
        .map_err(Error::internal)?;
    let (email_parameter, hash_parameter) = if backend == DbBackend::Postgres {
        ("$1", "$2")
    } else {
        ("?", "?")
    };
    // 同一身份行的锁将并发 seed 串行化；只接受仍保持禁用和未验证的种子身份。
    let query = format!(
        "SELECT id FROM {users} WHERE email = {email_parameter} AND password_hash = {hash_parameter} AND disabled_at IS NOT NULL AND email_verified_at IS NULL FOR UPDATE"
    );
    let row = transaction
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            query,
            [SEED_EMAIL.into(), DISABLED_HASH.into()],
        ))
        .await
        .map_err(Error::internal)?
        .ok_or_else(conflict)?;
    row.try_get("", "id").map_err(Error::internal)
}
