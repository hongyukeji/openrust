#[cfg(feature = "sanctum")]
impl openrust::sanctum::HasApiTokens for User {
    #[cfg(feature = "fortify")]
    fn create_token<'a>(
        &'a self,
        sanctum: &'a openrust::sanctum::Sanctum,
        database: &'a openrust::database::DatabaseManager,
        name: &'a str,
        abilities: &'a [&'a str],
        expires_at: Option<openrust_fortify::starter::TokenTimestamp>,
    ) -> impl std::future::Future<Output = openrust::auth::AuthResult<openrust::sanctum::NewAccessToken>> + Send + 'a {
        async move {
            use openrust::support::facades::Facade as _;
            let mut user = self.clone();
            let config = openrust::support::facades::Config::get_facade_root()?;
            openrust_fortify::starter::credentials::issue_with_expiration(&mut user, sanctum, database, config.as_ref(), name, abilities, expires_at).await
        }
    }
}
