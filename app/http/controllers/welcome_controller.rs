use openrust::{
    support::facades::Config,
    view::{View, view},
};

pub(crate) struct WelcomeController;

impl WelcomeController {
    pub(crate) fn index() -> View {
        let name = Config::string("app.name").unwrap_or_else(|_| "App".into());
        let version = env!("CARGO_PKG_VERSION");

        view("welcome").with("name", name).with("version", version)
    }
}
