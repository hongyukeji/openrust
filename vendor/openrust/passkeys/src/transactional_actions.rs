//! 在调用方的用户写锁事务内复用原生WebAuthn、模型与事件。
use crate::{
    AuthenticationOptions, PasskeyRecord, Passkeys, RegistrationOptions,
    actions::{DeletePasskey, StorePasskey, VerifyPasskey},
    internal, invalid,
};
use openrust::{
    contracts::auth::Authenticatable,
    database::{
        Transaction, Value,
        eloquent::{Attributes, Model},
    },
    http::HttpError,
};
fn account(
    service: &Passkeys,
    user: &dyn Authenticatable,
    transaction: &Transaction,
) -> Result<crate::passkeys::Account, HttpError> {
    service.validate_user_transaction(user, transaction)?;
    service.account(user)
}

impl StorePasskey {
    /*
     * 调用方持有用户写锁，负责提交或回滚，并在回滚后丢弃或刷新返回模型。
     * 保留原生WebAuthn校验、所选模型和事件；不启动嵌套事务或跨连接写入。
     */
    pub async fn call_in_transaction(
        &self,
        user: &dyn Authenticatable,
        transaction: &mut Transaction,
        name: &str,
        value: serde_json::Value,
        options: &RegistrationOptions,
    ) -> Result<PasskeyRecord, HttpError> {
        let account = account(&self.0, user, transaction)?;
        let credential = self
            .0
            .webauthn()
            .map_err(internal)?
            .register(value, options)
            .map_err(|_| invalid("Unable to register passkey. Please try again."))?;
        if credential.user_handle() != account.handle {
            return Err(invalid("Unable to register passkey. Please try again."));
        }
        let id = credential.credential_id();
        let store = self.0.passkey_store();
        if !store
            .records_in_transaction(transaction, "credential_id", id.clone().into())
            .await
            .map_err(internal)?
            .is_empty()
        {
            return Err(invalid("Unable to register this passkey."));
        }
        let attributes: Attributes = [
            ("name".into(), name.into()),
            ("credential_id".into(), id.into()),
            (
                "credential".into(),
                Value::Json(serde_json::to_value(credential).map_err(internal)?),
            ),
        ]
        .into();
        let result = store
            .create_in_transaction(transaction, attributes, account.key)
            .await;
        let record = match result {
            Ok(record) => record,
            Err(error) => {
                if matches!(&error,openrust::database::DatabaseError::Driver{source,..} if source.as_database_error().is_some_and(|error|error.is_unique_violation()))
                {
                    return Err(invalid("Unable to register this passkey."));
                }
                return Err(internal(error));
            }
        };
        crate::events::registered(user, &record).await?;
        Ok(record)
    }
}
impl VerifyPasskey {
    /*
     * 验证已锁定的当前用户，凭据计数器与调用方的凭据操作一起提交。
     * 复用传入用户派发事件，避免用户事务内再次通过其他连接查询用户。
     */
    pub async fn call_in_transaction(
        &self,
        user: &dyn Authenticatable,
        transaction: &mut Transaction,
        value: serde_json::Value,
        options: &AuthenticationOptions,
    ) -> Result<PasskeyRecord, HttpError> {
        self.verify_transaction(user, transaction, value, options, false)
            .await
    }
    /*
     * 无会话登录仍强制校验发现式凭据的userHandle，不能因已解析用户就降级。
     */
    pub async fn call_discoverable_in_transaction(
        &self,
        user: &dyn Authenticatable,
        transaction: &mut Transaction,
        value: serde_json::Value,
        options: &AuthenticationOptions,
    ) -> Result<PasskeyRecord, HttpError> {
        self.verify_transaction(user, transaction, value, options, true)
            .await
    }
    async fn verify_transaction(
        &self,
        user: &dyn Authenticatable,
        transaction: &mut Transaction,
        value: serde_json::Value,
        options: &AuthenticationOptions,
        discoverable: bool,
    ) -> Result<PasskeyRecord, HttpError> {
        let account = account(&self.0, user, transaction)?;
        let id = value
            .get("rawId")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| invalid("Invalid credential format."))?;
        let record = self
            .0
            .passkey_store()
            .records_in_transaction(transaction, "credential_id", id.into())
            .await
            .map_err(internal)?
            .into_iter()
            .next()
            .ok_or_else(|| {
                invalid("Passkey not recognized. It may have been removed from your account.")
            })?;
        record
            .verify_in_transaction(
                value,
                options,
                &self.0.webauthn().map_err(internal)?,
                &account.key,
                transaction,
                discoverable,
            )
            .await?;
        crate::events::verified(user, &record).await?;
        Ok(record)
    }
}
impl DeletePasskey {
    /*
     * 复用所选模型删除行为和事件；凭据必须属于当前持久化用户。
     * 调用方负责锁定用户及事务失败回滚。
     */
    pub async fn call_in_transaction<M: Model + Authenticatable>(
        &self,
        user: &M,
        transaction: &mut Transaction,
        record: &PasskeyRecord,
    ) -> Result<(), HttpError> {
        if !user.state().exists()
            || !transaction.belongs_to(&user.get_connection().map_err(internal)?)
            || user.get_key().is_none()
            || user.get_key() != user.get_raw_original(user.get_key_name())
        {
            return Err(internal(openrust::database::DatabaseError::InvalidQuery(
                "通行密钥删除须使用当前持久化用户的同一连接事务",
            )));
        }
        record
            .delete_in_transaction(transaction, user.get_key().expect("已核对真实主键"))
            .await
            .map_err(internal)?;
        crate::events::deleted(user, record).await
    }
}
