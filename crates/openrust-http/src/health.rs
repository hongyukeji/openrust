//! Liveness endpoint.

use axum::Json;
use serde::Serialize;

use crate::routes::Routes;

/// Body of `GET /health`.
#[derive(Debug, Serialize)]
pub struct Health {
    /// Always `"ok"` when the process can answer.
    pub status: &'static str,
}

/// Liveness probe: answers as long as the process serves requests. Dependency checks belong to
/// the readiness endpoint added by the capability crates.
pub async fn live() -> Json<Health> {
    Json(Health { status: "ok" })
}

/// Registers `GET /health`.
pub fn routes(routes: Routes) -> Routes {
    routes.get("/health", live)
}
