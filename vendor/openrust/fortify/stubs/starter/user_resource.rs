use openrust::{http::{Request, resources::json::{JsonResource, ResourceResult}}};
use serde_json::Value;
use crate::app::models::User;

pub(crate) struct UserResource(User);
impl From<User> for UserResource {
    fn from(user: User) -> Self { Self(user) }
}
impl JsonResource for UserResource {
    fn to_array(&self, _request: &Request) -> ResourceResult<Value> {
        // 显式选择公开字段；不要将用户模型整体序列化到认证响应。
        openrust_fortify::starter::UserResource::make(&self.0)
    }
}
