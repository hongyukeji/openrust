use openrust::{
    contracts::foundation::Application as _,
    database::DatabaseManager,
    foundation::observability::{self, Observation, Observer, Operation, Outcome},
    support::facades::{App, Facade as _},
    testing::TestApp,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Clone, Default)]
struct Records {
    finished: Arc<Mutex<Vec<&'static str>>>,
    polls: Arc<AtomicUsize>,
}
struct SqlObservation(Records);
impl Observer for Records {
    fn begin(&self, operation: Operation<'_>) -> Option<Box<dyn Observation>> {
        matches!(operation, Operation::Sql { .. })
            .then(|| Box::new(SqlObservation(self.clone())) as Box<dyn Observation>)
    }
}
impl Observation for SqlObservation {
    fn in_scope(&self, poll: &mut dyn FnMut()) {
        self.0.polls.fetch_add(1, Ordering::SeqCst);
        poll();
    }
    fn finish(&mut self, outcome: Outcome) {
        let status = match outcome {
            Outcome::Sql { failed: false } => "ok",
            Outcome::Sql { failed: true } => "failed",
            Outcome::Cancelled => "cancelled",
            _ => "other",
        };
        self.0.finished.lock().unwrap().push(status);
    }
}
#[tokio::test]
async fn boxed_sql_observation_preserves_success_failure_poll_scope_and_cancellation() {
    let mut app=TestApp::create(|builder|builder.with_config(|_,_|Ok([
        ("app".into(),serde_json::json!({"env":"testing"})),
        ("database".into(),serde_json::json!({"default":"test","connections":{"test":{"driver":"sqlite","database":":memory:","max_connections":1}}})),
    ].into()))).unwrap();
    app.boot().await.unwrap();
    let records = Records::default();
    observability::register(app.app().container(), Arc::new(records.clone())).unwrap();
    App::set_facade_application(Some(app.app()), async {
        let db = app
            .app()
            .make::<DatabaseManager>()
            .await
            .unwrap()
            .connection(None)
            .unwrap();
        assert_eq!(db.select("select 1", &[]).await.unwrap().len(), 1);
        assert_eq!(*records.finished.lock().unwrap(), ["ok"]);
        assert!(records.polls.load(Ordering::SeqCst) > 0);
        assert!(db.select("select from invalid syntax", &[]).await.is_err());
        assert_eq!(*records.finished.lock().unwrap(), ["ok", "failed"]);
        let mut transaction = db.begin_transaction().await.unwrap();
        records.finished.lock().unwrap().clear();
        let mut query = Box::pin(db.select("select 2", &[]));
        assert!(futures_util::poll!(query.as_mut()).is_pending());
        drop(query);
        assert_eq!(
            *records.finished.lock().unwrap(),
            ["cancelled"],
            "取消时须且只须收尾一次"
        );
        transaction.roll_back(None).await.unwrap();
    })
    .await;
}
