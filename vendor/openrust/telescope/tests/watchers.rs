use openrust::{
    contracts::events::Event,
    foundation::configuration::Routing,
    http::{HttpError, HttpFuture, Request, Response},
    support::facades::{Db, Event as Events, Route},
    testing::TestApp,
};
use openrust_telescope::{
    EntryType, Telescope, TelescopeServiceProvider,
    storage::{DatabaseEntriesRepository, EntryQueryOptions},
    watchers::{EventWatcher, GateWatcher, QueryWatcher, Watcher},
};
use serde_json::json;

#[path = "support/query_caller.rs"]
mod query_caller;

struct Probe(&'static str);
impl Event for Probe {
    fn name(&self) -> &str {
        self.0
    }
    fn properties(&self) -> Option<serde_json::Value> {
        Some(json!({"actual":true}))
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/watchers", handler)?;
    Route::get("/client-error", client_error_handler)?;
    Route::get("/gate-callers", gate_callers)?;
    Route::get("/query-callers", query_callers)?;
    Route::get("/page-callers", page_callers)?;
    Route::get("/model-write-callers", model_write_callers)?;
    Route::get("/relationship-callers", relationship_callers)?;
    Route::get("/pivot-callers", pivot_callers)?;
    for path in ["/json", "/plain", "/boundary", "/large", "/opaque"] {
        Route::get(path, response_handler)?;
    }
    Ok(())
}

struct PageProbe(openrust::database::eloquent::ModelState);

impl openrust::database::eloquent::Model for PageProbe {
    fn metadata() -> &'static openrust::database::eloquent::ModelMetadata {
        use openrust::database::eloquent::ModelMetadata;
        static META: ModelMetadata = ModelMetadata::new("query_page_probe")
            .timestamps(None, None)
            .fillable(&["owner_id"]);
        &META
    }

    fn from_state(state: openrust::database::eloquent::ModelState) -> Self {
        Self(state)
    }

    fn state(&self) -> &openrust::database::eloquent::ModelState {
        &self.0
    }

    fn state_mut(&mut self) -> &mut openrust::database::eloquent::ModelState {
        &mut self.0
    }
}

fn page_callers(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    use openrust::{
        database::eloquent::{
            Model as _,
            relations::{HasMany, HasOne},
        },
        pagination::{CursorOptions, PageOptions},
    };

    Box::pin(async {
        let connection = seed_relationship_source().await;
        let query = connection.table("query_page_probe").order_by("id", "asc");
        let builder = line!() + 1;
        let direct_page = query.paginate(2).await.unwrap();
        assert_eq!(direct_page.total(), 3);
        let query = PageProbe::query().unwrap().with(
            "children",
            HasMany::<PageProbe, PageProbe>::new("id", "owner_id"),
        );
        let options = CursorOptions::default();
        let model = line!() + 1;
        let model_page = query.cursor_paginate_with(options).await.unwrap();
        assert_eq!(model_page.items().len(), 3);
        let parent = &model_page.items()[0];
        assert!(parent.relation_loaded("children"));
        let relation = HasMany::<PageProbe, PageProbe>::new("id", "owner_id");
        let options = PageOptions {
            per_page: Some(1),
            ..PageOptions::default()
        };
        let related = line!() + 1;
        let related_page = relation.paginate_with(parent, options).await.unwrap();
        assert_eq!(related_page.total(), 2);
        assert_eq!(related_page.items().len(), 1);
        let relation = HasOne::<PageProbe, PageProbe>::new("id", "owner_id");
        let single = line!() + 1;
        let single_page = relation.simple_paginate(parent, 1).await.unwrap();
        assert_eq!(single_page.items().len(), 1);
        Response::json(
            http::StatusCode::OK,
            json!([[builder, 2], [model, 2], [related, 2], [single, 1]]),
        )
    })
}

async fn seed_relationship_source() -> openrust::database::Connection {
    let connection = Db::connection(None).unwrap();
    let table = format!("{}query_page_probe", connection.get_table_prefix());
    connection
        .statement(
            &format!("CREATE TABLE {table} (id BIGINT PRIMARY KEY, owner_id BIGINT)"),
            &[],
        )
        .await
        .unwrap();
    connection
        .statement(
            &format!("INSERT INTO {table} VALUES (1, 0), (2, 1), (3, 1)"),
            &[],
        )
        .await
        .unwrap();
    connection
}

fn relationship_callers(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    use futures_util::TryStreamExt as _;
    use openrust::database::eloquent::{
        Model as _,
        relations::{BelongsTo, HasMany, Relation},
    };

    Box::pin(async {
        let connection = seed_relationship_source().await;
        let query = PageProbe::query_on(&connection);
        let parent_read = line!() + 1;
        let parent = query.find_or_fail(1).await.unwrap();
        let children = HasMany::<PageProbe, PageProbe>::new("id", "owner_id");
        let look_up = line!() + 1;
        let child = children.find_or_fail(&parent, 2).await.unwrap();
        let get_many = line!() + 1;
        let related = children.get_results(&parent).await.unwrap();
        assert_eq!(related.len(), 2);

        let inverse = BelongsTo::<PageProbe, PageProbe>::new("owner_id", "id");
        let get_owner = line!() + 1;
        let owner = inverse.get_results(&child).await.unwrap().unwrap();
        assert_eq!(owner.get_key(), parent.get_key());

        let mut tx = Db::begin_transaction().await.unwrap();
        let parents = [&parent];
        let batch = line!() + 2;
        let groups = children
            .match_results_in_transaction(&parents, &mut tx)
            .await
            .unwrap();
        assert_eq!(groups[0].len(), 2);
        tx.commit().await.unwrap();
        let cursor = line!() + 1;
        let results = children.cursor(&parent);
        assert_eq!(results.try_collect::<Vec<_>>().await.unwrap().len(), 2);

        Response::json(
            http::StatusCode::OK,
            json!([
                [parent_read, 1],
                [look_up, 1],
                [get_many, 1],
                [get_owner, 1],
                [batch, 1],
                [cursor, 1]
            ]),
        )
    })
}

fn model_write_callers(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    use openrust::database::eloquent::{Attributes, Model as _};

    Box::pin(async {
        let connection = Db::connection(None).unwrap();
        connection
            .get_schema_builder()
            .create("query_page_probe", |table| {
                table.id();
                table.string("owner_id", None);
            })
            .await
            .unwrap();
        let attributes = |owner: &str| Attributes::from([("owner_id".into(), owner.into())]);
        let first = attributes("first");
        let create = line!() + 1;
        let mut model = PageProbe::create(first).await.unwrap();
        let second = attributes("second");
        let quiet = line!() + 1;
        assert!(model.update_quietly(second).await.unwrap());
        let third = attributes("third");
        let mut tx = Db::begin_transaction().await.unwrap();
        let transaction = line!() + 1;
        assert!(model.update_in_transaction(third, &mut tx).await.unwrap());
        tx.commit().await.unwrap();
        let query = PageProbe::query().unwrap();
        let fourth = attributes("fourth");
        let builder = line!() + 1;
        let builder_record = query.create(fourth).await.unwrap();
        assert!(builder_record.get_key().is_some());
        let deleted = line!() + 1;
        assert!(model.delete().await.unwrap());
        Response::json(
            http::StatusCode::OK,
            json!([
                [create, 1],
                [quiet, 1],
                [transaction, 1],
                [builder, 1],
                [deleted, 1]
            ]),
        )
    })
}

fn pivot_callers(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    use openrust::database::{
        Value,
        eloquent::{Attributes, Model as _, relations::BelongsToMany},
    };

    Box::pin(async {
        let connection = seed_relationship_source().await;
        connection
            .get_schema_builder()
            .create("query_pivot_probe", |table| {
                table.big_integer("owner_id");
                table.big_integer("related_id");
                table.string("label", None).nullable();
            })
            .await
            .unwrap();
        let relation = BelongsToMany::<PageProbe, PageProbe>::new(
            "query_pivot_probe",
            "owner_id",
            "related_id",
            "id",
            "id",
        );
        let parent = PageProbe::query_on(&connection)
            .find_or_fail(1)
            .await
            .unwrap();
        let first = [Value::Integer(2)];
        let second = [Value::Integer(3)];
        let attach = line!() + 1;
        relation.attach(&parent, &first).await.unwrap();
        let attrs = Attributes::from([("label".into(), "updated".into())]);
        let update = line!() + 2;
        let updated = relation
            .update_existing_pivot_or_fail(&parent, &first[0], attrs)
            .await
            .unwrap();
        assert_eq!(updated, 1);
        let sync = line!() + 1;
        let changes = relation.sync(&parent, &second).await.unwrap();
        assert_eq!(changes.attached, second);
        assert_eq!(changes.detached, first);
        let both = [Value::Integer(2), Value::Integer(3)];
        let toggle = line!() + 1;
        let changes = relation.toggle_or_fail(&parent, &both).await.unwrap();
        assert_eq!(changes.attached, first);
        assert_eq!(changes.detached, second);
        let detach = line!() + 2;
        let detached = relation
            .detach_or_fail(&parent, Some(&first))
            .await
            .unwrap();
        assert_eq!(detached, 1);
        Response::json(
            http::StatusCode::OK,
            json!([
                [attach, 1],
                [update, 1],
                [sync, 3],
                [toggle, 3],
                [detach, 1]
            ]),
        )
    })
}

fn query_callers(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    use futures_util::TryStreamExt as _;
    Box::pin(async {
        let connection = Db::connection(None).unwrap();
        query_caller::register_nested_query(&connection);
        let facade = line!() + 1;
        let facade_query = Db::select("SELECT 71 AS facade_caller", &[]);
        let direct = line!() + 1;
        let direct_query = connection.select("SELECT 72 AS direct_caller", &[]);
        let (facade_rows, direct_rows) = tokio::join!(facade_query, direct_query);
        facade_rows.unwrap();
        direct_rows.unwrap();
        let builder = connection
            .table("telescope_entries")
            .select_raw("73 AS builder_caller", vec![]);
        let builder_line = line!() + 1;
        builder.get().await.unwrap();
        let cursor = line!() + 1;
        let rows = connection.cursor("SELECT 74 AS cursor_caller", &[]);
        rows.try_collect::<Vec<_>>().await.unwrap();
        let mut transaction = Db::begin_transaction().await.unwrap();
        let transactional = line!() + 1;
        let rows = transaction.select("SELECT 75 AS transaction_caller", &[]);
        rows.await.unwrap();
        transaction.commit().await.unwrap();
        let outer = line!() + 1;
        let rows = connection.select("SELECT 77 AS outer_caller", &[]);
        rows.await.unwrap();
        Response::json(
            http::StatusCode::OK,
            json!({
                "facade_caller":facade, "direct_caller":direct, "builder_caller":builder_line,
                "cursor_caller":cursor, "transaction_caller":transactional, "outer_caller":outer,
            }),
        )
    })
}

#[tokio::test]
async fn query_watcher_attributes_pagination_queries_to_the_public_entry() {
    assert_query_source_lines("/api/page-callers", "query_page_probe", &["select "]).await;
}

#[tokio::test]
async fn query_watcher_attributes_model_writes_to_the_public_entry() {
    assert_query_source_lines(
        "/api/model-write-callers",
        "query_page_probe",
        &["insert ", "update ", "delete "],
    )
    .await;
}

#[tokio::test]
async fn query_watcher_attributes_relationship_reads_to_the_public_entry() {
    assert_query_source_lines(
        "/api/relationship-callers",
        "query_page_probe",
        &["select "],
    )
    .await;
}

#[tokio::test]
async fn query_watcher_attributes_pivot_writes_to_the_public_entry() {
    assert_query_source_lines(
        "/api/pivot-callers",
        "query_pivot_probe",
        &["select ", "insert ", "update ", "delete "],
    )
    .await;
}

async fn assert_query_source_lines(path: &str, table: &str, statements: &[&str]) {
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    QueryWatcher::new(
        json!({"ignore_packages":false})
            .as_object()
            .unwrap()
            .clone(),
    )
    .register(app.app())
    .await
    .unwrap();

    let response = app.get(path).await.unwrap();
    response.assert_ok();
    let expected = response.json::<Vec<(u64, usize)>>().unwrap();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let entries = repository
        .get(Some(EntryType::Query), EntryQueryOptions::default())
        .await
        .unwrap();
    let mut actual = std::collections::BTreeMap::new();
    for entry in entries.iter().filter(|entry| {
        let sql = entry.content["sql"].as_str().unwrap();
        statements.iter().any(|verb| sql.starts_with(verb)) && sql.contains(table)
    }) {
        assert_eq!(entry.content["file"], file!());
        *actual
            .entry(entry.content["line"].as_u64().unwrap())
            .or_insert(0) += 1;
    }
    assert_eq!(actual, expected.into_iter().collect());
}

fn gate_callers(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    use openrust::{
        auth::access::Response as Decision, contracts::auth::access::Gate as GateContract,
        support::facades::Gate,
    };

    Box::pin(async move {
        for ability in ["facade", "direct", "contract", "batch", "hidden"] {
            Gate::define_guest(ability, |_, _| Box::pin(async { Ok(Decision::from(true)) }))?;
        }
        let facade = line!() + 1;
        assert!(Gate::allows(request, "facade", &[]).await?);
        let gate = Gate::for_user(None)?;
        let direct = line!() + 1;
        assert!(gate.allows("direct", &[]).await?);
        let contract: &dyn GateContract = &gate;
        let dynamic = line!() + 1;
        assert!(contract.allows("contract", &[]).await?);
        let batch = line!() + 1;
        assert!(gate.check(&["batch", "hidden"], &[]).await?);
        Response::json(
            http::StatusCode::OK,
            json!({"facade":facade,"direct":direct,"contract":dynamic,"batch":batch}),
        )
    })
}
fn handler(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Db::connection(None)
            .unwrap()
            .select("SELECT 42 AS answer", &[])
            .await
            .unwrap();
        for name in [
            "Application\\Visible",
            "OpenRust\\Visible",
            "OpenRust\\Ignored",
        ] {
            Events::dispatch(Probe(name)).await.unwrap();
        }
        Ok(Response::from("observed"))
    })
}
fn application() -> TestApp {
    let app = TestApp::create(|builder| builder
        .with_config(|base, _| Ok(std::collections::BTreeMap::from([
            ("app".into(), json!({"env":"testing"})),
            ("auth".into(), json!({"defaults":{"guard":"gate-source"},"guards":{"gate-source":{"driver":"gate-source"}}})),
            ("database".into(), json!({"default":"test","connections":{"test":database_config(base)}})),
            ("cache".into(),json!({"default":"file","stores":{"file":{"driver":"file","path":base.join("cache"),"lock_path":base.join("locks")}}})),
            ("telescope".into(),json!({"watchers":{"QueryWatcher":false,"EventWatcher":false,"GateWatcher":false,"RequestWatcher":{"size_limit":1},"ClientRequestWatcher":true}})),
        ])))
        .with_providers(vec![Box::new(TelescopeServiceProvider)])
        .with_routing(Routing::default().api(routes))
    ).unwrap();
    std::fs::write(app.root().join("watchers.sqlite"), []).unwrap();
    app
}

#[tokio::test]
async fn gate_watcher_preserves_public_callers_and_clears_filtered_sources() {
    for (ignore_packages, ignore_paths, visible) in [
        (false, json!([]), true),
        (false, json!(["tests\\watchers.rs"]), false),
        (true, json!([]), false),
    ] {
        let mut app = application();
        app.artisan(["migrate", "--force"])
            .await
            .assert_successful();
        app.app()
            .make::<openrust::auth::AuthManager>()
            .await
            .unwrap()
            .via_request("gate-source", |_| Box::pin(async { Ok(None) }));
        GateWatcher::new(
            json!({
                "ignore_packages":ignore_packages,
                "ignore_paths":ignore_paths,
                "ignore_abilities":["hidden"],
            })
            .as_object()
            .unwrap()
            .clone(),
        )
        .register(app.app())
        .await
        .unwrap();
        let response = app.get("/api/gate-callers").await.unwrap();
        response.assert_ok();
        let expected = response.json::<serde_json::Value>().unwrap();
        let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
        let entries = repository
            .get(Some(EntryType::Gate), EntryQueryOptions::default())
            .await
            .unwrap();
        assert_eq!(entries.len(), 4);
        for entry in entries {
            let ability = entry.content["ability"].as_str().unwrap();
            assert!(expected.get(ability).is_some());
            assert_eq!(entry.content["result"], "allowed");
            if visible {
                assert_eq!(entry.content["file"], file!());
                assert_eq!(entry.content["line"], expected[ability]);
            } else {
                assert!(entry.content["file"].is_null());
                assert!(entry.content["line"].is_null());
            }
        }
        repository.clear().await.unwrap();
    }
}
#[tokio::test]
async fn query_watcher_uses_the_nearest_eligible_entry_across_nested_queries() {
    for (ignore_packages, ignore_paths, inner_skipped) in [
        (false, json!([]), false),
        (false, json!(["support/query_caller.rs"]), true),
        (true, json!([]), false),
    ] {
        let mut app = application();
        app.artisan(["migrate", "--force"])
            .await
            .assert_successful();
        QueryWatcher::new(
            json!({
                "ignore_packages":ignore_packages, "ignore_paths":ignore_paths, "slow":0,
            })
            .as_object()
            .unwrap()
            .clone(),
        )
        .register(app.app())
        .await
        .unwrap();
        let response = app.get("/api/query-callers").await.unwrap();
        response.assert_ok();
        let expected = response.json::<serde_json::Value>().unwrap();
        let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
        let entries = repository
            .get(Some(EntryType::Query), EntryQueryOptions::default())
            .await
            .unwrap();
        let queries: Vec<_> = entries
            .iter()
            .filter(|entry| {
                entry.content["sql"]
                    .as_str()
                    .is_some_and(|sql| sql.contains("_caller"))
            })
            .collect();
        if ignore_packages {
            assert!(queries.is_empty());
        } else {
            assert_eq!(
                queries.len(),
                7,
                "{:?}",
                queries
                    .iter()
                    .map(|entry| &entry.content)
                    .collect::<Vec<_>>()
            );
            let expected = expected.as_object().unwrap();
            for entry in queries {
                let sql = entry.content["sql"].as_str().unwrap();
                if sql.contains("inner_caller") && !inner_skipped {
                    assert!(
                        entry.content["file"]
                            .as_str()
                            .unwrap()
                            .ends_with("support/query_caller.rs")
                    );
                    assert!(entry.content["line"].as_u64().unwrap() > 0);
                } else {
                    assert_eq!(entry.content["file"], file!());
                    let line = if sql.contains("inner_caller") {
                        &expected["outer_caller"]
                    } else {
                        expected
                            .iter()
                            .find(|(marker, _)| sql.contains(marker.as_str()))
                            .unwrap()
                            .1
                    };
                    assert_eq!(&entry.content["line"], line);
                }
                assert_eq!(entry.content["slow"], true);
            }
        }
        repository.clear().await.unwrap();
    }
}

#[tokio::test]
async fn independent_watchers_use_own_options_and_framework_event_switch_is_application_local() {
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    app.get("/api/watchers").await.unwrap().assert_ok();
    assert!(
        repository
            .get(Some(EntryType::Query), EntryQueryOptions::default())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        repository
            .get(Some(EntryType::Event), EntryQueryOptions::default())
            .await
            .unwrap()
            .is_empty()
    );
    QueryWatcher::new(
        json!({"slow":0,"ignore_packages":false})
            .as_object()
            .unwrap()
            .clone(),
    )
    .register(app.app())
    .await
    .unwrap();
    EventWatcher::new(
        json!({"ignore":"OpenRust\\Ignored"})
            .as_object()
            .unwrap()
            .clone(),
    )
    .register(app.app())
    .await
    .unwrap();
    let disabled = QueryWatcher::new(json!({"enabled":false}).as_object().unwrap().clone());
    disabled.register(app.app()).await.unwrap();
    app.get("/api/watchers").await.unwrap().assert_ok();
    let entries = repository
        .get(Some(EntryType::Event), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].content["name"], "Application\\Visible");
    let telescope = app.app().make::<Telescope>().await.unwrap();
    telescope
        .ignore_framework_events
        .store(false, std::sync::atomic::Ordering::Release);
    app.get("/api/watchers").await.unwrap().assert_ok();
    let entries = repository
        .get(Some(EntryType::Event), EntryQueryOptions::default())
        .await
        .unwrap();
    assert!(
        entries
            .iter()
            .any(|entry| entry.content["name"] == "OpenRust\\Visible")
    );
    assert!(
        entries
            .iter()
            .all(|entry| entry.content["name"] != "OpenRust\\Ignored")
    );
    let queries = repository
        .get(Some(EntryType::Query), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(
        queries
            .iter()
            .filter(|entry| entry.content["sql"] == "SELECT 42 AS answer")
            .count(),
        2
    );
    assert!(queries.iter().all(|entry| entry.content["slow"] == true));
    let mut other = application();
    other.boot().await.unwrap();
    assert!(
        other
            .app()
            .make::<Telescope>()
            .await
            .unwrap()
            .ignore_framework_events
            .load(std::sync::atomic::Ordering::Acquire)
    );
}

fn response_handler(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        use http::{HeaderName, HeaderValue};
        use openrust::http::{Body, StatusCode};
        match request.path() {
            "api/json" => Response::json(
                StatusCode::OK,
                json!({"secret":"hidden","nested":{"password":"hidden"},"actual":true}),
            ),
            "api/plain" => Ok(Response::from("好".repeat(999)).header(
                HeaderName::from_static("content-type"),
                HeaderValue::from_static("text/plain"),
            )),
            "api/boundary" => Ok(Response::from("好".repeat(1000)).header(
                HeaderName::from_static("content-type"),
                HeaderValue::from_static("text/plain"),
            )),
            "api/large" => Ok(Response::from("好".repeat(1001)).header(
                HeaderName::from_static("content-type"),
                HeaderValue::from_static("text/plain"),
            )),
            _ => Ok(Response::new(StatusCode::OK, Body::from("opaque"))),
        }
    })
}
#[tokio::test]
async fn request_observes_buffered_content_redaction_character_limits_and_route_metadata() {
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    app.app()
        .make::<Telescope>()
        .await
        .unwrap()
        .hide_response_parameters(&["secret", "nested.password"]);
    for path in ["/json", "/plain", "/boundary", "/large", "/opaque"] {
        app.get(&format!("/api{path}")).await.unwrap().assert_ok();
    }
    let entries = app
        .app()
        .make::<DatabaseEntriesRepository>()
        .await
        .unwrap()
        .get(Some(EntryType::Request), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(entries.len(), 5);
    for entry in entries {
        assert!(
            entry.content["controller_action"]
                .as_str()
                .unwrap()
                .ends_with("response_handler")
        );
        assert!(entry.content["middleware"].is_array());
        match entry.content["uri"].as_str().unwrap() {
            "/api/json" => assert_eq!(
                entry.content["response"],
                json!({"secret":"********","nested":{"password":"********"},"actual":true})
            ),
            "/api/plain" => assert_eq!(entry.content["response"], "好".repeat(999)),
            "/api/boundary" => assert_eq!(entry.content["response"], "好".repeat(1000)),
            "/api/large" => assert_eq!(entry.content["response"], "Purged By Telescope"),
            "/api/opaque" => assert_eq!(entry.content["response"], "Not Captured"),
            value => panic!("unexpected URI {value}"),
        }
    }
}

fn database_config(base: &std::path::Path) -> serde_json::Value {
    // 每个 TestApp 使用独立表前缀，避免共享验收库中的其他用例污染采集结果。
    let prefix = format!("w_{}_", &uuid::Uuid::now_v7().simple().to_string()[24..]);
    let mut connection = match std::env::var("TELESCOPE_TEST_DRIVER")
        .as_deref()
        .unwrap_or("sqlite")
    {
        "sqlite" => json!({"driver":"sqlite","database":base.join("watchers.sqlite")}),
        "pgsql" => {
            assert_eq!(
                std::env::var("OPENRUST_REQUIRE_POSTGRES").as_deref(),
                Ok("1")
            );
            json!({"driver":"pgsql","url":std::env::var("OPENRUST_TEST_POSTGRES_URL").unwrap()})
        }
        "mysql" => {
            assert_eq!(std::env::var("OPENRUST_REQUIRE_MYSQL").as_deref(), Ok("1"));
            json!({"driver":"mysql","url":std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap()})
        }
        value => panic!("unsupported isolated driver {value}"),
    };
    connection["prefix"] = prefix.into();
    connection["prefix_indexes"] = true.into();
    connection
}

fn client_routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/client/{mode}", client_handler)?;
    Ok(())
}
fn client_handler(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let url = format!("http://example.test/{}", request.route("mode").unwrap());
        let response = openrust::support::facades::Http::get(&url)
            .await
            .map_err(|error| HttpError::Internal {
                source: Box::new(error),
            })?;
        Ok(Response::from(response.status().as_u16().to_string()))
    })
}
#[tokio::test]
async fn client_watcher_handles_closed_streams_and_uses_character_kilobytes() {
    use openrust::http::client::{Factory, Response as ClientResponse};
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(|base, _| {
                Ok(std::collections::BTreeMap::from([
                    ("app".into(), json!({"env":"testing"})),
                    (
                        "database".into(),
                        json!({"default":"test","connections":{"test":database_config(base)}}),
                    ),
                    (
                        "telescope".into(),
                        json!({"watchers":{"ClientRequestWatcher":{"size_limit":1}}}),
                    ),
                ]))
            })
            .with_providers(vec![Box::new(TelescopeServiceProvider)])
            .with_routing(Routing::default().api(client_routes))
    })
    .unwrap();
    std::fs::write(app.root().join("watchers.sqlite"), []).unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let telescope = app.app().make::<Telescope>().await.unwrap();
    telescope.hide_response_parameters(&["secret"]);
    let client = app.app().make::<Factory>().await.unwrap();
    client.fake(|request| {
        Some(Ok(match request.url().path() {
            "/small" => ClientResponse::from_json(
                http::StatusCode::OK,
                &json!({"secret":"value","actual":true}),
            )
            .unwrap(),
            "/large" => ClientResponse::new(
                http::StatusCode::OK,
                [(
                    http::header::CONTENT_TYPE,
                    http::HeaderValue::from_static("text/plain"),
                )]
                .into_iter()
                .collect(),
                "好".repeat(1001).into_bytes(),
            ),
            _ => ClientResponse::new(
                http::StatusCode::OK,
                [(
                    http::HeaderName::from_static("x-close"),
                    http::HeaderValue::from_static("yes"),
                )]
                .into_iter()
                .collect(),
                b"closed",
            ),
        }))
    });
    client.global_response_middleware(|response| {
        if response.header("x-close").is_some() {
            response.close()?;
        }
        Ok(response)
    });
    for mode in ["small", "large", "closed"] {
        app.get(&format!("/api/client/{mode}"))
            .await
            .unwrap()
            .assert_ok();
    }
    let entries = app
        .app()
        .make::<DatabaseEntriesRepository>()
        .await
        .unwrap()
        .get(Some(EntryType::ClientRequest), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(entries.len(), 3);
    for entry in entries {
        assert_eq!(
            entry.content.get("duration"),
            Some(&serde_json::Value::Null)
        );
        match entry.content["uri"].as_str().unwrap() {
            "http://example.test/small" => assert_eq!(
                entry.content["response"],
                json!({"secret":"********","actual":true})
            ),
            "http://example.test/large" => {
                assert_eq!(entry.content["response"], "Purged By Telescope")
            }
            "http://example.test/closed" => {
                assert_eq!(entry.content["response"], "Stream Response")
            }
            uri => panic!("unexpected URI {uri}"),
        }
    }
}

fn client_error_handler(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        let file = tempfile::NamedTempFile::new().unwrap();
        let output = std::fs::OpenOptions::new()
            .write(true)
            .open(file.path())
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(socket.read_u8().await.unwrap());
            }
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
                .await
                .unwrap();
        });
        let result = openrust::support::facades::Http::sink(output)
            .unwrap()
            .get(&url)
            .await;
        server.await.unwrap();
        let error = result.unwrap_err();
        assert!(matches!(error, openrust::http::client::Error::Event(_)));
        let mut source: &(dyn std::error::Error + 'static) = &error;
        while let Some(inner) = source.source() {
            source = inner;
        }
        assert!(source.downcast_ref::<std::io::Error>().is_some());
        Ok(Response::from("read error propagated"))
    })
}
#[tokio::test]
async fn client_watcher_propagates_unreadable_seekable_file_errors() {
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    app.get("/api/client-error")
        .await
        .unwrap()
        .assert_see("read error propagated", true);
}
