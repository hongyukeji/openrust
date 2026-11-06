//! SQLite验证码的短写事务与精确UTC判定；Argon2校验仍在此事务之外。
use super::{EncodedPasswordHash, Error, Result, VerificationChallenge, VerificationCodePolicy};
use crate::database::dialect::utc_value;
use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, Statement};

async fn finish<T>(transaction: DatabaseTransaction, outcome: Result<T>) -> Result<T> {
    match outcome {
        Ok(value) => {
            transaction.commit().await.map_err(Error::internal)?;
            Ok(value)
        }
        Err(error) => {
            transaction.rollback().await.map_err(Error::internal)?;
            Err(error)
        }
    }
}

pub(super) async fn issue(
    connection: &DatabaseConnection,
    table: &str,
    key: &str,
    challenge: &str,
    hash: &EncodedPasswordHash,
    policy: VerificationCodePolicy,
) -> Result<()> {
    policy.validate()?;
    let transaction = crate::database::begin_for_update(connection).await?;
    let outcome = async {
        let row = transaction.query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite,
            format!("SELECT challenge_id,resend_after_at FROM {table} WHERE key_hash=?"), [key.to_owned().into()]))
            .await.map_err(Error::internal)?;
        let now = Utc::now();
        if let Some(row) = row {
            let previous: String = row.try_get("", "challenge_id").map_err(Error::internal)?;
            if previous == challenge { return Ok(()); }
            let resend: DateTime<Utc> = row.try_get("", "resend_after_at").map_err(Error::internal)?;
            let wait = resend.signed_duration_since(now).num_microseconds().ok_or_else(|| Error::BadRequest("验证码冷却时间超出范围".into()))?;
            if wait > 0 { return Err(Error::rate_limited(u64::try_from((wait - 1) / 1_000_000 + 1).map_err(Error::internal)?)); }
        }
        let expiry = now.checked_add_signed(chrono::Duration::from_std(policy.lifetime()).map_err(Error::internal)?)
            .ok_or_else(|| Error::BadRequest("验证码期限超出范围".into()))?;
        let resend = now.checked_add_signed(chrono::Duration::from_std(policy.resend_after()).map_err(Error::internal)?)
            .ok_or_else(|| Error::BadRequest("验证码冷却期限超出范围".into()))?;
        transaction.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite,
            format!("INSERT INTO {table} (key_hash,challenge_id,code_hash,code_digits,attempt_count,max_attempts,issued_at,expires_at,resend_after_at,consumed_at) VALUES (?,?,?,?,0,?,?,?,?,NULL) ON CONFLICT(key_hash) DO UPDATE SET challenge_id=excluded.challenge_id,code_hash=excluded.code_hash,code_digits=excluded.code_digits,attempt_count=0,max_attempts=excluded.max_attempts,issued_at=excluded.issued_at,expires_at=excluded.expires_at,resend_after_at=excluded.resend_after_at,consumed_at=NULL"),
            [key.to_owned().into(),challenge.to_owned().into(),hash.as_str().to_owned().into(),i32::from(policy.digits()).into(),i32::try_from(policy.max_attempts()).map_err(Error::internal)?.into(),utc_value(DbBackend::Sqlite,now),utc_value(DbBackend::Sqlite,expiry),utc_value(DbBackend::Sqlite,resend)]))
            .await.map_err(Error::internal)?;
        Ok(())
    }.await;
    finish(transaction, outcome).await
}

pub(super) async fn load_active(
    connection: &DatabaseConnection,
    table: &str,
    key: &str,
) -> Result<Option<VerificationChallenge>> {
    let row=connection.query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite,
        format!("SELECT challenge_id,code_hash,code_digits,expires_at FROM {table} WHERE key_hash=? AND consumed_at IS NULL AND attempt_count<max_attempts"),[key.to_owned().into()]))
        .await.map_err(Error::internal)?;
    let Some(row) = row else { return Ok(None) };
    let expiry: DateTime<Utc> = row.try_get("", "expires_at").map_err(Error::internal)?;
    if expiry <= Utc::now() {
        return Ok(None);
    }
    let challenge: String = row.try_get("", "challenge_id").map_err(Error::internal)?;
    let hash: String = row.try_get("", "code_hash").map_err(Error::internal)?;
    let digits: i32 = row.try_get("", "code_digits").map_err(Error::internal)?;
    Ok(Some(VerificationChallenge::new(
        challenge,
        EncodedPasswordHash::from_encoded(hash),
        u8::try_from(digits).map_err(Error::internal)?,
    )?))
}

pub(super) async fn finish_attempt(
    connection: &DatabaseConnection,
    table: &str,
    key: &str,
    challenge: &str,
    matches: bool,
) -> Result<bool> {
    let transaction = crate::database::begin_for_update(connection).await?;
    let outcome=async{
        let row=transaction.query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite,
            format!("SELECT expires_at FROM {table} WHERE key_hash=? AND challenge_id=? AND consumed_at IS NULL AND attempt_count<max_attempts"),[key.to_owned().into(),challenge.to_owned().into()]))
            .await.map_err(Error::internal)?;
        let Some(row)=row else{return Ok(false)};
        let expiry:DateTime<Utc>=row.try_get("","expires_at").map_err(Error::internal)?;
        let now=Utc::now();
        if expiry<=now{return Ok(false)}
        let consumed=if matches{utc_value(DbBackend::Sqlite,now)}else{sea_orm::Value::String(None)};
        // 写所有权覆盖了expiry读取与此CAS；旧challenge及耗尽的尝试均不能覆盖新挑战。
        let written=transaction.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite,
            format!("UPDATE {table} SET attempt_count=attempt_count+?,consumed_at=? WHERE key_hash=? AND challenge_id=? AND consumed_at IS NULL AND attempt_count<max_attempts"),
            [i32::from(!matches).into(),consumed,key.to_owned().into(),challenge.to_owned().into()]))
            .await.map_err(Error::internal)?;
        Ok(matches && written.rows_affected()>0)
    }.await;
    finish(transaction, outcome).await
}

pub(super) async fn prune(connection: &DatabaseConnection, table: &str) -> Result<()> {
    let transaction = crate::database::begin_for_update(connection).await?;
    let outcome = async {
        // 按主键游标分页，既不依赖日期文本顺序，也不依赖SQLite有限的timezone解析。
        // 每页最多512行、每轮最多删除512行；大量未来项不会让后面的过期项永久饥饿。
        let mut cursor: Option<String> = None;
        let mut removed = 0_u16;
        loop {
            let statement = if let Some(cursor) = &cursor {
                Statement::from_sql_and_values(DbBackend::Sqlite,
                    format!("SELECT key_hash,expires_at FROM {table} WHERE key_hash>? ORDER BY key_hash LIMIT 512"),
                    [cursor.clone().into()])
            } else {
                Statement::from_string(DbBackend::Sqlite,
                    format!("SELECT key_hash,expires_at FROM {table} ORDER BY key_hash LIMIT 512"))
            };
            let rows = transaction.query_all_raw(statement).await.map_err(Error::internal)?;
            if rows.is_empty() { break; }
            let now = Utc::now();
            for row in rows {
                let key: String = row.try_get("", "key_hash").map_err(Error::internal)?;
                let expiry: DateTime<Utc> = row.try_get("", "expires_at").map_err(Error::internal)?;
                if expiry <= now {
                    transaction.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite,
                        format!("DELETE FROM {table} WHERE key_hash=?"), [key.clone().into()]))
                        .await.map_err(Error::internal)?;
                    removed += 1;
                    if removed == 512 { return Ok(()); }
                }
                cursor = Some(key);
            }
        }
        Ok(())
    }.await;
    finish(transaction, outcome).await
}

#[cfg(all(test, feature = "db-sqlite"))]
mod tests;
