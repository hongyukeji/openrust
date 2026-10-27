use openrust::prelude::*;

pub fn register(routes: Routes) -> Routes {
    routes.get("/", root)
}

async fn root(
    State(state): State<openrust::AppState>,
) -> Json<std::collections::BTreeMap<&'static str, String>> {
    Json(std::collections::BTreeMap::from([
        ("name", state.config().app.name.clone()),
        ("version", env!("CARGO_PKG_VERSION").to_owned()),
    ]))
}
