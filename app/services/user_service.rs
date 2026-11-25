use openrust::database::{Connection, DatabaseResult};

use super::service::TransactionalService;
use crate::app::{
    models::{User, model::Model},
    repositories::{repository::Repository, user_repository::UserRepository},
};

#[allow(dead_code, reason = "由需要账户资料编辑的应用入口按需接入")]
pub(crate) struct UserService {
    users: UserRepository,
}

impl TransactionalService for UserService {
    fn connection(&self) -> DatabaseResult<Connection> {
        Ok(self.users.query()?.get_query().get_connection().clone())
    }
}

#[allow(dead_code, reason = "由需要账户资料编辑的应用入口按需接入")]
impl UserService {
    pub(crate) const fn new(users: UserRepository) -> Self {
        Self { users }
    }

    /// 按邮箱查询用户。调用方决定是否以及如何公开查询结果。
    pub(crate) async fn find_by_email(&self, email: &str) -> DatabaseResult<Option<User>> {
        self.users.find_by_email(email).await
    }

    /// 调用方先完成授权和名称校验。返回 false 表示无变化或模型事件取消了写入。
    pub(crate) async fn rename(&self, id: i64, name: &str) -> DatabaseResult<bool> {
        let name = name.to_owned();
        let users = self.users;
        self.transaction(
            move |transaction| {
                let name = name.clone();
                Box::pin(async move {
                    let mut user = users.find_for_update(transaction, id).await?;
                    let name = name.into();
                    if user.get_attribute(User::NAME) == Some(&name) {
                        return Ok(false);
                    }
                    user.update_in_transaction([(User::NAME.into(), name)].into(), transaction)
                        .await
                })
            },
            1,
        )
        .await
    }
}
