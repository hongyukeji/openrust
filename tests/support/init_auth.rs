#[cfg(all(feature = "passport", feature = "sanctum"))]
fn init_auth_tokens(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        use openrust::{contracts::container::Container as _, database::eloquent::Model};
        let container = openrust::container::Container::get_instance();
        let mut query = crate::app::models::User::query().unwrap();
        query.r#where("email", "=", "alice@example.test");
        let user = query.first().await.unwrap().unwrap();
        let passport = openrust_passport::HasApiTokens::create_token(&user, "Init acceptance", &[])
            .await
            .unwrap();
        let sanctum = container.make::<openrust::sanctum::Sanctum>().unwrap();
        let database = container
            .make::<openrust::database::DatabaseManager>()
            .unwrap();
        let sanctum = openrust::sanctum::HasApiTokens::create_token(
            &user,
            &sanctum,
            &database,
            "Init acceptance",
            &["*"],
            None,
        )
        .await
        .unwrap();
        openrust::http::Response::json(
            openrust::http::StatusCode::OK,
            serde_json::json!({"passport":passport.access_token,"sanctum":sanctum.plain_text_token.as_str()}),
        )
    })
}
#[cfg(feature = "passport")]
fn init_passport_user(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move { openrust::http::Response::json(openrust::http::StatusCode::OK, true) })
}
