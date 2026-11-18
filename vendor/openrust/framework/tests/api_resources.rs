use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{extract::State, handler::Handler as _};
use openrust::database::eloquent::RelatedCollection;
use openrust::{
    contracts::{
        config::Repository, container::Container as _, foundation::Application as _,
        http::Kernel as _, support::Responsable,
    },
    database::{
        Connection, DatabaseError, DatabaseManager, Value,
        eloquent::{
            Attributes, Collection as ModelCollection, Model, ModelMetadata, ModelState,
            NamedRelations,
            casts::Attribute,
            relations::{BelongsToMany, HasMany, HasOne, Pivot},
        },
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{
        HttpError, HttpFuture, Method, Request, Response,
        resources::{
            json::{
                Conditional, JsonOptions, JsonResource, ModelResource, ResourceCollection,
                ResourceItems, ResourceResponder, ResourceResult, ResourceWrapping,
                TransformsToResourceCollection, UseResource,
            },
            json_api::{JsonApi, JsonApiAttributes, JsonApiRelationships, JsonApiResource},
        },
    },
    pagination::{LengthAwarePaginator, PaginatorOptions},
    routing::{ControllerDispatcher, RoutingError},
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
    fn namedRelations() -> NamedRelations<Self> {
        NamedRelations::default().nested("directReports", || {
            HasMany::<Self, Self>::new("id", "manager_id")
        })
    }

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

    fn jsonOptions(&self) -> JsonOptions {
        JsonOptions::default().prettyPrint()
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

struct PersonResource {
    account: Account,
    use_alias: bool,
    ignore_query: bool,
    include_loaded: bool,
}

impl From<Account> for PersonResource {
    fn from(account: Account) -> Self {
        Self {
            account,
            use_alias: false,
            ignore_query: false,
            include_loaded: false,
        }
    }
}

#[allow(non_snake_case, reason = "Implements the JSON:API contract")]
impl JsonApiResource for PersonResource {
    type Model = Account;

    fn model(&self) -> &Self::Model {
        &self.account
    }

    fn modelMut(&mut self) -> Option<&mut Self::Model> {
        Some(&mut self.account)
    }

    fn includePath(path: &str) -> Option<&'static str> {
        match path {
            "reports" | "visibleReports" => Some("directReports"),
            "reports.reports" | "visibleReports.reports" => Some("directReports.directReports"),
            _ => None,
        }
    }

    fn respectFieldsAndIncludesInQueryString(&self) -> bool {
        !self.ignore_query
    }

    fn includePreviouslyLoadedRelationships(&self) -> bool {
        self.include_loaded
    }

    fn toId(&self, _: &Request) -> Option<String> {
        self.use_alias.then(|| "profile-1".into())
    }

    fn toType(&self, _: &Request) -> Option<String> {
        self.use_alias.then(|| "profiles".into())
    }

    fn lazyAttributes<'a>(&'a self, request: &'a Request) -> JsonApiAttributes<'a> {
        JsonApiAttributes::default().attribute("score", || {
            if request.headers().contains_key("x-fail-lazy") {
                return Err(HttpError::InvalidRequest);
            }
            Ok(json!(0))
        })
    }

    fn toRelationships<'a>(&'a self, request: &'a Request) -> JsonApiRelationships<'a> {
        JsonApiRelationships::default()
            .toMany("reports", || {
                if request.headers().contains_key("x-fail-relationship") {
                    return Err(HttpError::InvalidRequest);
                }
                Ok(self
                    .account
                    .getRelation::<RelatedCollection<Account>>("directReports")
                    .into_iter()
                    .flatten()
                    .map(|account| {
                        PersonResource::make(Account::from_state(account.state().clone()))
                    })
                    .collect())
            })
            .toCollection("visibleReports", || {
                if request.headers().contains_key("x-fail-collection") {
                    return Err(HttpError::InvalidRequest);
                }
                let reports: Vec<_> = self
                    .account
                    .getRelation::<RelatedCollection<Account>>("directReports")
                    .into_iter()
                    .flatten()
                    .filter(|account| {
                        !request.headers().contains_key("x-empty-collection")
                            && !matches!(account.getAttribute("name"), Some(Value::Text(name)) if name == "Private")
                    })
                    .map(|account| PersonResource::make(Account::from_state(account.state().clone())))
                    .collect();
                Ok(ResourceCollection::new(reports))
            })
            .toOne::<PersonResource>("manager", || Ok(None))
            .toOne("thing", || Ok(Some(thing(json!(99)))))
    }

    fn toLinks(&self, _: &Request) -> ResourceResult<Map<String, Json>> {
        Ok(Map::from_iter([("self".into(), json!("/api/person"))]))
    }

    fn toMeta(&self, _: &Request) -> ResourceResult<Map<String, Json>> {
        Ok(Map::from_iter([("verified".into(), json!(false))]))
    }
}

struct FractionalResource(f64);

#[allow(non_snake_case, reason = "Implements the JSON resource contract")]
impl JsonResource for FractionalResource {
    fn toArray(&self, _: &Request) -> ResourceResult<Json> {
        Ok(json!({"reading_time":self.0,"path":"/café/😀"}))
    }

    fn defaultJsonOptions() -> JsonOptions {
        JsonOptions::default()
            .prettyPrint()
            .escapeSlashes()
            .escapeUnicode()
    }

    fn withResponse(&self, _: &Request, response: &mut Response) -> ResourceResult<()> {
        response
            .parts_mut()
            .headers
            .insert("x-resource", "fractional".parse().unwrap());
        Ok(())
    }
}

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
    json_api_routes()?;
    Route::get("/encoded/single", || FractionalResource(3.0))?;
    Route::get("/encoded/list", || {
        ResourceCollection::new(vec![FractionalResource(3.0)])
    })?;
    Route::get("/encoded/page", || {
        encoded_page(vec![FractionalResource(3.0)], 1)
    })?;
    Route::get("/encoded/empty", || encoded_page(Vec::new(), 99))?;
    Route::get("/encoded/document", encoded_document)?;
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

struct ThingResource(Map<String, Json>);

impl JsonApiResource for ThingResource {
    type Model = Map<String, Json>;

    fn model(&self) -> &Self::Model {
        &self.0
    }
}

fn thing(id: Json) -> ThingResource {
    ThingResource(Map::from_iter([
        ("id".into(), id),
        ("name".into(), json!("test")),
        ("type".into(), json!("reserved")),
    ]))
}

fn json_api_routes() -> Result<(), RoutingError> {
    Route::get("/thing", || thing(json!(99)))?;
    Route::get("/thing/list", || {
        ResourceCollection::new(vec![thing(json!(99)), thing(json!("remote-id"))])
    })?;
    Route::get("/thing/invalid", || thing(json!(true)))?;
    Route::get("/person/auto", || async {
        let person = Account::findOrFail(1).await.map_err(internal)?;
        Ok(PersonResource::make(person))
    })?;
    Route::get("/person/ignore-query", || async {
        let person = Account::findOrFail(1).await.map_err(internal)?;
        Ok(PersonResource {
            ignore_query: true,
            ..PersonResource::make(person)
        })
    })?;
    Route::get("/person/include-loaded", || async {
        let person = Account::query()
            .map_err(internal)?
            .with("directReports", nested_report_relation())
            .findOrFail(1)
            .await
            .map_err(internal)?;
        Ok(PersonResource {
            include_loaded: true,
            ..PersonResource::make(person)
        })
    })?;
    Route::get("/person/auto-page", || async {
        let page = Account::query()
            .map_err(internal)?
            .orderBy("id", "asc")
            .paginate(2)
            .await
            .map_err(internal)?;
        Ok(page.toResourceCollectionWith::<PersonResource>())
    })?;
    Route::get("/person/compound", || async {
        let person = Account::query()
            .map_err(internal)?
            .with("directReports", nested_report_relation())
            .findOrFail(1)
            .await
            .map_err(internal)?;
        Ok(PersonResource::make(person))
    })?;
    Route::get("/person/compound-list", || async {
        let mut query = Account::query()
            .map_err(internal)?
            .with("directReports", nested_report_relation())
            .orderBy("id", "asc");
        query.r#where("id", "<", 3);
        let people = query.get().await.map_err(internal)?;
        Ok(PersonResource::collection(people))
    })?;
    Route::get("/person", || async {
        let account = Account::findOrFail(1).await.map_err(internal)?;
        Ok(account.toResourceWith::<PersonResource>())
    })?;
    Route::get("/person/alias", || async {
        let account = Account::findOrFail(1).await.map_err(internal)?;
        Ok(PersonResource {
            use_alias: true,
            ..PersonResource::make(account)
        })
    })?;
    Route::get("/person/list", || async {
        let account = Account::findOrFail(1).await.map_err(internal)?;
        Ok(
            PersonResource::collection(Collection::fromEntries([("first", account)]))
                .preserveKeys(true),
        )
    })?;
    Route::get("/person/page", || async {
        let page = Account::query()
            .map_err(internal)?
            .paginate(2)
            .await
            .map_err(internal)?;
        Ok(page.toResourceCollectionWith::<PersonResource>())
    })?;
    Route::get("/person/empty", || {
        PersonResource::collection(Vec::<Account>::new())
    })?;
    Route::get("/person/invalid", || {
        PersonResource::make(Account::from_state(ModelState::default()))
    })?;
    Ok(())
}

fn nested_report_relation() -> HasMany<Account, Account> {
    HasMany::new("id", "manager_id").constrain(|query| {
        query.with(
            "directReports",
            HasMany::<Account, Account>::new("id", "manager_id"),
        )
    })
}

fn encoded_page(
    items: Vec<FractionalResource>,
    page: u64,
) -> ResourceCollection<FractionalResource> {
    ResourceCollection::new(
        LengthAwarePaginator::new(
            items,
            2,
            1,
            Some(page),
            PaginatorOptions {
                path: Some("/api/encoded/page"),
                ..PaginatorOptions::default()
            },
        )
        .unwrap(),
    )
}

fn encoded_document(request: &mut Request) -> HttpFuture<'_, ResourceResult<Response>> {
    Box::pin(async move {
        let resource = Plain(json!({"reading_time":3.0}));
        Response::json(
            openrust::http::StatusCode::OK,
            json!({
                "compact": resource.toJson(request)?,
                "pretty": resource.toPrettyJson(request)?,
            }),
        )
    })
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
        let picked = Conditional::attributes(&model, &["name", "password"])?;
        Conditional::merge(&mut fields, || picked);
        for (name, value) in [
            ("zero", json!(0)),
            ("false_value", json!(false)),
            ("blank", json!(" \t ")),
            ("empty", json!([])),
            ("empty_map", json!({})),
        ] {
            Conditional::insert(
                &mut fields,
                name,
                Conditional::transform(&value, Json::clone),
            );
        }
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
    let (header, body) = request_raw(address, method, path, headers, status).await;
    (header, serde_json::from_str(&body).unwrap())
}

async fn request_raw(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: &str,
    status: u16,
) -> (String, String) {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nConnection: close\r\n{headers}\r\n").as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with(&format!("HTTP/1.1 {status} ")), "{path} {response}");
        let (header, body) = response.split_once("\r\n\r\n").unwrap();
        (header.to_owned(), body.to_owned())
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
        "INSERT INTO resource_accounts VALUES (1, NULL, 'A', 'secret'), (2, 1, 'B', 'secret'), (3, 2, 'C', 'secret')".into(),
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
        json!({"data":{"status":"base","guest":true,"known_null":null,"name":"A","zero":0,"false_value":false}})
    );
    let (_, loaded) = request(address, "GET", "/api/conditional?load=1", "", 200).await;
    assert_eq!(
        loaded,
        json!({"data":{"status":"base","appended":"account label","report":"B","known_null":null,"name":"A","zero":0,"false_value":false}})
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
    verify_json_options(address).await;
    verify_json_api(address).await;
    verify_json_api_autoload(address, mysql.then_some(&connection)).await;
    connection.statement(
        "INSERT INTO resource_accounts VALUES (50, 1, 'Private', 'secret'), (51, 50, 'Private child', 'secret')",
        &[],
    ).await.unwrap();
    verify_json_api_relationship_collection(address).await;
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

async fn verify_json_api_relationship_collection(address: SocketAddr) {
    let path = "/api/person/auto?include=visibleReports.reports";
    let (headers, document) = request(address, "GET", path, "", 200).await;
    assert!(headers.contains("content-type: application/vnd.api+json"));
    assert_eq!(
        document["data"]["relationships"]["visibleReports"],
        json!({"data":[{"id":"2","type":"people"}]}),
    );
    let included = document["included"].as_array().unwrap();
    assert_eq!(included.len(), 2);
    assert_eq!(included[0]["id"], "2");
    assert_eq!(
        included[0]["relationships"]["reports"],
        json!({"data":[{"id":"3","type":"people"}]}),
    );
    assert_eq!(included[1]["id"], "3");
    let (_, empty) = request(address, "GET", path, "X-Empty-Collection: yes\r\n", 200).await;
    assert_eq!(
        empty["data"]["relationships"]["visibleReports"],
        json!({"data":[]})
    );
    assert!(empty.get("included").is_none());
    request(address, "GET", path, "X-Fail-Collection: yes\r\n", 400).await;
    let (_, omitted) = request(
        address,
        "GET",
        "/api/person/auto",
        "X-Fail-Collection: yes\r\n",
        200,
    )
    .await;
    assert!(omitted["data"].get("relationships").is_none());
}

async fn verify_json_api_autoload(address: SocketAddr, mysql: Option<&Connection>) {
    for (endpoint, expected_selects, expected_included) in [
        ("/api/person/auto", 1, 0),
        ("/api/person/auto?include=unknown", 1, 0),
        ("/api/person/auto?include%5B0%5D=reports.reports", 1, 0),
        (
            "/api/person/ignore-query?fields%5Bpeople%5D=name&include=reports.reports",
            1,
            0,
        ),
        (
            "/api/person/include-loaded?fields%5Bpeople%5D=name,reports&include=manager,reports.reports",
            3,
            1,
        ),
        ("/api/person/auto?include=reports.reports", 3, 2),
        ("/api/person/compound?include=reports.reports", 3, 2),
        ("/api/person/auto-page?include=reports.reports", 4, 1),
    ] {
        let before = if let Some(connection) = mysql {
            select_count(connection).await
        } else {
            0
        };
        let (headers, document) = request(address, "GET", endpoint, "", 200).await;
        assert!(headers.contains("content-type: application/vnd.api+json"));
        assert_eq!(
            document["included"].as_array().map_or(0, Vec::len),
            expected_included,
            "{endpoint}"
        );
        if expected_included > 0 {
            let last_id = if endpoint.contains("include-loaded") {
                "2"
            } else {
                "3"
            };
            assert_eq!(document["included"][expected_included - 1]["id"], last_id);
        }
        if endpoint.contains("ignore-query") {
            assert_eq!(document["data"]["attributes"]["score"], 0);
        }
        if endpoint.contains("include-loaded") {
            assert_eq!(document["data"]["attributes"], json!({"name":"A"}));
            assert!(document["data"]["relationships"].get("manager").is_none());
        }
        if endpoint.contains("auto-page") {
            assert_eq!(
                document["data"][0]["relationships"]["reports"]["data"][0]["id"],
                "2"
            );
            assert_eq!(document["meta"]["current_page"], 1);
        }
        if let Some(connection) = mysql {
            assert_eq!(
                select_count(connection).await - before,
                expected_selects,
                "{endpoint}"
            );
        }
    }
}

async fn select_count(connection: &Connection) -> u64 {
    let row = connection
        .selectOne("SHOW SESSION STATUS LIKE 'Com_select'", &[])
        .await
        .unwrap()
        .unwrap();
    let Some(Value::Text(count)) = row.get_at(1) else {
        panic!("invalid session SELECT counter")
    };
    count.parse().unwrap()
}

async fn verify_json_api(address: SocketAddr) {
    let (headers, resource) = request(address, "GET", "/api/person", "", 200).await;
    assert!(headers.contains("content-type: application/vnd.api+json"));
    assert_eq!(
        resource,
        json!({"data":{"id":"1","type":"people","attributes":{"manager_id":null,"name":"A","score":0},"links":{"self":"/api/person"},"meta":{"verified":false}}})
    );
    let (_, subset) = request(
        address,
        "GET",
        "/api/person?fields%5Bpeople%5D=name",
        "",
        200,
    )
    .await;
    assert_eq!(subset["data"]["attributes"], json!({"name":"A"}));
    let (_, omit) = request(address, "GET", "/api/person?fields%5Bpeople%5D=", "", 200).await;
    assert!(omit["data"].get("attributes").is_none());
    let (_, alias) = request(address, "GET", "/api/person/alias", "", 200).await;
    assert_eq!(alias["data"]["id"], "profile-1");
    assert_eq!(alias["data"]["type"], "profiles");
    for endpoint in ["list", "page", "empty"] {
        let (headers, body) =
            request(address, "GET", &format!("/api/person/{endpoint}"), "", 200).await;
        assert!(headers.contains("content-type: application/vnd.api+json"));
        assert!(body["data"].is_array());
        if endpoint != "empty" {
            assert_eq!(body["data"][0]["id"], "1");
        }
        if endpoint == "page" {
            assert_eq!(body["links"]["next"], "/api/person/page?page=2");
        }
    }
    request(address, "GET", "/api/person/invalid", "", 500).await;
    verify_json_api_data_sources(address).await;
    verify_json_api_includes(address).await;
}

async fn verify_json_api_data_sources(address: SocketAddr) {
    let (headers, document) = request(address, "GET", "/api/thing", "", 200).await;
    assert!(headers.contains("content-type: application/vnd.api+json"));
    let object = json!({"id":"99","type":"things","attributes":{"name":"test"}});
    assert_eq!(document, json!({"data":object}));
    let (_, list) = request(address, "GET", "/api/thing/list", "", 200).await;
    assert_eq!(list["data"][0], object);
    assert_eq!(list["data"][1]["id"], "remote-id");
    let (_, compound) = request(address, "GET", "/api/person?include=thing", "", 200).await;
    assert_eq!(
        compound["data"]["relationships"]["thing"]["data"],
        json!({"id":"99","type":"things"})
    );
    assert_eq!(compound["included"], json!([object]));

    let (_, shaped) = request(
        address,
        "GET",
        "/api/thing?fields%5Bthings%5D%5B0%5D=name&include%5B0%5D=unused",
        "",
        200,
    )
    .await;
    assert_eq!(shaped, json!({"data":{"id":"99","type":"things"}}));
    let (_, unrelated) = request(
        address,
        "GET",
        "/api/thing?fields%5Bother%5D%5B0%5D=name",
        "",
        200,
    )
    .await;
    assert_eq!(unrelated, document);
    request(address, "GET", "/api/thing/invalid", "", 500).await;
}

async fn verify_json_api_includes(address: SocketAddr) {
    let (_, sparse) = request(
        address,
        "GET",
        "/api/person?fields%5Bpeople%5D=name",
        "X-Fail-Lazy: yes\r\nX-Fail-Relationship: yes\r\n",
        200,
    )
    .await;
    assert_eq!(sparse["data"]["attributes"], json!({"name":"A"}));
    assert!(sparse["data"].get("relationships").is_none());
    request(
        address,
        "GET",
        "/api/person?fields%5Bpeople%5D=score",
        "X-Fail-Lazy: yes\r\n",
        400,
    )
    .await;
    request(
        address,
        "GET",
        "/api/person/compound?include=reports",
        "X-Fail-Relationship: yes\r\n",
        400,
    )
    .await;
    let (_, document) = request(
        address,
        "GET",
        "/api/person/compound?include=reports.reports,manager",
        "",
        200,
    )
    .await;
    assert_eq!(
        document["data"]["relationships"],
        json!({
            "reports":{"data":[{"id":"2","type":"people"}]},
            "manager":{"data":null},
        })
    );
    assert_eq!(document["included"][0]["id"], "2");
    assert_eq!(
        document["included"][0]["relationships"]["reports"]["data"][0]["id"],
        "3"
    );
    assert_eq!(document["included"][1]["id"], "3");
    let (_, sparse) = request(
        address,
        "GET",
        "/api/person/compound?fields%5Bpeople%5D=name&include=reports",
        "",
        200,
    )
    .await;
    assert!(sparse["data"].get("relationships").is_none());
    assert_eq!(sparse["included"][0]["attributes"], json!({"name":"B"}));
    let (_, collection) = request(
        address,
        "GET",
        "/api/person/compound-list?include=reports.reports",
        "",
        200,
    )
    .await;
    assert_eq!(collection["included"].as_array().unwrap().len(), 1);
    assert_eq!(collection["included"][0]["id"], "3");
    request(
        address,
        "GET",
        "/api/person/compound?include=reports.reports.reports.reports.reports.reports",
        "",
        200,
    )
    .await;
    request(
        address,
        "GET",
        "/api/person/compound?include=reports..reports",
        "",
        400,
    )
    .await;
}

async fn verify_json_options(address: SocketAddr) {
    for endpoint in ["single", "list", "page"] {
        let (headers, body) =
            request_raw(address, "GET", &format!("/api/encoded/{endpoint}"), "", 200).await;
        assert!(headers.contains("content-type: application/json"));
        assert_eq!(
            headers.contains("x-resource: fractional"),
            endpoint == "single"
        );
        assert!(body.starts_with("{\n    \"data\":"));
        assert!(body.contains(r#""reading_time": 3.0"#));
        assert!(body.contains(r#""path": "\/caf\u00e9\/\ud83d\ude00""#));
        let data: Json = serde_json::from_str(&body).unwrap();
        let item = if endpoint == "single" {
            &data["data"]
        } else {
            &data["data"][0]
        };
        assert_eq!(item["path"], "/café/😀");
        if endpoint == "page" {
            assert_eq!(data["links"]["next"], "/api/encoded/page?page=2");
            assert!(body.contains(r#""next": "\/api\/encoded\/page?page=2""#));
        }
    }
    let (_, empty) = request_raw(address, "GET", "/api/encoded/empty", "", 200).await;
    assert!(empty.starts_with("{\n    \"data\": []"));
    assert!(empty.contains(r#""path": "\/api\/encoded\/page""#));
    let (_, documents) = request(address, "GET", "/api/encoded/document", "", 200).await;
    assert_eq!(documents["compact"], "{\"reading_time\":3.0}");
    assert_eq!(documents["pretty"], "{\n    \"reading_time\": 3.0\n}");
}

async fn verify_resolved_resources(address: SocketAddr) {
    let (headers, body) = request_raw(address, "GET", "/api/resolved", "", 200).await;
    assert!(body.starts_with("{\n    \"accounts\":"));
    let payload: Json = serde_json::from_str(&body).unwrap();
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
        verify(json!({"url":url,"max_connections":1})).await;
    }
}

struct ForcedResource;

#[allow(non_snake_case, reason = "Implements the JSON resource contract")]
impl JsonResource for ForcedResource {
    fn toArray(&self, _: &Request) -> ResourceResult<Json> {
        Ok(json!({"items":{"id":1}}))
    }

    fn forceWrapping(&self) -> bool {
        true
    }
}

fn explicitly_wrapped(request: &mut Request) -> HttpFuture<'_, ResourceResult<Response>> {
    Box::pin(async move {
        Plain(json!({"id":1}))
            .response()
            .wrap("data")
            .toResponse(request)
    })
}

fn wrapping_metadata(request: &mut Request) -> HttpFuture<'_, ResourceResult<Response>> {
    Box::pin(async move {
        let resource = Plain(json!({"data":{"id":1}}));
        let response = resource
            .response()
            .additional(Map::from_iter([("meta".into(), json!({"total":1}))]));
        Responsable::toResponse(response, request).await
    })
}

#[derive(Default)]
struct PreparedResource(Option<Json>);

#[allow(non_snake_case, reason = "Implements the Laravel JsonResource API")]
impl JsonResource for PreparedResource {
    fn prepareResponse(&mut self, request: &Request) -> Option<HttpFuture<'_, ResourceResult<()>>> {
        let name = request.query(Some("name"));
        Some(Box::pin(async move {
            tokio::task::yield_now().await;
            self.0 = name?;
            Ok(())
        }))
    }

    fn toArray(&self, _: &Request) -> ResourceResult<Json> {
        Ok(json!({"name": self.0}))
    }

    fn with(&self, _: &Request) -> ResourceResult<Map<String, Json>> {
        Ok(Map::from_iter([(
            "meta".into(),
            json!({"prepared": self.0.is_some()}),
        )]))
    }

    fn withResponse(&self, _: &Request, response: &mut Response) -> ResourceResult<()> {
        response
            .parts_mut()
            .headers
            .insert("x-resource", "prepared".parse().unwrap());
        Ok(())
    }
}

struct ResourceController;

impl ResourceController {
    fn show<'a>(
        &'a self,
        _: &'a mut Request,
    ) -> HttpFuture<'a, ResourceResult<ResourceResponder<PreparedResource>>> {
        Box::pin(async { Ok(PreparedResource::default().intoResponsable()) })
    }
}

fn wrapping_routes() -> Result<(), RoutingError> {
    Route::get("/person", || {
        let mut state = ModelState::default();
        state.setAttribute("id", "external-id");
        let mut person = Account::from_state(state);
        let mut child = ModelState::default();
        child.setAttribute("id", "child");
        let mut child = Account::from_state(child);
        let mut grandchild = ModelState::default();
        grandchild.setAttribute("id", "grandchild");
        child.setRelation::<HasMany<Account, Account>>(
            "directReports",
            RelatedCollection::fromShared(vec![Arc::new(Account::from_state(grandchild))]),
        );
        person.setRelation::<HasMany<Account, Account>>(
            "directReports",
            RelatedCollection::fromShared(vec![Arc::new(child)]),
        );
        PersonResource::make(person)
    })?;
    Route::get("/person/empty", || {
        PersonResource::collection(Vec::<Account>::new())
    })?;
    Route::get("/plain", || Plain(json!({"id":1})))?;
    Route::get("/already", || Plain(json!({"items":{"id":1}})))?;
    Route::get("/forced", || ForcedResource)?;
    Route::get("/explicit", explicitly_wrapped)?;
    Route::get("/unwrapped", unwrapped)?;
    Route::get("/metadata", wrapping_metadata)?;
    Route::get("/prepared", PreparedResource::default)?;
    Route::get(
        "/prepared/controller",
        ControllerDispatcher::new(ResourceController::show),
    )?;
    Route::get("/collection", || {
        ResourceCollection::new(vec![Plain(json!({"id":1}))]).intoResponsable()
    })?;
    Route::get("/named", || {
        IndexedAccounts(ResourceCollection::new(Vec::new()))
    })?;
    Route::get("/page", || {
        let page = LengthAwarePaginator::new(
            vec![Plain(json!({"id":1}))],
            2,
            1,
            Some(1),
            PaginatorOptions::default(),
        )
        .unwrap();
        ResourceCollection::new(page)
            .preserveQuery()
            .intoResponsable()
    })?;
    Ok(())
}

async fn wrapping_server(
    wrapping: ResourceWrapping,
    json_api: JsonApi,
) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let mut app = Application::configure(std::env::temp_dir())
        .withRouting(Routing::default().api(wrapping_routes))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    app.container().instance(Arc::new(wrapping)).unwrap();
    app.container().instance(Arc::new(json_api)).unwrap();
    app.container()
        .instance(Arc::new(ResourceController))
        .unwrap();

    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    (address, task)
}

#[tokio::test]
async fn resource_wrapping_is_scoped_to_each_http_application() {
    let left_configuration = JsonApi::configure(
        Some("1.1".into()),
        vec!["https://example.test/ext".into()],
        vec![],
        Map::from_iter([("app".into(), json!("left"))]),
    )
    .maxRelationshipDepth(1);
    let right_configuration = JsonApi::configure(
        None,
        vec![],
        vec!["https://example.test/profile".into()],
        Map::from_iter([("app".into(), json!("right"))]),
    )
    .maxRelationshipDepth(2);
    let (left, left_task) =
        wrapping_server(ResourceWrapping::withoutWrapping(), left_configuration).await;
    let (right, right_task) =
        wrapping_server(ResourceWrapping::wrap("items"), right_configuration).await;
    let include_path = "/api/person?include=reports.reports.reports";
    let ((_, left_person), (_, right_person)) = tokio::join!(
        request(left, "GET", include_path, "", 200),
        request(right, "GET", include_path, "", 200),
    );
    assert_eq!(
        left_person["jsonapi"],
        json!({"version":"1.1","ext":["https://example.test/ext"],"meta":{"app":"left"}})
    );
    assert_eq!(
        right_person["jsonapi"],
        json!({"profile":["https://example.test/profile"],"meta":{"app":"right"}})
    );
    assert_eq!(left_person["included"].as_array().unwrap().len(), 1);
    assert_eq!(right_person["included"].as_array().unwrap().len(), 2);
    assert_eq!(right_person["included"][1]["id"], "grandchild");
    let (_, empty) = request(left, "GET", "/api/person/empty", "", 200).await;
    assert_eq!(empty, json!({"jsonapi":left_person["jsonapi"],"data":[]}));
    for _ in 0..2 {
        let ((_, plain), (_, wrapped)) = tokio::join!(
            request(left, "GET", "/api/plain", "", 200),
            request(right, "GET", "/api/plain", "", 200),
        );
        assert_eq!(plain, json!({"id":1}));
        assert_eq!(wrapped, json!({"items":{"id":1}}));
    }
    for address in [left, right] {
        let (headers, person) = request(address, "GET", "/api/person", "", 200).await;
        assert!(headers.contains("content-type: application/vnd.api+json"));
        assert_eq!(person["data"]["id"], "external-id");
        assert_eq!(
            request(address, "GET", "/api/explicit", "", 200).await.1,
            json!({"data":{"id":1}})
        );
        assert_eq!(
            request(address, "GET", "/api/named", "", 200).await.1,
            json!({"accounts":[],"source":"indexed"})
        );
        assert_eq!(
            request(address, "GET", "/api/unwrapped", "", 200).await.1,
            json!({"id":1})
        );
    }
    assert_eq!(
        request(right, "GET", "/api/already", "", 200).await.1,
        json!({"items":{"id":1}})
    );
    assert_eq!(
        request(left, "GET", "/api/forced", "", 200).await.1,
        json!({"items":{"id":1}})
    );
    assert_eq!(
        request(right, "GET", "/api/forced", "", 200).await.1,
        json!({"items":{"items":{"id":1}}})
    );
    let left_meta = request(left, "GET", "/api/metadata", "", 200).await.1;
    assert_eq!(
        left_meta,
        json!({"data":{"data":{"id":1}},"meta":{"total":1}})
    );
    for (address, key) in [(left, "data"), (right, "items")] {
        for path in [
            "/api/prepared?name=Ada",
            "/api/prepared/controller?name=Ada",
        ] {
            let (headers, body) = request(address, "GET", path, "", 200).await;
            assert!(headers.contains("x-resource: prepared"));
            assert_eq!(body[key], json!({"name":"Ada"}));
            assert_eq!(body["meta"], json!({"prepared":true}));
        }
        let page = request(address, "GET", "/api/page?filter=active", "", 200)
            .await
            .1;
        assert_eq!(page[key], json!([{"id":1}]));
        assert_eq!(page["meta"]["total"], 2);
        assert!(
            page["links"]["next"]
                .as_str()
                .unwrap()
                .contains("filter=active")
        );
    }
    let collection = request(right, "GET", "/api/collection", "", 200).await.1;
    assert_eq!(collection["items"], json!([{"id":1}]));
    left_task.abort();
    right_task.abort();
}
