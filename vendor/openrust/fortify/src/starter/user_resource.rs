use openrust::{
    contracts::auth::Authenticatable,
    database::Value,
    http::{HttpError, Request},
};
use serde_json::{Map, Value as Json, json};

/// 当前用户的默认安全投影；不枚举模型属性，也不访问密码或恢复码。
/// 应用可以替换响应绑定来提供自己的 Resource。
pub struct UserResource;
impl UserResource {
    pub fn make(user: &dyn Authenticatable) -> Result<Json, HttpError> {
        let mut fields = Map::from_iter([("id".into(), json!(user.get_auth_identifier()))]);
        for name in ["name", "email", "email_verified_at"] {
            let value = match user.get_auth_attribute(name)? {
                None | Some(Value::Null) => Json::Null,
                Some(Value::Text(value)) => Json::String(value),
                // 默认字段是字符串／时间，不静默暴露复杂自定义属性。
                Some(_) => {
                    return Err(crate::resolver::internal(std::io::Error::other(format!(
                        "用户资源字段 {name} 必须为字符串或空值；请替换应用资源以适配自定义类型"
                    ))));
                }
            };
            fields.insert(name.into(), value);
        }
        Ok(Json::Object(fields))
    }

    pub async fn current(request: &mut Request) -> Result<Json, HttpError> {
        let user = openrust::auth::Auth::user(request)
            .await?
            .ok_or(HttpError::Unauthenticated)?;
        Self::make(user.as_ref())
    }
}
