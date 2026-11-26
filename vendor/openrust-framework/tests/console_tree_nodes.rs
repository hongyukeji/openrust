use openrust::{
    console::{ConsoleError, TreeChild, TreeChildren, TreeNode, TreeValue, TreeValues},
    support::ArrayKey,
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
fn values(kind: &str) -> TreeValues {
    match kind {
        "empty" => vec![],
        "scalar" => vec![(0i64.into(), "one".into()), (1i64.into(), "two".into())],
        "nested" => vec![
            (
                "branch".into(),
                TreeValue::branch([
                    (ArrayKey::Integer(0), TreeValue::from("a")),
                    ("deep".into(), TreeValue::branch([(0i64, "b"), (1i64, "c")])),
                ]),
            ),
            ("empty".into(), TreeValue::Values(vec![])),
        ],
        "node" => {
            let node = TreeNode::new("discarded");
            node.add_child("leaf")
                .add_child(TreeNode::with_children("inner", ["deep"]));
            vec![("node-key".into(), node.into())]
        }
        "mixed" => vec![
            (0i64.into(), "text".into()),
            ("branch".into(), TreeValue::branch([(0i64, "nested")])),
            (
                42i64.into(),
                TreeNode::with_children("discarded", ["n"]).into(),
            ),
            (43i64.into(), "last".into()),
        ],
        _ => unreachable!(),
    }
}
fn shape(node: &TreeNode) -> Value {
    json!([
        node.get_value(),
        node.into_iter()
            .map(|child| shape(&child.unwrap()))
            .collect::<Vec<_>>()
    ])
}
#[test]
fn fixed_source_values_identity_snapshot_and_each_public_node_entry() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/tree-node-oracle.json"
    ))
    .unwrap();
    for c in cases.iter().filter(|c| c["lazy"] != true) {
        let root = TreeNode::with_children(c["root"].as_str().unwrap(), ["prior"]);
        let same = root.clone();
        let made = TreeNode::from_values(values(c["kind"].as_str().unwrap()), Some(root)).unwrap();
        let mut iterator: TreeChildren = made.get_children();
        if c["mutation"] == true {
            made.add_child("before-start");
        }
        let first = iterator.next().unwrap().unwrap();
        made.add_child("after-start");
        let mut held = vec![first.get_value().to_owned()];
        held.extend(iterator.map(|node| node.unwrap().get_value().to_owned()));
        let got = json!({"root":c["root"],"kind":c["kind"],"mutation":c["mutation"],"same":same.ptr_eq(&made),"first":first.get_value(),"held":held,"count":made.count().unwrap(),"string":made.to_string(),"shape":shape(&made)});
        assert_eq!(got, *c, "{c}");
        assert_eq!(made.get_iterator().count(), made.count().unwrap());
    }
    assert_eq!(TreeNode::default().get_value(), "");
    assert_eq!(
        TreeNode::from_values(Vec::<(i64, &str)>::new(), None)
            .unwrap()
            .count()
            .unwrap(),
        0
    );
}
#[test]
fn lazy_keyed_iterators_match_source_and_are_recreated_for_count() {
    let expected: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/tree-node-oracle.json"
    ))
    .unwrap();
    let trace = Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));
    let root = TreeNode::new("lazy");
    root.add_child("a");
    let t = trace.clone();
    root.add_child(TreeChild::lazy_keyed(move || {
        t.lock().push("factory".into());
        vec![
            Ok((7i64.into(), TreeNode::new("x"))),
            Ok((2i64.into(), TreeNode::new("y"))),
        ]
    }));
    root.add_child("b");
    let t = trace.clone();
    root.add_child(TreeChild::lazy(move || {
        t.lock().push("factory2".into());
        [Ok(TreeNode::new("z"))]
    }));
    root.add_child("c");
    let mut iterator = root.get_children();
    let before = trace.lock().clone();
    let mut keys = vec![];
    let mut values = vec![];
    while let Some(node) = iterator.next() {
        keys.push(iterator.key().unwrap().clone());
        values.push(node.unwrap().to_string());
    }
    assert_eq!(iterator.key(), None);
    assert!(iterator.next().is_none());
    let count = root.count().unwrap();
    let got = json!({"lazy":true,"before":before,"trace":*trace.lock(),"keys":keys,"values":values,"count":count});
    assert_eq!(&got, expected.last().unwrap());
}
#[test]
fn factory_is_unlocked_reentrant_and_count_propagates_errors() {
    let root = TreeNode::new("reentrant");
    let weak = root.downgrade();
    root.add_child(TreeChild::lazy(move || {
        weak.upgrade().unwrap().add_child("late");
        [Ok(TreeNode::new("first"))]
    }));
    assert_eq!(root.count().unwrap(), 1);
    assert_eq!(root.count().unwrap(), 2);
    let calls = Arc::new(AtomicUsize::new(0));
    let c = calls.clone();
    root.add_child(TreeChild::lazy(move || {
        c.fetch_add(1, Ordering::SeqCst);
        [Err(ConsoleError::new("consumer failure"))]
    }));
    assert_eq!(root.count().unwrap_err().to_string(), "consumer failure");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let mut iterator = root.get_children();
    while let Some(result) = iterator.next() {
        if result.is_err() {
            assert!(iterator.next().is_none());
            break;
        }
    }
}
#[test]
fn owned_identity_weak_backlinks_and_explicit_cycle_release() {
    let weak = {
        let root = TreeNode::new("root");
        let child = TreeNode::new("child");
        let weak = root.downgrade();
        root.add_child(child);
        weak
    };
    assert!(weak.upgrade().is_none());
    let root = TreeNode::new("cycle");
    let weak = root.downgrade();
    root.add_child(root.clone());
    assert!(root.ptr_eq(&root.get_children().next().unwrap().unwrap()));
    root.clear_children();
    drop(root);
    assert!(weak.upgrade().is_none());
    let root = TreeNode::new("cycle value");
    root.add_child(root.clone());
    assert!(
        TreeNode::from_values([(0i64, root.clone())], None)
            .unwrap_err()
            .to_string()
            .contains("循环")
    );
    root.clear_children();
    let root = TreeNode::new("callback");
    let weak = root.downgrade();
    let w = weak.clone();
    root.add_child(TreeChild::lazy(move || {
        [Ok(TreeNode::new(w.upgrade().unwrap().get_value()))]
    }));
    assert_eq!(root.count().unwrap(), 1);
    drop(root);
    assert!(weak.upgrade().is_none());
}
fn command() -> Result<(), ConsoleError> {
    openrust::support::facades::Artisan::command_with(
        "trees:nodes {--fail}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let root = TreeNode::from_values(values("nested"), Some(TreeNode::new("Report")))?;
                root.add_child(TreeChild::lazy(|| [Ok(TreeNode::new("generated"))]));
                if input.option("fail").and_then(|value| value.as_bool()) == Some(true) {
                    root.add_child(TreeChild::lazy(|| {
                        [Err(ConsoleError::new("tree factory failed"))]
                    }));
                }
                let count = root.count()?;
                let mut selected = openrust::console::OutputStyle::new(input, output);
                selected.write(
                    json!({"shape":shape(&root),"count":count})
                        .to_string()
                        .as_str(),
                    false,
                    openrust::console::OutputOptions::default(),
                )?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
#[test]
fn actual_artisan_node_factories_propagate_failures_and_exit_codes() {
    for fail in [false, true] {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tree_nodes_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("TREE_NODES_FAIL", if fail { "1" } else { "0" })
            .output()
            .unwrap();
        assert_eq!(
            result.status.code(),
            Some(if fail { 1 } else { 0 }),
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        if !fail {
            let output = String::from_utf8(result.stdout).unwrap();
            let marker = output.find('{').unwrap();
            let data: Value = serde_json::from_str(&output[marker..]).unwrap();
            assert_eq!(data["count"], 3);
            assert_eq!(data["shape"][0], "Report");
        }
    }
}
#[test]
#[ignore = "由父消费者运行独立命令并核对真实退出码"]
fn tree_nodes_cli_driver() {
    use openrust::foundation::{Application, configuration::Routing};
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(command))
        .create()
        .unwrap();
    let mut args = vec!["trees:nodes".into(), "--no-ansi".into()];
    if std::env::var("TREE_NODES_FAIL").unwrap() == "1" {
        args.push("--fail".into());
    }
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let code = runtime.block_on(app.handle_command(&args)).unwrap();
    drop(runtime);
    drop(root);
    std::process::exit(code);
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    for (path, fail) in [("/tree-nodes", false), ("/tree-nodes/failure", true)] {
        openrust::support::facades::Route::get(path, move || async move {
            let mut output = openrust::console::Output::default();
            let mut args = vec!["--no-ansi".into()];
            if fail {
                args.push("--fail".into());
            }
            openrust::support::facades::Artisan::call("trees:nodes", &args, &mut output)
                .await
                .map_err(|source| openrust::http::HttpError::Internal {
                    source: Box::new(source),
                })?;
            let data: Value = serde_json::from_str(&output.stdout).unwrap();
            openrust::http::Response::json(http::StatusCode::OK, data)
        })?;
    }
    Ok(())
}
#[tokio::test]
async fn real_http_nodes_factory_error_recovers_and_parallel_requests_remain_isolated() {
    use axum::handler::Handler as _;
    use openrust::{
        contracts::http::Kernel as _,
        foundation::{Application, configuration::Routing},
    };
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<openrust::foundation::http::Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(command).web(routes))
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
    for (path, status) in [
        ("/tree-nodes", 200),
        ("/tree-nodes/failure", 500),
        ("/tree-nodes", 200),
    ] {
        let response = reqwest::Client::new()
            .get(format!("http://{address}{path}"))
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert!(
            response.headers()[reqwest::header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("application/json")
        );
        let data: Value = response.json().await.unwrap();
        if status == 200 {
            assert_eq!(data["count"], 3);
            assert_eq!(data["shape"][0], "Report");
        }
    }
    let mut tasks = vec![];
    for _ in 0..12 {
        tasks.push(tokio::spawn(async move {
            let data: Value = reqwest::get(format!("http://{address}/tree-nodes"))
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            assert_eq!(data["count"], 3);
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
}
#[test]
fn failed_expansion_keeps_previous_siblings_without_attaching_partial_branch() {
    let expected: Value = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/tree-node-failure-oracle.json"
    ))
    .unwrap();
    let target = TreeNode::with_children("target", ["prior"]);
    let source = TreeNode::new("source");
    let trace = Arc::new(parking_lot::Mutex::new(vec![]));
    let t = trace.clone();
    let weak = target.downgrade();
    source.add_child(TreeChild::lazy(move || {
        let t = t.clone();
        let weak = weak.clone();
        (0..2).map(move |step| {
            t.lock().push(json!([
                if step == 0 { "open" } else { "error" },
                weak.upgrade().unwrap().count().unwrap()
            ]));
            if step == 0 {
                Ok(TreeNode::new("first"))
            } else {
                Err(ConsoleError::new("consumer failure"))
            }
        })
    }));
    let error = TreeNode::from_values(
        [
            (0i64.into(), TreeValue::from("ok")),
            (
                ArrayKey::from("branch"),
                TreeValue::branch([(0i64, TreeValue::from("good")), (1i64, source.into())]),
            ),
        ],
        Some(target.clone()),
    )
    .unwrap_err();
    let actual = json!({"target":target.get_children().map(|node|node.unwrap().to_string()).collect::<Vec<_>>(),"trace":*trace.lock(),"error":error.to_string()});
    assert_eq!(actual, expected);
}
#[test]
fn clearing_callbacks_drops_their_owned_resources_outside_the_child_lock() {
    struct OnDrop(openrust::console::WeakTreeNode);
    impl Drop for OnDrop {
        fn drop(&mut self) {
            if let Some(node) = self.0.upgrade() {
                node.add_child("after-drop");
            }
        }
    }
    let node = TreeNode::new("root");
    let resource = OnDrop(node.downgrade());
    node.add_child(TreeChild::lazy(move || {
        let _keep = &resource;
        std::iter::empty()
    }));
    node.clear_children();
    assert_eq!(
        node.get_children().next().unwrap().unwrap().get_value(),
        "after-drop"
    );
}

#[test]
fn values_consume_the_producer_only_after_previous_children_are_attached() {
    let expected: Value = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/tree-node-incremental-oracle.json"
    ))
    .unwrap();
    let root = TreeNode::new("incremental");
    let owner = root.clone();
    let trace = Arc::new(parking_lot::Mutex::new(vec![]));
    let t = trace.clone();
    let values = (0_i64..3).map(move |step| {
        t.lock().push(owner.count().unwrap());
        (step, format!("item{step}"))
    });
    let made = TreeNode::from_values(values, Some(root)).unwrap();
    assert_eq!(
        json!({"trace":*trace.lock(),"count":made.count().unwrap()}),
        expected
    );
}
#[test]
fn fallible_input_producer_preserves_previous_values_and_original_error() {
    let expected: Value = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/tree-node-input-failure-oracle.json"
    ))
    .unwrap();
    let root = TreeNode::new("input");
    let error = TreeNode::try_from_values(
        [
            Ok((0_i64, "first")),
            Err(ConsoleError::new("input failure")),
        ],
        Some(root.clone()),
    )
    .unwrap_err();
    assert_eq!(
        json!({"values":root.get_children().map(|node|node.unwrap().to_string()).collect::<Vec<_>>(),"error":error.to_string()}),
        expected
    );
}
#[test]
fn nested_iterable_values_are_lazy_and_attach_only_after_complete_expansion() {
    let expected: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/tree-node-nested-oracle.json"
    ))
    .unwrap();
    for case in expected {
        let fail = case["fail"].as_bool().unwrap();
        let root = TreeNode::with_children("target", ["prior"]);
        let trace = Arc::new(parking_lot::Mutex::new(vec![]));
        let t = trace.clone();
        let weak = root.downgrade();
        let iterator = (0..2).map(move |step| {
            t.lock().push(json!([
                if step == 0 { "open" } else { "next" },
                weak.upgrade().unwrap().count().unwrap()
            ]));
            if step == 0 {
                Ok((0i64.into(), TreeValue::from("first")))
            } else if fail {
                Err(ConsoleError::new("nested failure"))
            } else {
                Ok((
                    ArrayKey::from("deep"),
                    TreeValue::iterable([(0i64, "leaf")]),
                ))
            }
        });
        let values = [
            (0i64.into(), TreeValue::from("ok")),
            (ArrayKey::from("branch"), TreeValue::try_iterable(iterator)),
            (1i64.into(), TreeValue::from("last")),
        ];
        assert!(trace.lock().is_empty());
        let error = TreeNode::from_values(values, Some(root.clone()))
            .err()
            .map(|error| error.to_string());
        assert_eq!(
            json!({"fail":fail,"trace":*trace.lock(),"shape":shape(&root),"error":error}),
            case
        );
    }
}
struct ReentrantChild(openrust::console::WeakTreeNode);
impl From<ReentrantChild> for TreeChild {
    fn from(child: ReentrantChild) -> Self {
        child.0.upgrade().unwrap().add_child("converted");
        TreeChild::from("outer")
    }
}
#[test]
#[ignore = "由父消费者隔离执行，死锁反例有明确超时和进程回收"]
fn tree_node_conversion_driver() {
    let node = TreeNode::new("conversion");
    node.add_child(ReentrantChild(node.downgrade()));
    let labels: Vec<_> = node
        .get_children()
        .map(|child| child.unwrap().to_string())
        .collect();
    assert_eq!(labels, ["converted", "outer"]);
}
#[test]
fn child_conversion_runs_outside_the_node_lock() {
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tree_node_conversion_driver",
            "--ignored",
            "--nocapture",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            let result = child.wait_with_output().unwrap();
            assert!(
                status.success(),
                "{}{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            return;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "子节点转换发生重入死锁；已终止并回收隔离子进程：{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
