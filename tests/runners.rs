use std::{
    collections::BTreeMap,
    io,
    sync::{Arc, Mutex},
    time::Duration,
};

use openrust::{
    foundation::{Application, Runner, RunnerContext, RuntimeResult},
    http::HttpFuture,
    support::ServiceProvider,
};
use serde_json::json;
use tokio::sync::{Barrier, Notify};

#[derive(Default)]
struct State {
    events: Mutex<Vec<(&'static str, &'static str)>>,
    run: Notify,
    fail: Notify,
}

impl State {
    fn record(&self, phase: &'static str, signature: &'static str) {
        self.events.lock().unwrap().push((phase, signature));
    }
}

#[derive(Clone, Copy)]
enum Role {
    Normal,
    Returns,
    Fails,
    Ineligible,
    Filtered,
}

struct Probe {
    signature: &'static str,
    role: Role,
    priority: i32,
    state: Arc<State>,
    shutdown_group: Option<Arc<Barrier>>,
}

#[allow(non_snake_case, reason = "Implements the Runner contract")]
impl Runner for Probe {
    fn signature(&self) -> &str {
        self.signature
    }

    fn shouldRun(&self) -> HttpFuture<'_, RuntimeResult<bool>> {
        Box::pin(async move {
            assert!(
                !matches!(self.role, Role::Filtered),
                "filtered or duplicate runner evaluated"
            );
            self.state.record("should", self.signature);
            Ok(!matches!(self.role, Role::Ineligible))
        })
    }

    fn run(&self, context: RunnerContext) -> HttpFuture<'_, RuntimeResult<()>> {
        Box::pin(async move {
            self.state.record("run", self.signature);
            match self.role {
                Role::Normal => context.cancelled().await,
                Role::Returns => {}
                Role::Fails => {
                    self.state.run.notify_one();
                    self.state.fail.notified().await;
                    return Err(io::Error::other("runner probe failure").into());
                }
                _ => unreachable!("runner was not eligible"),
            }
            Ok(())
        })
    }

    fn shutdown(&self, context: RunnerContext) -> HttpFuture<'_, RuntimeResult<()>> {
        Box::pin(async move {
            assert!(context.is_cancelled());
            self.state.record("stop", self.signature);
            if let Some(group) = &self.shutdown_group {
                group.wait().await;
            }
            self.state.record("stopped", self.signature);
            Ok(())
        })
    }

    fn shutdownPriority(&self) -> i32 {
        self.priority
    }
}

fn probe(state: &Arc<State>, signature: &'static str, role: Role, priority: i32) -> Probe {
    Probe {
        signature,
        role,
        priority,
        state: state.clone(),
        shutdown_group: None,
    }
}

struct Provider(Arc<State>);

impl ServiceProvider for Provider {
    fn runners(&self, _: &Application) -> Vec<Arc<dyn Runner>> {
        let barrier = Arc::new(Barrier::new(2));
        let mut first = probe(&self.0, "app:first", Role::Normal, -1);
        first.shutdown_group = Some(barrier.clone());
        let mut returns = probe(&self.0, "app:returns", Role::Returns, -1);
        returns.shutdown_group = Some(barrier);
        vec![Arc::new(first), Arc::new(returns)]
    }
}

#[tokio::test]
async fn provider_and_custom_runners_filter_start_and_shutdown_in_priority_groups() {
    let state = Arc::new(State::default());
    let custom = state.clone();
    let app = Application::configure(std::env::temp_dir().canonicalize().unwrap())
        .withConfig(|_, _| {
            Ok(BTreeMap::from([(
                "app".into(),
                json!({"disabled_runners": ["app:skip/*", r"app:literal\*", "app:[x-z]", "[broken"]}),
            )]))
        })
        .withProviders(vec![Box::new(Provider(state.clone()))])
        .withRunners(|| panic!("replaced custom runner registration was evaluated"))
        .withRunners(move || {
            vec![
                Arc::new(probe(&custom, "queue", Role::Filtered, 0)),
                Arc::new(probe(&custom, "app:first", Role::Filtered, 0)),
                Arc::new(probe(&custom, "app:skip/one", Role::Filtered, 0)),
                Arc::new(probe(&custom, "app:literal*", Role::Filtered, 0)),
                Arc::new(probe(&custom, "app:y", Role::Filtered, 0)),
                Arc::new(probe(&custom, "app:ineligible", Role::Ineligible, 0)),
                Arc::new(probe(&custom, "app:skip/deep/run", Role::Normal, 0)),
                Arc::new(probe(&custom, "app:failure", Role::Fails, 0)),
            ]
        })
        .create()
        .unwrap();

    let (result, ()) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(app.start(), async {
            state.run.notified().await;
            tokio::task::yield_now().await;
            assert!(
                state
                    .events
                    .lock()
                    .unwrap()
                    .iter()
                    .all(|(phase, _)| *phase != "stop")
            );
            state.fail.notify_one();
        })
    })
    .await
    .expect("runners and their shutdown hooks must run concurrently");

    assert!(result.unwrap_err().to_string().contains("app:failure"));
    let events = state.events.lock().unwrap();
    assert!(events.contains(&("should", "app:ineligible")));
    assert!(!events.contains(&("run", "app:ineligible")));
    assert!(!events.contains(&("stop", "app:failure")));
    let position = |phase, signature| {
        events
            .iter()
            .position(|item| *item == (phase, signature))
            .unwrap()
    };
    assert!(position("stopped", "app:first") < position("stop", "app:skip/deep/run"));
    assert!(position("stopped", "app:returns") < position("stop", "app:skip/deep/run"));
}
