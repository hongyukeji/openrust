use openrust::prelude::*;

pub fn register(routes: Routes) -> Routes {
    routes.get("/", root)
}

async fn root() -> Json<std::collections::BTreeMap<&'static str, &'static str>> {
    Json(std::collections::BTreeMap::from([
        ("name", "openrust-server"),
        ("version", env!("CARGO_PKG_VERSION")),
    ]))
}
