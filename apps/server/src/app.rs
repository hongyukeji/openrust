use openrust::prelude::*;

use crate::routes;

pub struct Server;

impl Application for Server {
    type Config = NoConfig;

    fn build(app: &mut AppBuilder) {
        app.routes(routes::register);
    }
}
