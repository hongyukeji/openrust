use openrust::{
    contracts::auth::Authenticatable,
    database::Value,
    http::{HttpError, Request},
};
use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::{HttpFuture, resources::json::JsonResource},
    support::{
        ProviderResult,
        facades::{App, Facade as _},
    },
};
use serde_json::{Map, Value as Json, json};
use std::{any::Any, sync::Arc};

type Resolve = dyn for<'a> Fn(&'a dyn Authenticatable, &'a mut Request) -> HttpFuture<'a, Result<Json, HttpError>>
    + Send
    + Sync;
struct Projection(Arc<Resolve>);

/// 当前用户的默认安全投影；不枚举模型属性，也不访问密码或恢复码。
/// 通过use_resource选择应用的原生JsonResource；未选择时使用四字段白名单。
/// 当前用户的默认安全投影；不枚举模型属性，也不访问密码或恢复码。
/// 通过use_resource选择应用的原生JsonResource；未选择时使用四字段白名单。
pub struct UserResource;
impl UserResource {
    /// 为所属应用选择原生资源；这里只解析用户数据，不改变外层响应状态、封装或响应头。
    pub fn use_resource<M, R>(app: &Application) -> ProviderResult
    where
        M: Authenticatable + Clone,
        R: JsonResource + From<M> + Send + 'static,
    {
        app.container()
            .instance(Arc::new(Projection(Arc::new(|user, request| {
                Box::pin(async move {
                    let model = (user as &dyn Any)
                        .downcast_ref::<M>()
                        .ok_or(HttpError::Unauthenticated)?
                        .clone();
                    let mut resource = R::from(model);
                    if let Some(prepare) = resource.prepare_response(request) {
                        prepare.await?;
                    }
                    let value = resource.to_array(request)?;
                    if !value.is_object() {
                        return Err(crate::resolver::internal(std::io::Error::other(
                            "用户资源必须返回JSON对象",
                        )));
                    }
                    Ok(value)
                })
            }))))?;
        Ok(())
    }

    pub async fn for_request(
        user: &dyn Authenticatable,
        request: &mut Request,
    ) -> Result<Json, HttpError> {
        let app = App::get_facade_application()?.ok_or_else(|| {
            crate::resolver::internal(std::io::Error::other("用户资源必须在应用作用域内解析"))
        })?;
        if app
            .container()
            .has(openrust::container::ServiceKey::typed::<Projection>())
        {
            let projection = app.make::<Projection>().await?;
            (projection.0)(user, request).await
        } else {
            Self::make(user)
        }
    }

    /// 网页内部字段不跟随API资源的自定义字段改名。
    pub(crate) async fn current_default(request: &mut Request) -> Result<Json, HttpError> {
        let user = openrust::auth::Auth::user(request)
            .await?
            .ok_or(HttpError::Unauthenticated)?;
        Self::make(user.as_ref())
    }

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
        Self::for_request(user.as_ref(), request).await
    }
}
