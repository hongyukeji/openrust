//! Eloquent 邮箱验证状态的条件更新，供模型的验证能力委托使用。

use crate::{
    auth::AuthResult,
    contracts::auth::MustVerifyEmail,
    database::{Db, Value, eloquent::Model},
};

/// 仅更新主键、邮箱及原验证状态均匹配的记录。
///
/// 列名由模型映射提供；成功更新后才同步内存属性，保留幂等更新语义。
/// 此操作沿用条件更新，不派发 Eloquent 保存事件。
pub async fn set_verification<M: Model + MustVerifyEmail>(
    model: &mut M,
    verified: bool,
    email_column: &str,
    verified_at_column: &str,
    updated_at_column: &str,
) -> AuthResult<bool> {
    let now = jiff::Timestamp::now()
        .strftime("%Y-%m-%d %H:%M:%S")
        .to_string();
    let value = if verified {
        Value::Text(now.clone())
    } else {
        Value::Null
    };
    let query = Db::connection(model.get_connection_name())?
        .table(model.get_table())
        .r#where(
            model.get_key_name(),
            "=",
            Value::from(model.get_auth_identifier()),
        )
        .where_exact_text(email_column, model.get_email_for_verification());
    let query = if verified {
        query.where_null(verified_at_column)
    } else {
        query.where_not_null(verified_at_column)
    };
    let changed = query
        .update(
            [
                (verified_at_column.into(), value.clone()),
                (updated_at_column.into(), now.clone().into()),
            ]
            .into(),
        )
        .await?
        != 0;
    if changed {
        model.set_attribute(verified_at_column, value);
        model.set_attribute(updated_at_column, now);
    }
    Ok(changed)
}
