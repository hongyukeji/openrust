use openrust::http::HttpPlugin;
use openrust::i18n::I18nPlugin;
use openrust::mail::MailerPlugin;
use openrust::prelude::*;
use openrust::sms::SmsPlugin;

use crate::routes;

pub struct Example;

impl Application for Example {
    type Config = NoConfig;

    fn build(app: &mut AppBuilder) {
        app.plugin(HttpPlugin)
            .plugin(I18nPlugin::default())
            .plugin(openrust::cache::CachePlugin)
            .plugin(MailerPlugin)
            .plugin(SmsPlugin)
            .plugin(openrust::view::ViewPlugin)
            .plugin(DatabasePlugin)
            .plugin(openrust::auth::JwtPlugin::new())
            .plugin(openrust::auth::AuthPlugin::new())
            .plugin(openrust::audit::AuditPlugin::database())
            .plugin(
                openrust::signing::SignedLinksPlugin::new().with_database_revocations_from_app(),
            )
            .plugin(openrust::security::EncryptionPlugin)
            .plugin(openrust::auth::AuthFlowsPlugin::new())
            .plugin(MigrationPlugin::<openrust_migration::Migrator>::new())
            .plugin(SeedPlugin::new(crate::seed::run))
            .plugin(crate::jobs::JobsPlugin)
            .routes(routes::register);
    }
}
