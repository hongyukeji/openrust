use openrust::{
    support::facades::Config,
    view::{View, view},
};

pub(crate) struct WelcomeController;

impl WelcomeController {
    pub(crate) fn index() -> View {
        let name = Config::stringOr("app.name", "App");
        let version = Config::stringOr("app.version", "1.0.0");

        view("welcome").with("name", name).with("version", version)
    }
}
