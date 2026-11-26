use crate::app::models::User;
use openrust::{auth::{AuthFuture, AuthResult}, database::Transaction};
use openrust_fortify::starter::deletion::{DeleteUser as DefaultDeleteUser, DeletesUsers};

pub(crate) struct DeleteUser;
impl DeletesUsers<User> for DeleteUser {
    /*
     * 可在preflight中检查业务依赖，或替换这里的删除策略；只使用传入事务。
     * 套件统一撤销认证凭据并持久化文件清理任务，不默认级联删除业务数据。
     */
    fn delete<'a>(&'a self, user: &'a mut User, transaction: &'a mut Transaction) -> AuthFuture<'a, AuthResult<()>> {
        DefaultDeleteUser.delete(user, transaction)
    }
}
