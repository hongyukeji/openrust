use openrust::http::HttpPlugin;
use openrust::prelude::*;

use crate::routes;

pub struct Server;

impl Application for Server {
    type Config = NoConfig;

    fn build(app: &mut AppBuilder) {
        let app = app
            .plugin(HttpPlugin)
            .plugin(openrust::i18n::I18nPlugin::default())
            .plugin(openrust::mail::MailerPlugin)
            .plugin(openrust::sms::SmsPlugin);
        #[cfg(feature = "cache-redis")]
        let app = app.plugin(openrust::cache::CachePlugin);
        #[cfg(feature = "encryption")]
        let app = app.plugin(openrust::security::EncryptionPlugin);
        #[cfg(feature = "storage-local")]
        let app = app.plugin(openrust::storage::StoragePlugin);
        app.routes(routes::register);
    }
}
