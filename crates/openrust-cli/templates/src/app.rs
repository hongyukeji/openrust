use openrust::http::HttpPlugin;
use openrust::mail::MailerPlugin;
use openrust::prelude::*;

use crate::routes;

pub struct {{type_name}};

impl Application for {{type_name}} {
    type Config = NoConfig;

    fn build(app: &mut AppBuilder) {
        app.plugin(HttpPlugin)
            .plugin(MailerPlugin){{database_plugins}}
            .routes(routes::register);
    }
}
