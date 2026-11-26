use openrust::{
    config::Repository,
    database::{
        Connection, DatabaseError, DatabaseManager, DatabaseResult, Value,
        eloquent::{
            Builder, Collection, EagerLoads, Model, ModelMetadata, ModelState, RelatedCollection,
            Scope, SoftDeletingScope, relations::HasMany,
        },
        query,
    },
};
use serde_json::{Value as Json, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Item(ModelState);
struct Child(ModelState);
struct Custom(ModelState);
struct Visible;
struct Tenant;
impl Scope<Item> for Visible {
    fn apply(&self, builder: &mut Builder<'_, Item>) -> DatabaseResult<()> {
        builder.r#where("visible", "=", 1);
        Ok(())
    }
}
impl Scope<Child> for Visible {
    fn apply(&self, builder: &mut Builder<'_, Child>) -> DatabaseResult<()> {
        builder.r#where("visible", "=", 1);
        Ok(())
    }
}
impl Scope<Item> for Tenant {
    fn apply(&self, builder: &mut Builder<'_, Item>) -> DatabaseResult<()> {
        builder.r#where("tenant", "=", 1);
        Ok(())
    }
}
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
impl Model for Item {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("parity_scope_items")
            .timestamps(None, None)
            .soft_deletes("deleted_at");
        &META
    }
    state!();
    fn global_scopes() -> &'static [(&'static str, &'static dyn Scope<Self>)] {
        &[("visible", &Visible), ("tenant", &Tenant)]
    }
}
impl Model for Child {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata =
            ModelMetadata::new("parity_scope_children").timestamps(None, None);
        &META
    }
    state!();
    fn global_scopes() -> &'static [(&'static str, &'static dyn Scope<Self>)] {
        &[("visible", &Visible)]
    }
}
impl Model for Custom {
    fn metadata() -> &'static ModelMetadata {
        Item::metadata()
    }
    state!();
    fn new_eloquent_builder(query: query::Builder<'static>) -> Builder<'static, Self> {
        Builder::from_query(query.r#where("tenant", "=", 2))
    }
}
struct Extended(Arc<AtomicUsize>, Arc<AtomicUsize>);
impl Scope<Item> for Extended {
    fn extend(&self, builder: &mut Builder<'_, Item>) -> DatabaseResult<()> {
        self.0.fetch_add(1, Ordering::SeqCst);
        builder.r#where("id", "=", 1);
        Ok(())
    }
    fn apply(&self, _builder: &mut Builder<'_, Item>) -> DatabaseResult<()> {
        self.1.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
fn connect() -> Connection {
    let config = match std::env::var("OPENRUST_TEST_SCOPE_URL") {
        Ok(url) => json!({"url":url}),
        Err(_) => {
            assert!(std::env::var_os("OPENRUST_REQUIRE_SCOPE").is_none());
            json!({"driver":"sqlite","database":":memory:"})
        }
    };
    DatabaseManager::new(
        Arc::new(Repository::new(std::collections::BTreeMap::from([(
            "database".into(),
            json!({"default":"scopes","connections":{"scopes":config}}),
        )]))),
        ".",
    )
    .connection(None)
    .unwrap()
}
fn ids<M: Model>(models: Collection<M>) -> Vec<i64> {
    models
        .iter()
        .map(|m| match m.get_key().unwrap() {
            Value::Integer(v) => *v,
            other => panic!("主键类型错误：{other:?}"),
        })
        .collect()
}
async fn run(name: &str, query: Builder<'static, Item>, reference: &Json) {
    let removed: Vec<_> = query
        .removed_scopes()
        .iter()
        .map(|n| {
            if n == std::any::type_name::<SoftDeletingScope>() {
                "soft"
            } else {
                n.as_str()
            }
        })
        .collect();
    assert_eq!(
        json!(removed),
        reference[name]["removed"],
        "{name} 移除顺序"
    );
    let got = ids(query.order_by("id", "asc").get().await.unwrap());
    assert_eq!(json!(got), reference[name]["ids"], "{name}");
}
#[tokio::test]
async fn model_scopes_query_replacement_and_eager_loads_follow_fixed_reference() {
    let db = connect();
    for sql in [
        "create table parity_scope_items (id integer primary key, visible integer, tenant integer, deleted_at varchar(64))",
        "create table parity_scope_children (id integer primary key, parent_id integer, visible integer)",
        "insert into parity_scope_items values (1,1,1,null),(2,1,2,null),(3,0,1,null),(4,1,1,'2026-11-23 00:00:00')",
        "insert into parity_scope_children values (10,1,1),(11,1,0),(12,2,1)",
    ] {
        db.statement(sql, &[]).await.unwrap();
    }
    let reference: Json =
        serde_json::from_str(include_str!("reference/eloquent-scopes.json")).unwrap();
    assert_eq!(reference.as_object().unwrap().len(), 23);
    let q = || Item::query_on(&db);
    run("default", q(), &reference).await;
    run(
        "override",
        q().with_global_scope("visible", |q: &mut Builder<'_, Item>| {
            q.r#where("visible", "=", 0);
            Ok(())
        })
        .unwrap(),
        &reference,
    )
    .await;
    run(
        "except_tenant",
        q().without_global_scopes_except(&["tenant"]),
        &reference,
    )
    .await;
    run(
        "except_empty",
        q().without_global_scopes_except(&[]),
        &reference,
    )
    .await;
    run(
        "except_all",
        q().without_global_scopes_except(&[
            "visible",
            "tenant",
            std::any::type_name::<SoftDeletingScope>(),
        ]),
        &reference,
    )
    .await;
    run(
        "remove_repeat",
        q().without_global_scope("visible")
            .without_global_scope("visible")
            .without_global_scope("missing"),
        &reference,
    )
    .await;
    run(
        "remove_reregister",
        q().without_global_scope("visible")
            .with_global_scope("visible", |q: &mut Builder<'_, Item>| {
                q.r#where("visible", "=", 0);
                Ok(())
            })
            .unwrap(),
        &reference,
    )
    .await;
    run(
        "clear_add",
        q().without_global_scopes()
            .with_global_scope("single", |q: &mut Builder<'_, Item>| {
                q.r#where("id", "=", 2);
                Ok(())
            })
            .unwrap(),
        &reference,
    )
    .await;
    let mut query = q();
    query.r#where("id", "=", 2).or_where("id", "=", 1);
    run("or_safe", query, &reference).await;
    run(
        "scope_or_safe",
        q().with_global_scope("visible", |q: &mut Builder<'_, Item>| {
            q.r#where("id", "=", 1).or_where("id", "=", 2);
            Ok(())
        })
        .unwrap(),
        &reference,
    )
    .await;
    run("with_trashed", q().with_trashed(), &reference).await;
    run("without_trashed", q().without_trashed(), &reference).await;
    run("only_trashed", q().only_trashed(), &reference).await;
    run(
        "only_except_empty",
        q().only_trashed().without_global_scopes_except(&[]),
        &reference,
    )
    .await;
    let mut query = q();
    query.r#where("id", "=", 999);
    run(
        "replace_query",
        query.set_query(db.table("parity_scope_items").r#where("id", "=", 1)),
        &reference,
    )
    .await;
    let base = q()
        .get_query()
        .order_by("id", "asc")
        .pluck("id")
        .await
        .unwrap();
    assert_eq!(
        json!(
            base.iter()
                .map(|v| match v {
                    Value::Integer(v) => *v,
                    _ => panic!(),
                })
                .collect::<Vec<_>>()
        ),
        reference["base_unscoped"]
    );
    let mut query = q();
    query.r#where("id", "=", 2);
    assert_eq!(query.get_query().pluck("id").await.unwrap(), vec![2.into()]);
    let got = q()
        .apply_scopes()
        .unwrap()
        .get_query()
        .order_by("id", "asc")
        .pluck("id")
        .await
        .unwrap();
    assert_eq!(got, vec![1.into()]);
    let got = q()
        .apply_scopes()
        .unwrap()
        .apply_scopes()
        .unwrap()
        .get_query()
        .order_by("id", "asc")
        .pluck("id")
        .await
        .unwrap();
    assert_eq!(got, vec![1.into()]);
    let eager =
        EagerLoads::default().with("children", HasMany::<Item, Child>::new("id", "parent_id"));
    let query = q().set_eager_loads(eager);
    assert_eq!(
        json!(query.get_eager_loads().names().collect::<Vec<_>>()),
        reference["eager_names"]
    );
    let saved = query.get_eager_loads().clone();
    let model = query.first_or_fail().await.unwrap();
    assert_eq!(
        model
            .get_relation::<RelatedCollection<Child>>("children")
            .unwrap()
            .iter()
            .map(|c| {
                match c.get_key().unwrap() {
                    Value::Integer(v) => *v,
                    other => panic!("主键类型错误：{other:?}"),
                }
            })
            .collect::<Vec<_>>(),
        vec![10]
    );
    let query = q()
        .set_eager_loads(saved)
        .set_eager_loads(EagerLoads::default());
    assert_eq!(
        json!(query.get_eager_loads().names().collect::<Vec<_>>()),
        reference["eager_reset"]
    );
    assert!(
        !query
            .first_or_fail()
            .await
            .unwrap()
            .relation_loaded("children")
    );
    assert_eq!(
        json!(ids(Child::query_on(&db)
            .order_by("id", "asc")
            .get()
            .await
            .unwrap())),
        reference["model_isolation"]
    );
    // 查询工厂通过公开模型入口生效，且不会泄漏到另一模型类型。
    assert_eq!(ids(Custom::query_on(&db).get().await.unwrap()), vec![2]);
    let extends = Arc::new(AtomicUsize::new(0));
    let applies = Arc::new(AtomicUsize::new(0));
    let query = q()
        .with_global_scope("extended", Extended(extends.clone(), applies.clone()))
        .unwrap();
    assert_eq!(extends.load(Ordering::SeqCst), 1);
    assert_eq!(applies.load(Ordering::SeqCst), 0);
    assert_eq!(ids(query.get().await.unwrap()), vec![1]);
    assert_eq!(applies.load(Ordering::SeqCst), 1);
    let error = q()
        .with_global_scope("failed", |_: &mut Builder<'_, Item>| {
            Err(DatabaseError::InvalidQuery("作用域失败"))
        })
        .unwrap()
        .get()
        .await;
    assert!(matches!(
        error,
        Err(DatabaseError::InvalidQuery("作用域失败"))
    ));
    assert_eq!(ids(q().get().await.unwrap()), vec![1]);
    // 同时运行两个查询，各自的作用域注册表与移除记录保持隔离。
    let (a, b) = tokio::join!(q().without_global_scopes().get(), q().get());
    assert_eq!(ids(a.unwrap()).len(), 4);
    assert_eq!(ids(b.unwrap()), vec![1]);
    db.statement("drop table parity_scope_children", &[])
        .await
        .unwrap();
    db.statement("drop table parity_scope_items", &[])
        .await
        .unwrap();
}
