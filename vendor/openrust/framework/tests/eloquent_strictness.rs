use futures_util::FutureExt as _;
use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    database::{
        Connection, DatabaseError, DatabaseFuture, DatabaseManager, DatabaseResult, Value,
        eloquent::{
            Attributes, Model, ModelMetadata, ModelState, Touches,
            casts::{Attribute, AttributeCast, CastableEnum, EnumCast},
            concerns::{ModelEvent, ModelEventListener, ModelEventResponse},
            relations::{BelongsTo, HasMany},
        },
    },
    foundation::Application,
};
use serde_json::{Value as Json, json};
use std::{
    any::TypeId,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
enum Status {
    On,
}
impl CastableEnum for Status {
    fn fromValue(value: &Value) -> DatabaseResult<Self> {
        if value == &Value::from("on") {
            Ok(Self::On)
        } else {
            Err(DatabaseError::InvalidQuery("枚举值错误"))
        }
    }
    fn intoValue(self) -> Value {
        "on".into()
    }
}
struct Item(ModelState);
struct Guarded(ModelState);
struct Handled(ModelState);
struct AsyncItem(ModelState);
struct Owner(ModelState);
struct OtherOwner(ModelState);
struct Child(ModelState);
macro_rules! state {
    () => {
        fn from_state(state: ModelState) -> Self {
            Self(state)
        }
        fn state(&self) -> &ModelState {
            &self.0
        }
        fn state_mut(&mut self) -> &mut ModelState {
            &mut self.0
        }
    };
}
macro_rules! read_casts {
    () => {
        fn casts() -> &'static [(&'static str, AttributeCast)] {
            static CASTS: [(&str, AttributeCast); 2] = [
                ("count", AttributeCast::Integer),
                ("status", AttributeCast::Enum(EnumCast::of::<Status>())),
            ];
            &CASTS
        }
        fn attribute(name: &str) -> Option<Attribute> {
            (name == "alias").then(|| {
                Attribute::get(|value, _| Ok(value.cloned().unwrap_or_else(|| "fallback".into())))
            })
        }
    };
}
impl Model for Item {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("parity_strict_items")
            .timestamps(None, None)
            .fillable(&["name"]);
        &META
    }
    state!();
    read_casts!();
}
impl Model for Guarded {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata =
            ModelMetadata::new("parity_strict_items").timestamps(None, None);
        &META
    }
    state!();
    read_casts!();
}
impl Model for Handled {
    fn metadata() -> &'static ModelMetadata {
        Item::metadata()
    }
    state!();
    read_casts!();
    fn handleDiscardedAttributeViolationUsing(
        &mut self,
        keys: &[String],
        _: DatabaseError,
    ) -> DatabaseResult<()> {
        self.setAttribute("discarded", keys.join(", "));
        Ok(())
    }
    fn handleMissingAttributeViolationUsing(
        &self,
        _: &str,
        _: DatabaseError,
    ) -> DatabaseResult<Option<Value>> {
        Ok(Some("provided".into()))
    }
}
impl Model for AsyncItem {
    fn metadata() -> &'static ModelMetadata {
        Item::metadata()
    }
    state!();
    fn casts() -> &'static [(&'static str, AttributeCast)] {
        &[("name", AttributeCast::Hashed)]
    }
}
impl Model for Owner {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata =
            ModelMetadata::new("parity_strict_owners").timestamps(None, Some("updated_at"));
        &META
    }
    state!();
}
impl Model for OtherOwner {
    fn metadata() -> &'static ModelMetadata {
        Owner::metadata()
    }
    state!();
}
impl Model for Child {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("parity_strict_children")
            .timestamps(None, None)
            .fillable(&["name"]);
        &META
    }
    state!();
    fn touches() -> Touches<Self> {
        Touches::default().belongsTo(BelongsTo::<Child, Owner>::new("owner_id", "id"))
    }
}
struct Saved(Arc<AtomicUsize>);
impl ModelEventListener<Owner> for Saved {
    fn handle<'a>(
        &'a self,
        _: ModelEvent,
        _: &'a mut Owner,
        _: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(ModelEventResponse::Continue)
        })
    }
}
async fn application() -> (Application, Connection) {
    let config = match std::env::var("OPENRUST_TEST_STRICT_URL") {
        Ok(url) => json!({"url":url}),
        Err(_) => {
            assert!(std::env::var_os("OPENRUST_REQUIRE_STRICT").is_none());
            json!({"driver":"sqlite","database":":memory:"})
        }
    };
    let mut app = Application::configure(std::env::temp_dir())
        .create()
        .unwrap();
    app.boot().await.unwrap();
    app.config()
        .unwrap()
        .set("database.default", json!("strict"))
        .unwrap();
    app.config()
        .unwrap()
        .set("database.connections.strict", config)
        .unwrap();
    let db = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    (app, db)
}
fn value(value: Option<&Value>) -> Json {
    match value {
        None | Some(Value::Null) => Json::Null,
        Some(Value::Integer(v)) => json!(v),
        Some(Value::Text(v)) => json!(v),
        Some(Value::Bool(v)) => json!(v),
        other => panic!("未覆盖的属性类型：{other:?}"),
    }
}
fn fill_case<M: Model>(db: &Connection, force: bool) -> Json {
    let mut model = M::newInstanceOn(db, Attributes::new()).unwrap();
    let input = Attributes::from([("name".into(), "New".into()), ("secret".into(), "x".into())]);
    let result = if force {
        model.forceFill(input)
    } else {
        model.fill(input)
    };
    let error = match result {
        Ok(_) => Json::Null,
        Err(DatabaseError::MassAssignment(_)) => json!("mass"),
        Err(e) => panic!("批量赋值错误：{e}"),
    };
    let attributes = model
        .state()
        .getAttributes()
        .iter()
        .map(|(k, v)| (k.clone(), value(Some(v))))
        .collect::<serde_json::Map<_, _>>();
    json!({"error":error,"attributes":attributes})
}
#[tokio::test]
async fn strict_attributes_and_scoped_touches_match_reference_and_isolate_applications() {
    let (app, db) = application().await;
    let id = if db.getDriverName() == "sqlite" {
        "integer primary key"
    } else if db.getDriverName() == "pgsql" {
        "bigserial primary key"
    } else {
        "bigint primary key auto_increment"
    };
    db.statement(&format!("create table parity_strict_items (id {id}, name varchar(64), count integer, nullable varchar(64))"),&[]).await.unwrap();
    for sql in [
        "insert into parity_strict_items (name,count,nullable) values ('Ada',7,null)",
        "create table parity_strict_owners (id integer primary key, updated_at varchar(64))",
        "insert into parity_strict_owners values (1,'2000-01-01 00:00:00')",
        "create table parity_strict_children (id integer primary key, owner_id integer, name varchar(64))",
        "insert into parity_strict_children values (1,1,'first')",
    ] {
        db.statement(sql, &[]).await.unwrap();
    }
    let reference: Json =
        serde_json::from_str(include_str!("reference/eloquent-strictness.json")).unwrap();
    assert_eq!(reference.as_object().unwrap().len(), 51);
    for strict in [false, true] {
        for guarded in [false, true] {
            for force in [false, true] {
                Item::preventSilentlyDiscardingAttributes(&app, strict).unwrap();
                Guarded::preventSilentlyDiscardingAttributes(&app, strict).unwrap();
                let actual = if guarded {
                    fill_case::<Guarded>(&db, force)
                } else {
                    fill_case::<Item>(&db, force)
                };
                assert_eq!(
                    actual,
                    reference[format!(
                        "fill_{}_{}_{}",
                        u8::from(strict),
                        u8::from(guarded),
                        u8::from(force)
                    )]
                );
            }
        }
    }
    Handled::preventSilentlyDiscardingAttributes(&app, true).unwrap();
    let mut handled = Handled::newInstanceOn(&db, Attributes::new()).unwrap();
    handled
        .fill(Attributes::from([
            ("name".into(), "New".into()),
            ("secret".into(), "x".into()),
        ]))
        .unwrap();
    let actual = handled
        .state()
        .getAttributes()
        .iter()
        .map(|(k, v)| (k.clone(), value(Some(v))))
        .collect::<serde_json::Map<_, _>>();
    assert_eq!(json!(actual), reference["fill_handler"]);
    Item::preventSilentlyDiscardingAttributes(&app, false).unwrap();
    for strict in [false, true] {
        for state in ["new", "existing", "recent"] {
            for key in ["unknown", "count", "alias", "nullable", "status"] {
                Item::preventAccessingMissingAttributes(&app, strict).unwrap();
                let model = match state {
                    "new" => Item::newInstanceOn(&db, Attributes::new()).unwrap(),
                    "existing" => Item::queryOn(&db)
                        .select(["id", "nullable"])
                        .findOrFail(1)
                        .await
                        .unwrap(),
                    "recent" => Item::queryOn(&db)
                        .create(Attributes::from([("name".into(), "recent".into())]))
                        .await
                        .unwrap(),
                    _ => unreachable!(),
                };
                let actual = match model.getAttributeValue(key) {
                    Ok(v) => json!({"value":value(v.as_ref())}),
                    Err(DatabaseError::MissingAttribute { .. }) => json!({"error":"missing"}),
                    Err(e) => panic!("属性读取错误：{e}"),
                };
                assert_eq!(
                    actual,
                    reference[format!("read_{}_{}_{}", u8::from(strict), state, key)]
                );
            }
        }
    }
    Handled::preventAccessingMissingAttributes(&app, true).unwrap();
    let model = Handled::queryOn(&db)
        .select(["id", "nullable"])
        .findOrFail(1)
        .await
        .unwrap();
    for key in ["unknown", "count", "status"] {
        let got = model.getAttributeValue(key).unwrap();
        assert_eq!(
            json!({"value":value(got.as_ref())}),
            reference[format!("read_handler_{key}")]
        );
    }
    assert!(model.getEnum::<Status>("status").unwrap().is_none());
    Item::preventAccessingMissingAttributes(&app, true).unwrap();
    let enum_model = Item::queryOn(&db)
        .select(["id"])
        .findOrFail(1)
        .await
        .unwrap();
    assert!(matches!(
        enum_model.getEnum::<Status>("status"),
        Err(DatabaseError::MissingAttribute { .. })
    ));
    // 异步转换器先完成可赋值字段，拒绝额外字段时保留已完成的属性赋值。
    AsyncItem::preventSilentlyDiscardingAttributes(&app, true).unwrap();
    let mut hashed = AsyncItem::newInstanceOn(&db, Attributes::new()).unwrap();
    assert!(matches!(
        hashed
            .fillAsync(Attributes::from([
                ("name".into(), "New".into()),
                ("secret".into(), "x".into())
            ]))
            .await,
        Err(DatabaseError::MassAssignment(_))
    ));
    assert!(matches!(hashed.getAttribute("name"),Some(Value::Text(v)) if v.starts_with('$')));
    Item::shouldBeStrict(&app, true).unwrap();
    let relation = HasMany::<Item, Child>::new("id", "owner_id");
    let mut single = Item::queryOn(&db).findOrFail(1).await.unwrap();
    assert!(!single.preventsLazyLoading());
    single
        .getRelationOrLoad("children", &relation)
        .await
        .unwrap();
    let mut many: Vec<_> = Item::queryOn(&db)
        .orderBy("id", "asc")
        .get()
        .await
        .unwrap()
        .into_iter()
        .collect();
    assert!(many[0].preventsLazyLoading());
    assert!(matches!(
        many[0].getRelationOrLoad("children", &relation).await,
        Err(DatabaseError::LazyLoadingViolation { .. })
    ));
    Item::preventLazyLoading(&app, false).unwrap();
    assert!(
        many[0].preventsLazyLoading(),
        "已有实例保留水合时的lazy-loading策略"
    );
    let mut later: Vec<_> = Item::queryOn(&db)
        .get()
        .await
        .unwrap()
        .into_iter()
        .collect();
    assert!(!later[0].preventsLazyLoading());
    later[0]
        .getRelationOrLoad("children", &relation)
        .await
        .unwrap();
    // 同一模型类型在另一个应用中不继承严格策略。
    let (_other_app, other_db) = application().await;
    let permissive = Item::newInstanceOn(&other_db, Attributes::new()).unwrap();
    assert!(!permissive.preventsAccessingMissingAttributes());
    let guarded = Guarded::newInstanceOn(&db, Attributes::new()).unwrap();
    assert!(!guarded.preventsAccessingMissingAttributes());
    let saved = Arc::new(AtomicUsize::new(0));
    Owner::registerModelEvent(&app, ModelEvent::Saved, Saved(saved.clone())).unwrap();
    for name in ["normal", "ignore_owner", "ignore_child", "ignore_on"] {
        db.statement(
            "update parity_strict_owners set updated_at='2000-01-01 00:00:00'",
            &[],
        )
        .await
        .unwrap();
        saved.store(0, Ordering::SeqCst);
        let mut child = Child::queryOn(&db).findOrFail(1).await.unwrap();
        child.setAttribute("name", name);
        match name {
            "normal" => child.save().await,
            "ignore_owner" => Owner::withoutTouching(child.save()).await,
            "ignore_child" => Child::withoutTouching(child.save()).await,
            "ignore_on" => Child::withoutTouchingOn(&[TypeId::of::<Owner>()], child.save()).await,
            _ => unreachable!(),
        }
        .unwrap();
        let owner = Owner::queryOn(&db).findOrFail(1).await.unwrap();
        let actual = json!({"changed":owner.getAttribute("updated_at")!=Some(&"2000-01-01 00:00:00".into()),"saved":saved.load(Ordering::SeqCst)});
        assert_eq!(actual, reference[format!("touch_{name}")]);
    }
    // 显式差异：同类型嵌套结束后恢复外层抑制，避免上游array_diff提前删除外层条目。
    assert_eq!(
        reference["upstream_nested_same"],
        json!([true, true, false, false])
    );
    Owner::withoutTouching(async {
        assert!(Owner::isIgnoringTouch());
        Owner::withoutTouching(async {
            assert!(Owner::isIgnoringTouch());
        })
        .await;
        assert!(Owner::isIgnoringTouch());
    })
    .await;
    assert!(!Owner::isIgnoringTouch());
    // 预先构造的嵌套Future在轮询时继承外层上下文。
    Owner::withoutTouching(OtherOwner::withoutTouching(async {
        assert!(Owner::isIgnoringTouch());
        assert!(OtherOwner::isIgnoringTouch());
    }))
    .await;
    assert!(!Owner::isIgnoringTouch());
    assert!(!OtherOwner::isIgnoringTouch());
    assert!(Child::isIgnoringTouch());
    let error = Owner::withoutTouching(async { Err::<(), _>("失败") }).await;
    assert!(error.is_err());
    assert!(!Owner::isIgnoringTouch());
    let panic = std::panic::AssertUnwindSafe(Owner::withoutTouching(async {
        panic!("隔离测试恐慌")
    }))
    .catch_unwind()
    .await;
    assert!(panic.is_err());
    assert!(!Owner::isIgnoringTouch());
    let started = Arc::new(tokio::sync::Notify::new());
    let signal = started.clone();
    let task = tokio::spawn(Owner::withoutTouching(async move {
        signal.notify_one();
        std::future::pending::<()>().await
    }));
    started.notified().await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(!Owner::isIgnoringTouch());
    let (inside, outside) = tokio::join!(
        Owner::withoutTouching(async {
            tokio::task::yield_now().await;
            Owner::isIgnoringTouch()
        }),
        async {
            tokio::task::yield_now().await;
            Owner::isIgnoringTouch()
        }
    );
    assert!(inside);
    assert!(!outside);
    for sql in [
        "drop table parity_strict_children",
        "drop table parity_strict_owners",
        "drop table parity_strict_items",
    ] {
        db.statement(sql, &[]).await.unwrap();
    }
}
