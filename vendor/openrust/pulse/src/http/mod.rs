pub mod middleware;
use openrust::{
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{RouteGroup, RoutingError},
    support::facades::{Config, Facade, Route},
};
use std::sync::Arc;
pub(crate) fn routes() -> Result<(), RoutingError> {
    if !crate::facades::Options::get_facade_root()
        .map_err(|_| RoutingError::NoRegistrationScope)?
        .registers_routes
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        return Ok(());
    }
    let config = Config::get_facade_root().map_err(|_| RoutingError::NoRegistrationScope)?;
    let path = config
        .get("pulse.path")
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or("pulse".into());
    let mut group = RouteGroup::new().prefix(path);
    if let Some(domain) = config
        .get("pulse.domain")
        .and_then(|v| v.as_str().map(str::to_owned))
    {
        group = group.domain(domain);
    }
    for name in config
        .get("pulse.middleware")
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(serde_json::Value::as_str)
    {
        if name == "openrust_pulse::http::middleware::Authorize" {
            group = group.middleware(Arc::new(middleware::Authorize));
        } else {
            group = group.middleware_name(name);
        }
    }
    Route::group(group, |router| {
        router.get("/", page)?.name("pulse");
        router.get("/api/metrics", metrics)?;
        Ok(())
    })
}
fn page(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let factory = openrust::support::facades::View::get_facade_root()?;
        let html = match factory.render("pulse::dashboard", &Default::default()) {
            Ok(html) => html,
            Err(openrust::view::ViewError::NotFound) => {
                include_str!("../../resources/views/dashboard.html").into()
            }
            Err(e) => return Err(e.into()),
        };
        let pulse = crate::facades::Pulse::get_facade_root()?;
        let html = if html.contains("<!-- PULSE_STYLES -->") {
            html.replace("<!-- PULSE_STYLES -->", &pulse.css()?)
        } else {
            html
        };
        let html = html.replace("<!-- PULSE_SCRIPTS -->", &pulse.js());
        let mut response = Response::new(StatusCode::OK, Body::from(html));
        response.parts_mut().headers.insert(
            "content-type",
            ::http::HeaderValue::from_static("text/html; charset=utf-8"),
        );
        response.parts_mut().headers.insert(
            "cache-control",
            ::http::HeaderValue::from_static("no-store"),
        );
        Ok(response)
    })
}
fn metrics(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let period = request
            .query(Some("period"))?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or("1h".into());
        let interval = match period.as_str() {
            "1h" => 3600,
            "6h" => 21600,
            "24h" => 86400,
            "7d" => 604800,
            _ => return Err(HttpError::InvalidRequest),
        };
        let usage = request.query(Some("usage"))?;
        let usage_type = match usage.as_ref().and_then(serde_json::Value::as_str) {
            None | Some("requests") => "user_request",
            Some("slow_requests") => "slow_user_request",
            Some("jobs") => "user_job",
            Some(_) => return Err(HttpError::InvalidRequest),
        };
        let pulse = crate::facades::Pulse::get_facade_root()?;
        let mut cards = serde_json::Map::new();
        for (name, kind) in [
            ("slow_requests", "slow_request"),
            ("slow_queries", "slow_query"),
            ("slow_jobs", "slow_job"),
            ("slow_outgoing_requests", "slow_outgoing_request"),
            ("exceptions", "exception"),
            ("requests", usage_type),
            ("jobs", "user_job"),
        ] {
            let is_usage = matches!(name, "requests" | "jobs");
            let aggregates = if is_usage {
                &["count"][..]
            } else {
                &["count", "max"]
            };
            let aggregates = aggregates
                .iter()
                .map(|value| (*value).to_owned())
                .collect::<Vec<_>>();
            let order = match name {
                "requests" | "jobs" => "count",
                "exceptions" => {
                    if request
                        .query(Some("exceptions"))?
                        .as_ref()
                        .and_then(serde_json::Value::as_str)
                        == Some("latest")
                    {
                        "max"
                    } else {
                        "count"
                    }
                }
                _ => {
                    let parameter = name.replace('_', "-");
                    if request
                        .query(Some(parameter.as_str()))?
                        .as_ref()
                        .and_then(serde_json::Value::as_str)
                        == Some("count")
                    {
                        "count"
                    } else {
                        "max"
                    }
                }
            };
            let limit = if is_usage { 10 } else { 101 };
            let rows = pulse
                .aggregate(kind, &aggregates, interval, Some(order), "desc", limit)
                .await?;
            let mut rows: Vec<_> = rows.into_iter().map(row).collect();
            if is_usage {
                let keys: Vec<_> = rows
                    .iter()
                    .filter_map(|row| row.get("key").and_then(|v| v.as_str()).map(str::to_owned))
                    .collect();
                let users = pulse.resolve_users(&keys).await?;
                for row in &mut rows {
                    if let Some(key) = row.get("key").and_then(|v| v.as_str()) {
                        let user = serde_json::to_value(users.find(key))?;
                        row.as_object_mut()
                            .expect("database row")
                            .insert("user".into(), user);
                    }
                }
            }
            cards.insert(name.into(), serde_json::Value::Array(rows));
        }
        let cache_types = ["cache_hit".into(), "cache_miss".into()];
        let cache = pulse
            .aggregate_types(&cache_types, "count", interval, None, "desc", 101)
            .await?;
        cards.insert(
            "cache".into(),
            serde_json::Value::Array(cache.into_iter().map(row).collect()),
        );
        let totals = pulse
            .aggregate_total(&cache_types, "count", interval)
            .await?;
        let sample_rate = Config::get_facade_root()?
            .get("pulse.recorders.CacheInteractions.sample_rate")
            .and_then(|value| value.as_f64())
            .unwrap_or(1.0);
        cards.insert(
            "cache_totals".into(),
            serde_json::json!({
                "hits": totals.get(&cache_types[0]).copied().flatten().unwrap_or(0.0),
                "misses": totals.get(&cache_types[1]).copied().flatten().unwrap_or(0.0),
                "sample_rate": sample_rate,
            }),
        );
        cards.insert(
            "queues".into(),
            serde_json::to_value(
                pulse
                    .graph(
                        &["queued", "processing", "processed", "released", "failed"]
                            .map(str::to_owned),
                        "count",
                        interval,
                    )
                    .await?,
            )?,
        );
        let systems = pulse.values("system", None).await?;
        cards.insert(
            "servers".into(),
            serde_json::Value::Array(systems.into_iter().map(row).collect()),
        );
        let mut response = Response::new(StatusCode::OK, Body::from(serde_json::to_vec(&cards)?));
        response.parts_mut().headers.insert(
            "content-type",
            ::http::HeaderValue::from_static("application/json"),
        );
        response.parts_mut().headers.insert(
            "cache-control",
            ::http::HeaderValue::from_static("no-store"),
        );
        Ok(response)
    })
}
fn row(row: openrust::database::Row) -> serde_json::Value {
    serde_json::Value::Object(
        row.columns()
            .iter()
            .map(|(k, v)| {
                use openrust::database::Value as Sql;
                (
                    k.clone(),
                    match v {
                        Sql::Null => serde_json::Value::Null,
                        Sql::Text(v) => v.clone().into(),
                        Sql::Bytes(v) if k == "key" => {
                            String::from_utf8_lossy(v).into_owned().into()
                        }
                        Sql::Integer(v) => (*v).into(),
                        Sql::Unsigned(v) => (*v).into(),
                        Sql::Real(v) => (*v).into(),
                        Sql::Json(v) => v.clone(),
                        Sql::Bool(v) => (*v).into(),
                        _ => serde_json::Value::Null,
                    },
                )
            })
            .collect(),
    )
}

impl From<crate::Error> for HttpError {
    fn from(source: crate::Error) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}
