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
        #[cfg(feature = "encryption")]
        let app = app.plugin(openrust::security::EncryptionPlugin);
        app.routes(routes::register);
    }
}
