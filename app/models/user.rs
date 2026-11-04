//! 应用账户属性适配；复用同一UserProvider，不创建第二份账户存储。

use openrust::{
    Result,
    auth::{Authenticatable, GenericUser},
};
use serde_json::{Map, Value};

/// 当前应用账户模型；密码列沿用已有存储字段，其他应用可实现自己的 `Authenticatable`。
pub struct User(GenericUser);
impl User {
    /// 从数据库Provider已转换的属性构造账户，不要求邮箱或手机号存在。
    pub fn new(attributes: Map<String, Value>) -> Self {
        Self(GenericUser::new(attributes))
    }
}
impl Authenticatable for User {
    fn auth_identifier_name(&self) -> &'static str {
        "id"
    }
    fn auth_identifier(&self) -> Result<Value> {
        self.0.auth_identifier()
    }
    fn auth_password_name(&self) -> &'static str {
        "password_hash"
    }
    fn auth_password(&self) -> Result<Option<String>> {
        match self.0.attribute("password_hash")? {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(value)) => Ok(Some(value)),
            _ => Err(openrust::Error::custom(
                "app_password_hash_invalid",
                "应用账户密码哈希类型无效",
            )),
        }
    }
    fn remember_token(&self) -> Result<Option<String>> {
        self.0.remember_token()
    }
    fn set_remember_token(&self, token: Option<String>) -> Result<()> {
        self.0.set_remember_token(token)
    }
    fn compare_and_set_remember_token(
        &self,
        expected: Option<&str>,
        replacement: Option<String>,
    ) -> Result<bool> {
        self.0.compare_and_set_remember_token(expected, replacement)
    }
    fn remember_token_name(&self) -> &'static str {
        "remember_token"
    }
}
