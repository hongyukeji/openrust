use openrust::database::{DatabaseError, DatabaseResult, Transaction};

use super::repository::Repository;
use crate::app::models::User;

#[allow(dead_code, reason = "供应用业务服务接入的用户仓储示例")]
#[derive(Clone, Copy)]
pub(crate) struct UserRepository;

impl Repository for UserRepository {
    type Model = User;
}

#[allow(dead_code, reason = "供应用业务服务接入的用户仓储示例")]
impl UserRepository {
    pub(crate) async fn find_by_email(&self, email: &str) -> DatabaseResult<Option<User>> {
        let mut query = self.query()?;
        query.r#where(User::EMAIL, "=", email);
        query.first().await
    }

    pub(crate) async fn find_for_update(
        &self,
        transaction: &mut Transaction,
        id: i64,
    ) -> DatabaseResult<User> {
        let mut query = self.query()?;
        query.where_key(id);
        query
            .lock_for_update()
            .limit(1)
            .get_in_transaction(transaction)
            .await?
            .into_iter()
            .next()
            .ok_or(DatabaseError::ModelNotFound(User::TABLE))
    }
}
