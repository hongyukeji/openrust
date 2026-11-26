use openrust::console::{
    ConsoleError, DefaultQuestionHelper, Helper, HelperInterface, HelperSet, Output, QuestionHelper,
};
use serde_json::{Value, json};
use std::{any::Any, sync::Arc};

struct Probe {
    name: &'static str,
    id: &'static str,
    helper: Helper,
}

impl Probe {
    fn new(name: &'static str, id: &'static str) -> Arc<Self> {
        Arc::new(Self {
            name,
            id,
            helper: Helper::new(),
        })
    }
}

impl HelperInterface for Probe {
    fn get_name(&self) -> &str {
        self.name
    }

    fn set_helper_set(&self, set: Option<&HelperSet>) {
        self.helper.set_helper_set(set);
        if let Some(set) = set {
            assert!(set.has(self.name), "注册后才通知关联，回调可重入读取");
        }
    }

    fn get_helper_set(&self) -> Option<HelperSet> {
        self.helper.get_helper_set()
    }
}

fn id(helper: &dyn HelperInterface) -> &'static str {
    (helper as &dyn Any).downcast_ref::<Probe>().unwrap().id
}

fn snapshot(set: &HelperSet) -> Vec<(String, &'static str)> {
    set.into_iter()
        .map(|(name, helper)| (name, id(helper.as_ref())))
        .collect()
}

#[test]
fn replacement_alias_order_and_membership_match_fixed_helper_set() {
    let source: Value = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/helper-set-oracle.json"
    ))
    .unwrap();
    let first = Probe::new("question", "old");
    let replacement = Probe::new("question", "new");
    let other = Probe::new("formatter", "formatter");
    let left = HelperSet::from_helpers([
        (
            Some("q".into()),
            Arc::clone(&first) as Arc<dyn HelperInterface>,
        ),
        (None, Arc::clone(&other) as Arc<dyn HelperInterface>),
    ]);
    let right = HelperSet::new();
    let before = left.get_iterator();
    let expected = |name: &str| source[name].as_object().unwrap();
    let check = |name: &str, values: Vec<(String, &'static str)>| {
        for (key, value) in values {
            assert_eq!(expected(name)[&key], value, "{name}/{key}");
        }
    };
    check("initial", snapshot(&left));
    assert_eq!(
        snapshot(&left)
            .iter()
            .map(|(key, _)| key.as_str())
            .collect::<Vec<_>>(),
        ["question", "q", "formatter"]
    );
    assert_eq!(
        first.get_helper_set().unwrap().same_set(&left),
        source["initial_owner"]
    );
    left.set(replacement, Some(""));
    assert_eq!(snapshot(&left).last().unwrap().0, "");
    check("after_replace", snapshot(&left));
    assert_eq!(
        id(left.get("q").unwrap().as_ref()) == "old",
        source["old_alias_kept"]
    );
    assert_eq!(
        first.get_helper_set().unwrap().same_set(&left),
        source["old_owner_kept"]
    );
    check(
        "iterator_snapshot",
        before
            .map(|(name, helper)| (name, id(helper.as_ref())))
            .collect(),
    );
    right.set(
        Arc::clone(&first) as Arc<dyn HelperInterface>,
        Some("other"),
    );
    check("right", snapshot(&right));
    assert_eq!(
        id(left.get("q").unwrap().as_ref()) == "old",
        source["old_still_lookup_after_move"]
    );
    assert_eq!(
        first.get_helper_set().unwrap().same_set(&right),
        source["owner_after_move"]
    );
    first.set_helper_set(None);
    assert_eq!(first.get_helper_set().is_none(), source["manual_detach"]);
    assert_eq!(
        id(right.get("other").unwrap().as_ref()) == "old",
        source["still_registered_after_detach"]
    );
    left.set(other, Some("question"));
    check("alias_collision", snapshot(&left));
    assert_eq!(!left.has("Question"), source["case_sensitive"]);
    assert!(left.get("missing").is_err());
}

#[test]
fn weak_membership_and_concrete_question_helpers_release_group() {
    let helper = Arc::new(QuestionHelper::new());
    let default = Arc::new(DefaultQuestionHelper::new());
    assert!(helper.get_helper_set().is_none());
    let set = HelperSet::new();
    set.set(
        Arc::clone(&helper) as Arc<dyn HelperInterface>,
        Some("component"),
    );
    set.set(
        Arc::clone(&default) as Arc<dyn HelperInterface>,
        Some("default"),
    );
    assert!(helper.get_helper_set().unwrap().same_set(&set));
    assert!(default.get_helper_set().unwrap().same_set(&set));
    assert_eq!(set.get("component").unwrap().get_name(), "question");
    assert_eq!(set.get("question").unwrap().get_name(), "question");
    assert!((set.get("question").unwrap().as_ref() as &dyn Any).is::<DefaultQuestionHelper>());
    drop(set);
    assert!(helper.get_helper_set().is_none());
    assert!(default.get_helper_set().is_none());
}

fn run_view(view: &str) -> Result<Value, ConsoleError> {
    let set = HelperSet::new();
    set.set(Probe::new("canonical", "registered"), Some("alias"));
    // 每次真实命令调用都持有自己的集合，未知名称走所属错误路径。
    let name = if view == "invalid" {
        "unknown"
    } else {
        "alias"
    };
    let helper = set.get(name).map_err(ConsoleError::from_source)?;
    Ok(json!({ "name": helper.get_name(), "id": id(helper.as_ref()), "input":view, "alias":name }))
}
fn commands() -> Result<(), ConsoleError> {
    openrust::support::facades::Artisan::command_with(
        "helpers:set {view=normal}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let view = input
                    .argument("view")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap();
                output.comment(&run_view(&view)?.to_string());
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
#[test]
#[ignore = "由父消费者执行真实Artisan退出码"]
fn set_cli_driver() {
    let root = tempfile::tempdir().unwrap();
    let app = openrust::foundation::Application::configure(root.path().into())
        .with_routing(openrust::foundation::configuration::Routing::default().commands(commands))
        .create()
        .unwrap();
    let args = vec![
        "helpers:set".into(),
        std::env::var("HELPER_SET_VIEW").unwrap().into(),
        "--no-ansi".into(),
    ];
    let rt = tokio::runtime::Runtime::new().unwrap();
    let code = rt.block_on(app.handle_command(&args)).unwrap();
    drop(rt);
    drop(root);
    std::process::exit(code);
}
#[test]
fn actual_artisan_helper_set_and_unknown_name_exit() {
    for view in ["normal", "unicode", "invalid"] {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "set_cli_driver", "--ignored", "--nocapture"])
            .env("HELPER_SET_VIEW", view)
            .output()
            .unwrap();
        assert_eq!(
            result.status.code(),
            Some(if view == "invalid" { 1 } else { 0 }),
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        if view != "invalid" {
            assert!(String::from_utf8_lossy(&result.stdout).contains("alias"));
        }
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::get("/helper-set", response)?;
    Ok(())
}
fn response(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let view = request
            .query(Some("view"))?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "normal".into());
        let mut output = Output::default();
        let result =
            openrust::support::facades::Artisan::call("helpers:set", &[view.into()], &mut output)
                .await;
        openrust::http::Response::json(
            if result.is_ok() {
                http::StatusCode::OK
            } else {
                http::StatusCode::INTERNAL_SERVER_ERROR
            },
            json!({"output":output.stdout}),
        )
    })
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_http_helper_failure_recovery_and_parallel_registries() {
    use axum::handler::Handler as _;
    use openrust::contracts::http::Kernel as _;
    use std::sync::Arc;
    async fn transport(
        axum::extract::State(k): axum::extract::State<Arc<openrust::foundation::http::Kernel>>,
        r: axum::extract::Request,
    ) -> axum::response::Response {
        k.handle_transport(r).await
    }
    let root = tempfile::tempdir().unwrap();
    let app = openrust::foundation::Application::configure(root.path().into())
        .with_routing(
            openrust::foundation::configuration::Routing::default()
                .web(routes)
                .commands(commands),
        )
        .create()
        .unwrap();
    let mut kernel = openrust::foundation::http::Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap()
    });
    let client = reqwest::Client::new();
    assert_eq!(
        client
            .get(format!("http://{address}/helper-set?view=invalid"))
            .send()
            .await
            .unwrap()
            .status(),
        500
    );
    let mut calls = tokio::task::JoinSet::new();
    for index in 0..14 {
        let client = client.clone();
        calls.spawn(async move {
            let view = if index % 2 == 0 { "normal" } else { "unicode" };
            let response = client
                .get(format!("http://{address}/helper-set?view={view}"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let body: Value = response.json().await.unwrap();
            let value: Value =
                serde_json::from_str(body["output"].as_str().unwrap().trim()).unwrap();
            assert_eq!(value, run_view(view).unwrap());
        });
    }
    while let Some(result) = calls.join_next().await {
        result.unwrap();
    }
    server.abort();
    let _ = server.await;
}
