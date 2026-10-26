use openrust::http::HttpPlugin;
use openrust::prelude::*;

use crate::routes;

pub struct Server;

impl Application for Server {
    type Config = NoConfig;

    fn build(app: &mut AppBuilder) {
        app.plugin(HttpPlugin)
            .plugin(openrust::i18n::I18nPlugin::default())
            .plugin(openrust::mail::MailerPlugin)
            .routes(routes::register);
    }
}
