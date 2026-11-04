//! 经认证、限流的欢迎消息入口；业务调用显式触发通信副作用。

use std::collections::BTreeMap;

use crate::modules::example::http::requests::welcome::WelcomeInput;
use crate::modules::example::services::welcome::{WelcomeService, verified_email};
use openrust::Result;
use openrust::auth::CurrentUser;
use openrust::http::Json;
use openrust::i18n::{I18n, RequestLocale};
use openrust::prelude::{Db, Inject};

pub(crate) async fn send(
    user: CurrentUser,
    Inject(db): Inject<Db>,
    Inject(welcome): Inject<WelcomeService>,
    Inject(i18n): Inject<I18n>,
    locale: RequestLocale,
    Json(_input): Json<WelcomeInput>,
) -> Result<Json<BTreeMap<&'static str, &'static str>>> {
    let email = verified_email(&db, &db.primary_connection()?, user.integer_id()?).await?;
    welcome.send_email(email, &i18n, locale.locale()).await?;
    Ok(Json(BTreeMap::from([("status", "accepted")])))
}
