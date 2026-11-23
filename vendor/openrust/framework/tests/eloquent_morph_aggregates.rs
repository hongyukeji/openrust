use openrust::{
    config::Repository,
    database::{
        Connection, DatabaseManager, Value,
        eloquent::{
            Builder, Model, ModelMetadata, ModelState, Scope,
            relations::{HasMany, MorphTarget, MorphTo},
        },
    },
};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};

macro_rules! model {
    ($name:ident, $table:literal) => {
        struct $name(ModelState);
        impl Model for $name {
            fn metadata() -> &'static ModelMetadata {
                static META: ModelMetadata = ModelMetadata::new($table).timestamps(None, None);
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
    };
}
model!(Activity, "parity_morph_activities");
model!(Post, "parity_morph_posts");
model!(Other, "parity_morph_others");
struct Rating(ModelState);
struct Positive;
impl Scope<Rating> for Positive {
    fn apply(&self, query: &mut Builder<'_, Rating>) -> openrust::database::DatabaseResult<()> {
        query.r#where("score", ">=", 0);
        Ok(())
    }
}
impl Model for Rating {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata =
            ModelMetadata::new("parity_morph_ratings").timestamps(None, None);
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
    fn global_scopes() -> &'static [(&'static str, &'static dyn Scope<Self>)] {
        &[("positive", &Positive)]
    }
}
fn morph() -> MorphTo<Activity> {
    MorphTo::new("subject_type", "subject_id")
        .target::<Post>("post", "id")
        .target::<Other>("other", "id")
}
fn connection() -> Connection {
    let config = match std::env::var("OPENRUST_TEST_MORPH_PARITY_URL") {
        Ok(url) => json!({"url":url}),
        Err(_) => {
            assert!(std::env::var_os("OPENRUST_REQUIRE_MORPH_PARITY").is_none());
            json!({"driver":"sqlite","database":":memory:"})
        }
    };
    DatabaseManager::new(
        Arc::new(Repository::new(BTreeMap::from([(
            "database".into(),
            json!({"default":"test","connections":{"test":config}}),
        )]))),
        ".",
    )
    .connection(None)
    .unwrap()
}
fn numeric(value: &Value) -> Option<f64> {
    match value {
        Value::Null => None,
        Value::Integer(value) => Some(*value as f64),
        Value::Real(value) => Some(*value),
        Value::Text(value) => Some(value.parse().unwrap()),
        other => panic!("聚合数值类型错误：{other:?}"),
    }
}
#[tokio::test]
async fn morph_aggregate_helpers_preserve_scopes_null_targets_aliases_and_cached_state_on_errors() {
    let db = connection();
    for sql in [
        "create table parity_morph_posts (id integer primary key)",
        "create table parity_morph_others (id integer primary key)",
        "create table parity_morph_ratings (id integer primary key, post_id integer, score integer)",
        "create table parity_morph_activities (id integer primary key, subject_type varchar(64), subject_id integer)",
        "insert into parity_morph_posts (id) values (1),(2)",
        "insert into parity_morph_others (id) values (1)",
        "insert into parity_morph_ratings (id,post_id,score) values (1,1,10),(2,1,20),(3,1,-100)",
        "insert into parity_morph_activities (id,subject_type,subject_id) values (1,'post',1),(2,'post',2),(3,'other',1),(4,null,null)",
    ] {
        db.statement(sql, &[]).await.unwrap();
    }
    let subjects = morph();
    let ratings = HasMany::<Post, Rating>::new("id", "post_id");
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("reference/eloquent-morph-aggregates.json")).unwrap();
    assert_eq!(reference.as_array().unwrap().len(), 16);
    for case in reference.as_array().unwrap() {
        let id = case["id"].as_i64().unwrap();
        let attribute = case["attribute"].as_str().unwrap();
        let mut activity = Activity::query_on(&db)
            .with("subject", morph())
            .find_or_fail(id)
            .await
            .unwrap();
        match case["method"].as_str().unwrap() {
            "max" => {
                activity
                    .load_morph_max("subject", &subjects, "post", "ratings", &ratings, "score")
                    .await
            }
            "min" => {
                activity
                    .load_morph_min("subject", &subjects, "post", "ratings", &ratings, "score")
                    .await
            }
            "sum" => {
                activity
                    .load_morph_sum("subject", &subjects, "post", "ratings", &ratings, "score")
                    .await
            }
            "avg" => {
                activity
                    .load_morph_avg("subject", &subjects, "post", "ratings", &ratings, "score")
                    .await
            }
            _ => unreachable!(),
        }
        .unwrap();
        let target = activity
            .get_relation::<Option<MorphTarget>>("subject")
            .unwrap();
        if id <= 2 {
            let model = target.as_ref().unwrap().downcast_ref::<Post>().unwrap();
            let value = model.get_attribute(attribute);
            assert_eq!(
                value.is_some(),
                case["present"].as_bool().unwrap(),
                "{case}"
            );
            assert_eq!(value.and_then(numeric), case["value"].as_f64(), "{case}");
            assert_eq!(model.get_raw_original(attribute), value);
        } else if id == 3 {
            assert!(
                target
                    .as_ref()
                    .unwrap()
                    .downcast_ref::<Other>()
                    .unwrap()
                    .get_attribute(attribute)
                    .is_none()
            );
        } else {
            assert!(target.is_none());
        }
    }
    let mut activity = Activity::query_on(&db)
        .with("subject", morph())
        .find_or_fail(1)
        .await
        .unwrap();
    activity
        .load_morph_max(
            "subject",
            &subjects,
            "post",
            "ratings as peak",
            &ratings,
            "score",
        )
        .await
        .unwrap();
    assert_eq!(
        numeric(
            activity
                .get_relation::<Option<MorphTarget>>("subject")
                .unwrap()
                .as_ref()
                .unwrap()
                .downcast_ref::<Post>()
                .unwrap()
                .get_attribute("peak")
                .unwrap()
        ),
        Some(20.)
    );
    let before = activity.to_array().unwrap();
    assert!(
        activity
            .load_morph_sum(
                "subject",
                &subjects,
                "post",
                "ratings",
                &ratings,
                "missing_column"
            )
            .await
            .is_err()
    );
    assert_eq!(
        activity.to_array().unwrap(),
        before,
        "SQL 失败不能部分改写缓存目标"
    );
    assert!(
        activity
            .load_morph_avg("subject", &subjects, "other", "ratings", &ratings, "score")
            .await
            .is_err()
    );
    assert_eq!(activity.to_array().unwrap(), before);
}
