pub mod middleware;
use openrust::{
    cache::{CacheError, Repository as CacheRepository, Ttl, Value as CacheValue},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{RouteGroup, RoutingError},
    support::{
        Date, DateTimezone, data_get,
        facades::{Cache, Config, Facade, Route},
        php_integer_cast, php_regular_compare,
    },
};
use serde_json::Value;
use std::{collections::BTreeMap, future::Future, sync::Arc, time::Instant};
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
        let config = Config::get_facade_root()?;
        let cache_name = config
            .get("pulse.cache")
            .and_then(|value| value.as_str().map(str::to_owned));
        let cache = Cache::store(cache_name.as_deref())?;
        let mut cards = serde_json::Map::new();
        let mut timings = BTreeMap::new();
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
            let key = if is_usage {
                format!("usage:{kind}:{period}")
            } else {
                format!("{name}:{order}:{period}")
            };
            let rows = remember_card(&cache, &key, async {
                let stored = pulse
                    .aggregate(kind, &aggregates, interval, Some(order), "desc", limit)
                    .await?;
                let mut rows: Vec<_> = stored.into_iter().map(row).collect();
                if is_usage {
                    let keys: Vec<_> = rows
                        .iter()
                        .filter_map(|row| row.get("key").and_then(Value::as_str).map(str::to_owned))
                        .collect();
                    let users = pulse.resolve_users(&keys).await?;
                    for row in &mut rows {
                        if let Some(key) = row.get("key").and_then(Value::as_str) {
                            let user = serde_json::to_value(users.find(key))?;
                            row.as_object_mut()
                                .expect("database row")
                                .insert("user".into(), user);
                        }
                    }
                }
                Ok(Value::Array(rows))
            })
            .await?;
            cards.insert(name.into(), rows.value);
            timings.insert(name, rows.timing);
        }
        let cache_types = ["cache_hit".into(), "cache_miss".into()];
        let keys = remember_card(&cache, &format!("cache:keys:{period}"), async {
            let rows = pulse
                .aggregate_types(&cache_types, "count", interval, None, "desc", 101)
                .await?;
            Ok(Value::Array(rows.into_iter().map(row).collect()))
        })
        .await?;
        cards.insert("cache".into(), keys.value);
        timings.insert("cache", keys.timing);
        let mut totals = remember_card(&cache, &format!("cache:all:{period}"), async {
            let totals = pulse
                .aggregate_total(&cache_types, "count", interval)
                .await?;
            Ok(serde_json::json!({
                "hits": totals.get(&cache_types[0]).copied().flatten().unwrap_or(0.0),
                "misses": totals.get(&cache_types[1]).copied().flatten().unwrap_or(0.0),
            }))
        })
        .await?;
        let sample_rate = config
            .get("pulse.recorders.CacheInteractions.sample_rate")
            .and_then(|value| value.as_f64())
            .unwrap_or(1.0);
        totals
            .value
            .as_object_mut()
            .ok_or(CacheError::InvalidValue)?
            .insert("sample_rate".into(), sample_rate.into());
        cards.insert("cache_totals".into(), totals.value);
        timings.insert("cache_totals", totals.timing);
        let queues = remember_card(&cache, &format!("queues:{period}"), async {
            let values = pulse
                .graph(
                    &["queued", "processing", "processed", "released", "failed"].map(str::to_owned),
                    "count",
                    interval,
                )
                .await?;
            Ok(serde_json::to_value(values)?)
        })
        .await?;
        cards.insert("queues".into(), queues.value);
        timings.insert("queues", queues.timing);
        let options = config.get("pulse.dashboard.servers").unwrap_or_default();
        let ignore_after = match &options["ignore_after"] {
            Value::Null => None,
            value => Some(server_max_age(value)?),
        };
        let key = format!("servers:{period}:{:x}", md5::compute(options.to_string()));
        let snapshot = remember_card(&cache, &key, async {
            let (systems, graphs) = servers(&pulse, interval, &options, ignore_after).await?;
            Ok(Value::Array(vec![Value::Array(systems), graphs]))
        })
        .await?;
        let Value::Array(server_values) = snapshot.value else {
            return Err(CacheError::InvalidValue.into());
        };
        let [systems, graphs]: [Value; 2] = server_values
            .try_into()
            .map_err(|_| CacheError::InvalidValue)?;
        cards.insert("servers".into(), systems);
        cards.insert("server_graphs".into(), graphs);
        timings.insert("servers", snapshot.timing);
        cards.insert("query_timing".into(), serde_json::to_value(timings)?);
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
#[derive(serde::Deserialize, serde::Serialize)]
struct CardSnapshot {
    value: Value,
    timing: QueryTiming,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct QueryTiming {
    time: f64,
    run_at: String,
}

async fn remember_card(
    cache: &CacheRepository,
    key: &str,
    query: impl Future<Output = crate::Result<Value>>,
) -> Result<CardSnapshot, HttpError> {
    let key = format!("openrust:pulse:card:query:{key}");
    let value = cache
        .remember(&key, Some(Ttl::Seconds(5)), async {
            let run_at = Date::now(Some(DateTimezone::UTC))
                .and_then(|date| date.format("Y-m-d H:i:s"))
                .map_err(|source| CacheError::Backend {
                    source: Box::new(source),
                })?;
            let start = Instant::now();
            let value = query.await.map_err(|source| CacheError::Backend {
                source: Box::new(source),
            })?;
            let snapshot = CardSnapshot {
                value,
                timing: QueryTiming {
                    time: start.elapsed().as_secs_f64() * 1000.0,
                    run_at,
                },
            };
            serde_json::to_value(snapshot)
                .map(CacheValue::Json)
                .map_err(|source| CacheError::Backend {
                    source: Box::new(source),
                })
        })
        .await?;
    let CacheValue::Json(value) = value else {
        return Err(CacheError::InvalidValue.into());
    };
    Ok(serde_json::from_value(value)?)
}
async fn servers(
    pulse: &crate::Pulse,
    interval: i64,
    options: &Value,
    ignore_after: Option<i128>,
) -> crate::Result<(Vec<Value>, Value)> {
    let sort_by = options["sort_by"].as_str().unwrap_or("name");
    let descending = options["sort_direction"] == "desc";
    let now = jiff::Timestamp::now();
    let mut graphs = pulse
        .graph(&["cpu".into(), "memory".into()], "avg", interval)
        .await?;
    let mut visible_graphs = serde_json::Map::new();
    let mut systems = Vec::new();
    for system in pulse.values("system", None).await? {
        let mut system = row(system);
        let timestamp = system["timestamp"].as_i64().unwrap_or_default();
        if let Some(microseconds) = ignore_after
            && i128::from(timestamp) * 1_000_000 + microseconds
                < i128::from(now.as_second()) * 1_000_000
                    + i128::from(now.subsec_nanosecond() / 1_000)
        {
            continue;
        }
        system["recently_reported"] = (timestamp > now.as_second() - 30).into();
        let snapshot = system["value"]
            .as_str()
            .and_then(|value| serde_json::from_str::<Value>(value).ok())
            .unwrap_or_default();
        let graph = system["key"]
            .as_str()
            .and_then(|key| graphs.remove(key))
            .map(serde_json::to_value)
            .transpose()?;
        let series = graph.as_ref().unwrap_or(&Value::Null);
        let fields = serde_json::json!({
            "name": snapshot["name"],
            "cpu_current": php_integer_cast(&snapshot["cpu"]),
            "memory_current": php_integer_cast(&snapshot["memory_used"]),
            "memory_total": php_integer_cast(&snapshot["memory_total"]),
            "updated_at": timestamp,
            "recently_reported": system["recently_reported"],
            "cpu": series.get("cpu").cloned().unwrap_or_else(|| Value::Array(vec![])),
            "memory": series.get("memory").cloned().unwrap_or_else(|| Value::Array(vec![])),
            "storage": snapshot.get("storage").cloned().unwrap_or_else(|| Value::Array(vec![])),
        });
        let order = data_get(&fields, Some(sort_by)).unwrap_or_default();
        if let Some((key, graph)) = system["key"].as_str().zip(graph) {
            visible_graphs.insert(key.into(), graph);
        }
        systems.push((order, system));
    }
    let systems = sort_systems(systems, descending);
    Ok((
        systems.into_iter().map(|(_, row)| row).collect(),
        Value::Object(visible_graphs),
    ))
}

fn sort_systems(mut systems: Vec<(Value, Value)>, descending: bool) -> Vec<(Value, Value)> {
    let follows = |left: &Value, right: &Value| {
        let order = php_regular_compare(left, right);
        if descending {
            order.is_lt()
        } else {
            order.is_gt()
        }
    };
    // PHP 混合值的比较可能存在循环，不能传给要求全序的切片排序。
    if systems.len() <= 16 {
        for index in 1..systems.len() {
            let mut current = index;
            while current > 0 && follows(&systems[current - 1].0, &systems[current].0) {
                systems.swap(current - 1, current);
                current -= 1;
            }
        }
        return systems;
    }
    let count = systems.len();
    let right = systems.split_off(count / 2);
    let mut left = sort_systems(systems, descending).into_iter().peekable();
    let mut right = sort_systems(right, descending).into_iter().peekable();
    let mut sorted = Vec::with_capacity(count);
    while let (Some(l), Some(r)) = (left.peek(), right.peek()) {
        let source = if follows(&l.0, &r.0) {
            &mut right
        } else {
            &mut left
        };
        sorted.extend(source.next());
    }
    sorted.extend(left);
    sorted.extend(right);
    sorted
}

fn server_max_age(value: &Value) -> crate::Result<i128> {
    use openrust::support::InputInterval;

    if let Some(seconds) = value
        .as_i64()
        .or_else(|| value.as_str()?.trim().parse().ok())
    {
        return Ok(i128::from(seconds) * 1_000_000);
    }
    if let Some(seconds) = value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse().ok())
        .filter(|seconds| seconds.is_finite())
    {
        return Ok(i128::from(seconds as i64) * 1_000_000);
    }
    let invalid = || crate::Error::InvalidConfiguration("服务器快照时限无效".into());
    let text = value.as_str().ok_or_else(invalid)?;
    let duration = InputInterval::from_date_string(text).map_err(|_| invalid())?;
    Ok(duration.total_microseconds())
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
