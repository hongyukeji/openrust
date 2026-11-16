use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{extract::State, handler::Handler as _};
use openrust::database::eloquent::RelatedCollection;
use openrust::{
    contracts::{
        config::Repository, container::Container as _, foundation::Application as _,
        http::Kernel as _,
    },
    database::{
        DatabaseError, DatabaseManager, Value,
        eloquent::{
            Attributes, Collection as ModelCollection, Model, ModelMetadata, ModelState,
            casts::Attribute,
            relations::{BelongsToMany, HasMany, HasOne, Pivot},
        },
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{
        HttpError, HttpFuture, Method, Request, Response,
        resources::json::{
            Conditional, JsonResource, ModelResource, ResourceCollection, ResourceItems,
            ResourceResult, TransformsToResourceCollection, UseResource,
        },
    },
    routing::RoutingError,
    support::{ArrayKey, Collection, facades::Route},
};
use serde_json::{Map, Value as Json, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};

struct Account(ModelState);
struct IndexedAccount(ModelState);
struct Membership(ModelState);

#[allow(non_snake_case, reason = "Implements the Eloquent model contract")]
impl Model for IndexedAccount {
    fn metadata() -> &'static ModelMetadata {
        Account::metadata()
    }

    fn from_state(state: ModelState) -> Self {
        Self(state)
    }

    fn state(&self) -> &ModelState {
        &self.0
    }

    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }

    fn newCollection(models: Vec<Self>) -> ModelCollection<Self> {
        ModelCollection::fromEntries(models.into_iter().map(|model| {
            let Some(Value::Integer(id)) = model.getKey() else {
                panic!("expected account id");
            };
            (*id * 10, model)
        }))
    }
}

impl Model for Membership {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("membership")
            .incrementing(false)
            .timestamps(None, None);
        &META
    }

    fn from_state(state: ModelState) -> Self {
        Self(state)
    }

    fn state(&self) -> &ModelState {
        &self.0
    }

    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
}

impl Model for Account {
    fn attribute(name: &str) -> Option<Attribute> {
        (name == "label").then(|| Attribute::get(|_, _| Ok("account label".into())))
    }

    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("resource_accounts")
            .timestamps(None, None)
            .fillable(&["name", "password"])
            .hidden(&["password"]);
        &META
    }

    fn from_state(state: ModelState) -> Self {
        Self(state)
    }
    fn state(&self) -> &ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
}

struct AccountResource(Account);

impl From<Account> for AccountResource {
    fn from(account: Account) -> Self {
        Self(account)
    }
}

impl UseResource for Account {
    type Resource = AccountResource;
}

struct IndexedAccounts(ResourceCollection<ModelResource<IndexedAccount>>);

#[allow(non_snake_case, reason = "Implements the resource collection contract")]
impl UseResource for IndexedAccount {
    type Resource = ModelResource<Self>;

    fn resourceCollection(source: ResourceItems<Self>) -> impl JsonResource + Send {
        assert!(!source.items().is_empty());
        IndexedAccounts(ModelResource::collection(source).preserveKeys(true))
    }
}

impl JsonResource for IndexedAccounts {
    fn toArray(&self, request: &Request) -> ResourceResult<Json> {
        self.0.toArray(request)
    }

    fn wrap(&self) -> Option<&str> {
        Some("accounts")
    }

    fn with(&self, request: &Request) -> ResourceResult<Map<String, Json>> {
        let mut fields = self.0.with(request)?;
        fields.insert("source".into(), json!("indexed"));
        Ok(fields)
    }

    fn withResponse(&self, _: &Request, response: &mut Response) -> ResourceResult<()> {
        response
            .parts_mut()
            .headers
            .insert("x-resource", "indexed".parse().unwrap());
        Ok(())
    }
}

impl JsonResource for AccountResource {
    fn toArray(&self, request: &Request) -> ResourceResult<Json> {
        let mut fields = self.0.attributesToArray().map_err(internal)?;
        Conditional::insert(
            &mut fields,
            "admin",
            Conditional::when(request.headers().contains_key("x-test-admin"), || {
                json!(true)
            }),
        );
        Conditional::insert(
            &mut fields,
            "reports",
            Conditional::whenLoaded::<RelatedCollection<Account>>(&self.0, "directReports")
                .map(|accounts| json!(accounts.len())),
        );
        Conditional::insert(
            &mut fields,
            "report_count",
            Conditional::whenCounted(&self.0, "directReports").map(|count| match count {
                Value::Integer(value) => json!(value),
                Value::Unsigned(value) => json!(value),
                _ => panic!("expected an integer count"),
            }),
        );
        Ok(Json::Object(fields))
    }

    fn with(&self, _: &Request) -> ResourceResult<Map<String, Json>> {
        Ok(Map::from_iter([(
            "meta".into(),
            json!({"source":"resource"}),
        )]))
    }

    fn withResponse(&self, _: &Request, response: &mut Response) -> ResourceResult<()> {
        response
            .parts_mut()
            .headers
            .insert("x-resource", "account".parse().unwrap());
        Ok(())
    }
}

struct Plain(Json);

struct Preserved(Json);

#[allow(non_snake_case, reason = "Implements the JSON resource contract")]
impl JsonResource for Preserved {
    fn toArray(&self, _: &Request) -> ResourceResult<Json> {
        Ok(self.0.clone())
    }

    fn preservesKeys() -> bool {
        true
    }
}

impl JsonResource for Plain {
    fn toArray(&self, _: &Request) -> ResourceResult<Json> {
        Ok(self.0.clone())
    }
}

fn internal(source: DatabaseError) -> HttpError {
    if matches!(source, DatabaseError::ModelNotFound(_)) {
        return HttpError::NotFound;
    }
    HttpError::Internal {
        source: Box::new(source),
    }
}

fn routes() -> Result<(), RoutingError> {
    Route::get("/one", || async {
        let account = Account::query()
            .map_err(internal)?
            .with(
                "directReports",
                HasMany::<Account, Account>::new("id", "manager_id"),
            )
            .withCount(
                "directReports",
                &HasMany::<Account, Account>::new("id", "manager_id"),
            )
            .map_err(internal)?
            .findOrFail(1)
            .await
            .map_err(internal)?;
        Ok(account.toResource())
    })?;
    Route::get("/missing", || async {
        let account = Account::findOrFail(99).await.map_err(internal)?;
        Ok(account.toResource())
    })?;
    Route::get("/list", || async {
        let accounts = Account::query()
            .map_err(internal)?
            .orderBy("id", "asc")
            .get()
            .await
            .map_err(internal)?;
        Ok(accounts.toResourceCollection())
    })?;
    Route::get("/page", page)?;
    Route::get("/simple", || async {
        let page = Account::query()
            .map_err(internal)?
            .simplePaginate(2)
            .await
            .map_err(internal)?;
        Ok(page.toResourceCollection())
    })?;
    Route::get("/cursor", || async {
        let page = Account::query()
            .map_err(internal)?
            .cursorPaginate(2)
            .await
            .map_err(internal)?;
        Ok(page.toResourceCollectionWith::<AccountResource>())
    })?;
    Route::get("/empty", || ResourceCollection::new(Vec::<Plain>::new()))?;
    Route::get("/already", || Plain(json!({"data":{"id":9}})))?;
    Route::get("/unwrapped", unwrapped)?;
    Route::get("/metadata", metadata)?;
    Route::get("/conditional", conditional)?;
    Route::get("/pivots", pivots)?;
    keyed_routes()?;
    resolved_routes()?;
    Route::get("/failed", || -> ResourceResult<Plain> {
        Err(HttpError::InvalidRequest)
    })?;
    Route::r#match(&[Method::POST], "/new", || async {
        let new = Account::create(Attributes::from([
            ("name".into(), "New".into()),
            ("password".into(), "hidden".into()),
        ]))
        .await
        .map_err(internal)?;
        Ok(new.toResourceWith::<ModelResource<_>>())
    })?;
    Ok(())
}

fn resolved_routes() -> Result<(), RoutingError> {
    Route::get("/resolved", || async {
        let models = IndexedAccount::query()
            .map_err(internal)?
            .orderBy("id", "asc")
            .get()
            .await
            .map_err(internal)?;
        Ok(models.toResourceCollection())
    })?;
    Route::get("/resolved/page", || async {
        let page = IndexedAccount::query()
            .map_err(internal)?
            .paginate(2)
            .await
            .map_err(internal)?;
        Ok(page.toResourceCollection())
    })?;
    Route::get("/resolved/explicit", || async {
        let models = IndexedAccount::query()
            .map_err(internal)?
            .orderBy("id", "asc")
            .get()
            .await
            .map_err(internal)?;
        Ok(models.toResourceCollectionWith::<ModelResource<_>>())
    })?;
    Route::get("/resolved/empty", || {
        ModelCollection::<IndexedAccount>::new(Vec::new()).toResourceCollection()
    })?;
    Route::get("/resolved/empty-custom", || {
        ModelCollection::<IndexedAccount>::new(Vec::new())
            .toResourceCollectionUsing(|source| IndexedAccounts(ModelResource::collection(source)))
    })?;
    Ok(())
}

fn keyed_routes() -> Result<(), RoutingError> {
    let input = || {
        Collection::fromEntries([
            ("01", json!("first")),
            ("7", json!("second")),
            ("1e2", json!(null)),
        ])
    };
    Route::get("/keys/default", move || {
        ResourceCollection::new(ResourceItems::from(input()).map(Plain))
    })?;
    Route::get("/keys/preserved", move || {
        ResourceCollection::new(ResourceItems::from(input()).map(Preserved))
    })?;
    Route::get("/keys/reset", move || {
        ResourceCollection::new(ResourceItems::from(input()).map(Preserved)).preserveKeys(false)
    })?;
    Route::get("/keys/empty", || {
        ResourceCollection::new(Collection::<Plain>::new()).preserveKeys(true)
    })?;
    Route::get("/keys/mixed", || async {
        let mut accounts = Account::query()
            .map_err(internal)?
            .orderBy("id", "asc")
            .get()
            .await
            .map_err(internal)?
            .into_iter();
        let items = ModelCollection::fromEntries([
            (ArrayKey::from("team"), accounts.next().unwrap()),
            (ArrayKey::from(9_i64), accounts.next().unwrap()),
        ]);
        Ok(ResourceCollection::new(
            ResourceItems::from(items).map(AccountResource),
        ))
    })?;
    Route::get("/keys/related", || async {
        let owner = Account::query()
            .map_err(internal)?
            .with(
                "directReports",
                HasMany::<Account, IndexedAccount>::new("id", "manager_id"),
            )
            .findOrFail(1)
            .await
            .map_err(internal)?;
        let reports = owner
            .getRelation::<RelatedCollection<IndexedAccount>>("directReports")
            .unwrap()
            .clone();
        let items = ResourceItems::from(reports)
            .map(|account| Plain(account.toArray().map(Json::Object).unwrap()));
        Ok(ResourceCollection::new(items).preserveKeys(true))
    })?;
    Route::get("/keys/page", || async {
        let page = IndexedAccount::query()
            .map_err(internal)?
            .paginate(2)
            .await
            .map_err(internal)?;
        Ok(
            ResourceCollection::new(ResourceItems::from(page).map(ModelResource))
                .preserveKeys(true),
        )
    })?;
    Route::get("/keys/simple", || async {
        let page = IndexedAccount::query()
            .map_err(internal)?
            .simplePaginate(2)
            .await
            .map_err(internal)?;
        Ok(
            ResourceCollection::new(ResourceItems::from(page).map(ModelResource))
                .preserveKeys(true),
        )
    })?;
    Route::get("/keys/cursor", || async {
        let page = IndexedAccount::query()
            .map_err(internal)?
            .cursorPaginate(2)
            .await
            .map_err(internal)?;
        Ok(
            ResourceCollection::new(ResourceItems::from(page).map(ModelResource))
                .preserveKeys(true),
        )
    })?;
    Ok(())
}

fn conditional(request: &mut Request) -> HttpFuture<'_, ResourceResult<Plain>> {
    Box::pin(async move {
        let load = request.query(Some("load"))?.is_some();
        let id = if request.query(Some("empty"))?.is_some() {
            3
        } else {
            1
        };
        let mut query = Account::query().map_err(internal)?;
        if load {
            query = query.with(
                "firstReport",
                HasOne::<Account, Account>::new("id", "manager_id"),
            );
        }
        let mut model = query.findOrFail(id).await.map_err(internal)?;
        if load {
            model.append(&["label"]);
        }
        let mut fields = Map::from_iter([("status".into(), json!("base"))]);
        Conditional::mergeWhen(&mut fields, false, || panic!("skipped merge ran"));
        Conditional::mergeUnless(&mut fields, load, || {
            Map::from_iter([
                ("status".into(), json!("override")),
                ("guest".into(), json!(true)),
            ])
        });
        Conditional::insert(
            &mut fields,
            "appended",
            Conditional::whenAppended(&model, "label", || {
                model
                    .attributesToArray()
                    .map(|values| values["label"].clone())
            })
            .transpose()
            .map_err(internal)?,
        );
        let related = Conditional::whenLoadedOne::<Account, _>(&model, "firstReport", |report| {
            assert!(id != 3, "projection of a loaded null relation");
            report.toArray().map(|values| values["name"].clone())
        });
        let related = related
            .map(|value| value.transpose())
            .transpose()
            .map_err(internal)?;
        Conditional::insert(
            &mut fields,
            "report",
            related.map(|value| value.unwrap_or(Json::Null)),
        );
        Conditional::insert(
            &mut fields,
            "known_null",
            Conditional::whenNull(&Json::Null, || Json::Null),
        );
        Ok(Plain(Json::Object(fields)))
    })
}

async fn pivots() -> ResourceResult<Plain> {
    const TABLE: &str = "resource_memberships";
    let pivot = || {
        BelongsToMany::<Account, Account>::new(TABLE, "owner_id", "member_id", "id", "id")
            .withPivot(["level"])
    };
    let owner = Account::query()
        .map_err(internal)?
        .with("members", pivot())
        .with(
            "subscribers",
            pivot().r#as("membership").using::<Membership>(),
        )
        .findOrFail(1)
        .await
        .map_err(internal)?;
    let member = owner
        .getRelation::<RelatedCollection<Account>>("members")
        .unwrap()[0]
        .as_ref();
    let subscriber = owner
        .getRelation::<RelatedCollection<Account>>("subscribers")
        .unwrap()[0]
        .as_ref();
    let mut fields = Map::new();
    Conditional::insert(
        &mut fields,
        "ordinary",
        Conditional::whenPivotLoaded(member, TABLE, || {
            let row = member.getRelation::<Pivot>("pivot").unwrap();
            assert_eq!(row.getTable(), TABLE);
            level(row.getAttribute("level"))
        }),
    );
    Conditional::insert(
        &mut fields,
        "custom",
        Conditional::whenPivotLoadedAs(subscriber, "membership", TABLE, || json!(true)),
    );
    Conditional::insert(
        &mut fields,
        "typed",
        Conditional::whenPivotLoadedAsType::<Membership, _>(subscriber, "membership", |row| {
            assert_eq!(row.getTable(), TABLE);
            level(row.getAttribute("level"))
        }),
    );
    for (name, value) in [
        (
            "wrong_table",
            Conditional::whenPivotLoadedAs(subscriber, "membership", "wrong", || {
                panic!("wrong table matched")
            }),
        ),
        (
            "wrong_accessor",
            Conditional::whenPivotLoaded(subscriber, TABLE, || panic!("wrong accessor matched")),
        ),
        (
            "wrong_type",
            Conditional::whenPivotLoadedAsType::<Account, Json>(subscriber, "membership", |_| {
                panic!("wrong type matched")
            }),
        ),
        (
            "unloaded",
            Conditional::whenPivotLoaded(&owner, TABLE, || panic!("unloaded pivot matched")),
        ),
    ] {
        Conditional::insert(&mut fields, name, value);
    }
    Ok(Plain(Json::Object(fields)))
}

fn level(value: Option<&Value>) -> Json {
    let Some(Value::Text(level)) = value else {
        panic!("expected membership level")
    };
    json!(level)
}

fn page(request: &mut Request) -> HttpFuture<'_, ResourceResult<Response>> {
    Box::pin(async move {
        let page = Account::query()
            .map_err(internal)?
            .paginate(2)
            .await
            .map_err(internal)?;
        ResourceCollection::new(ResourceItems::from(page).map(AccountResource))
            .preserveQuery()
            .withPaginationInformation(|_, mut info| {
                info.meta.insert("source".into(), json!("page"));
                Ok(info)
            })
            .response()
            .withoutWrapping()
            .additional(Map::from_iter([(
                "meta".into(),
                json!({"request":"accepted"}),
            )]))
            .toResponse(request)
    })
}

fn unwrapped(request: &mut Request) -> HttpFuture<'_, ResourceResult<Response>> {
    Box::pin(async move {
        Plain(json!({"id":1}))
            .response()
            .withoutWrapping()
            .toResponse(request)
    })
}

fn metadata(request: &mut Request) -> HttpFuture<'_, ResourceResult<Response>> {
    Box::pin(async move {
        Plain(json!({"id":1}))
            .response()
            .withoutWrapping()
            .additional(Map::from_iter([(
                "meta".into(),
                json!({"tags":["a"],"version":1}),
            )]))
            .additional(Map::from_iter([(
                "meta".into(),
                json!({"tags":["b"],"version":2}),
            )]))
            .toResponse(request)
    })
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
}

async fn request(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: &str,
    status: u16,
) -> (String, Json) {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nConnection: close\r\n{headers}\r\n").as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with(&format!("HTTP/1.1 {status} ")), "{path} {response}");
        let (header, body) = response.split_once("\r\n\r\n").unwrap();
        (header.to_owned(), serde_json::from_str(body).unwrap())
    }).await.unwrap()
}

async fn verify(config: Json) {
    let mysql = config.get("url").is_some();
    let mut app = Application::configure(std::env::temp_dir())
        .withRouting(Routing::default().api(routes))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let repository = app.container().get::<dyn Repository>("config").unwrap();
    repository
        .set("database.default", json!("resource_test"))
        .unwrap();
    repository
        .set("database.connections.resource_test", config)
        .unwrap();
    let connection = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    let id = if mysql {
        "BIGINT AUTO_INCREMENT PRIMARY KEY"
    } else {
        "INTEGER PRIMARY KEY"
    };
    for sql in [
        "DROP TABLE IF EXISTS resource_accounts".into(),
        "DROP TABLE IF EXISTS resource_memberships".into(),
        format!("CREATE TABLE resource_accounts (id {id}, manager_id BIGINT, name VARCHAR(40), password VARCHAR(40))"),
        "INSERT INTO resource_accounts VALUES (1, NULL, 'A', 'secret'), (2, 1, 'B', 'secret'), (3, NULL, 'C', 'secret')".into(),
        "CREATE TABLE resource_memberships (owner_id BIGINT, member_id BIGINT, level VARCHAR(40))".into(),
        "INSERT INTO resource_memberships VALUES (1, 2, 'gold')".into(),
    ] {
        connection.statement(&sql, &[]).await.unwrap();
    }

    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    let ((headers, admin), (_, guest)) = tokio::join!(
        request(address, "GET", "/api/one", "X-Test-Admin: yes\r\n", 200),
        request(address, "GET", "/api/one", "", 200),
    );
    assert!(headers.contains("x-resource: account"));
    assert_eq!(admin["data"]["admin"], true);
    assert!(guest["data"].get("admin").is_none());
    assert!(guest["data"].get("password").is_none());
    assert_eq!(guest["data"]["manager_id"], Json::Null);
    assert_eq!(guest["data"]["reports"], 1);
    assert_eq!(guest["data"]["report_count"], 1);
    assert_eq!(guest["meta"]["source"], "resource");
    request(address, "GET", "/api/missing", "", 404).await;
    request(address, "GET", "/api/failed", "", 400).await;
    let (_, absent) = request(address, "GET", "/api/conditional", "", 200).await;
    assert_eq!(
        absent,
        json!({"data":{"status":"base","guest":true,"known_null":null}})
    );
    let (_, loaded) = request(address, "GET", "/api/conditional?load=1", "", 200).await;
    assert_eq!(
        loaded,
        json!({"data":{"status":"base","appended":"account label","report":"B","known_null":null}})
    );
    let (_, empty) = request(address, "GET", "/api/conditional?load=1&empty=1", "", 200).await;
    assert!(empty["data"].get("report").is_some_and(Json::is_null));
    let (_, pivot) = request(address, "GET", "/api/pivots", "", 200).await;
    assert_eq!(
        pivot,
        json!({"data":{"ordinary":"gold","custom":true,"typed":"gold"}})
    );

    let (_, list) = request(address, "GET", "/api/list", "", 200).await;
    assert_eq!(list["data"].as_array().unwrap().len(), 3);
    assert!(list["data"][0].get("reports").is_none());
    assert!(list.get("meta").is_none());
    assert_eq!(
        request(address, "GET", "/api/empty", "", 200).await.1,
        json!({"data":[]})
    );
    assert_eq!(
        request(address, "GET", "/api/already", "", 200).await.1,
        json!({"data":{"id":9}})
    );
    assert_eq!(
        request(address, "GET", "/api/unwrapped", "", 200).await.1,
        json!({"id":1})
    );

    assert_eq!(
        request(address, "GET", "/api/metadata", "", 200).await.1,
        json!({"data":{"id":1},"meta":{"tags":["a","b"],"version":[1,2]}})
    );
    let (_, page) = request(address, "GET", "/api/page?filter=all", "", 200).await;
    assert_eq!(page["data"].as_array().unwrap().len(), 2);
    assert_eq!(page["links"]["next"], "/api/page?filter=all&page=2");
    assert_eq!(page["meta"]["total"], 3);
    assert_eq!(page["meta"]["source"], "page");
    assert_eq!(page["meta"]["request"], "accepted");
    assert!(page["meta"].get("next_page_url").is_none());
    assert_eq!(
        request(address, "GET", "/api/page?page=99", "", 200)
            .await
            .1["data"],
        json!([])
    );
    let (_, simple) = request(address, "GET", "/api/simple", "", 200).await;
    assert!(simple["meta"].get("total").is_none());
    let (_, cursor) = request(address, "GET", "/api/cursor", "", 200).await;
    assert!(cursor["meta"]["next_cursor"].is_string());
    let (_, last) = request(
        address,
        "GET",
        cursor["links"]["next"].as_str().unwrap(),
        "",
        200,
    )
    .await;
    assert_eq!(last["data"][0]["id"], 3);
    assert!(last["links"]["next"].is_null());
    let (_, created) = request(address, "POST", "/api/new", "", 201).await;
    assert_eq!(created["data"]["name"], "New");
    assert!(created["data"].get("password").is_none());
    verify_collection_keys(address).await;
    verify_resolved_resources(address).await;
    server.abort();
    connection
        .statement("DROP TABLE resource_accounts", &[])
        .await
        .unwrap();
    connection
        .statement("DROP TABLE resource_memberships", &[])
        .await
        .unwrap();
}

async fn verify_resolved_resources(address: SocketAddr) {
    let (headers, payload) = request(address, "GET", "/api/resolved", "", 200).await;
    assert!(headers.contains("x-resource: indexed"));
    assert_eq!(payload["source"], "indexed");
    assert_eq!(payload["accounts"]["10"]["id"], 1);
    assert!(payload["accounts"]["10"].get("password").is_none());

    let (headers, payload) = request(address, "GET", "/api/resolved/explicit", "", 200).await;
    assert!(!headers.contains("x-resource:"));
    assert_eq!(payload["data"][0]["id"], 1);
    assert!(payload.get("source").is_none());

    let (_, page) = request(address, "GET", "/api/resolved/page", "", 200).await;
    assert_eq!(page["accounts"]["20"]["id"], 2);
    assert_eq!(page["links"]["next"], "/api/resolved/page?page=2");
    assert!(page["meta"]["total"].as_u64().unwrap() >= 3);
    let (headers, page) = request(address, "GET", "/api/resolved/page?page=99", "", 200).await;
    assert!(!headers.contains("x-resource:"));
    assert_eq!(page["data"], json!([]));
    assert_eq!(page["meta"]["current_page"], 99);
    assert!(page.get("source").is_none());
    assert_eq!(
        request(address, "GET", "/api/resolved/empty", "", 200)
            .await
            .1,
        json!({"data":[]})
    );
    let (headers, empty) = request(address, "GET", "/api/resolved/empty-custom", "", 200).await;
    assert!(headers.contains("x-resource: indexed"));
    assert_eq!(empty, json!({"accounts":[],"source":"indexed"}));
}

async fn verify_collection_keys(address: SocketAddr) {
    for path in ["/api/keys/default", "/api/keys/reset"] {
        let (_, response) = request(address, "GET", path, "", 200).await;
        assert_eq!(response["data"], json!(["first", "second", null]));
    }
    let (_, response) = request(address, "GET", "/api/keys/preserved", "", 200).await;
    let data = response["data"].as_object().unwrap();
    assert_eq!(
        data.keys().map(String::as_str).collect::<Vec<_>>(),
        ["01", "7", "1e2"]
    );
    assert_eq!(data["01"], "first");
    assert_eq!(data["7"], "second");
    assert!(data["1e2"].is_null());
    let (_, empty) = request(address, "GET", "/api/keys/empty", "", 200).await;
    assert_eq!(empty, json!({"data":[]}));
    let (_, mixed) = request(address, "GET", "/api/keys/mixed", "", 200).await;
    assert_eq!(mixed["data"]["team"]["id"], 1);
    assert_eq!(mixed["data"]["9"]["id"], 2);
    let (_, related) = request(address, "GET", "/api/keys/related", "", 200).await;
    assert_eq!(related["data"]["20"]["id"], 2);
    for path in ["/api/keys/page", "/api/keys/simple"] {
        let (_, page) = request(address, "GET", path, "", 200).await;
        assert_eq!(page["data"]["10"]["id"], 1);
        assert_eq!(page["data"]["20"]["id"], 2);
        let next = page["links"]["next"].as_str().unwrap();
        assert_eq!(next, format!("{path}?page=2"));
        let (_, last) = request(address, "GET", next, "", 200).await;
        assert_eq!(last["data"]["30"]["id"], 3);
    }
    let (_, cursor) = request(address, "GET", "/api/keys/cursor", "", 200).await;
    assert_eq!(cursor["data"][0]["id"], 1);
    let (_, later) = request(
        address,
        "GET",
        cursor["links"]["next"].as_str().unwrap(),
        "",
        200,
    )
    .await;
    assert_eq!(later["data"][0]["id"], 3);
    let (_, previous) = request(
        address,
        "GET",
        later["links"]["prev"].as_str().unwrap(),
        "",
        200,
    )
    .await;
    assert_eq!(previous["data"], cursor["data"]);
}

#[tokio::test]
async fn resource_responses_on_sqlite() {
    verify(json!({"driver":"sqlite","database":":memory:"})).await;
}

#[tokio::test]
async fn resource_responses_on_mysql() {
    let url = std::env::var("OPENRUST_TEST_ELOQUENT_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_ELOQUENT_MYSQL").is_none());
    if let Some(url) = url {
        verify(json!({"url":url})).await;
    }
}
