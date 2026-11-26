use openrust::{
    console::{
        ConsoleError, ConsoleTreeError, Output, TreeChild, TreeHelper, TreeNode, TreeStyle,
        TreeValue,
    },
    support::ArrayKey,
};
use serde_json::{Value, json};
use std::{error::Error as _, sync::Arc};
fn tag(error: &ConsoleError) -> Value {
    match error
        .source()
        .and_then(|error| error.downcast_ref::<ConsoleTreeError>())
    {
        Some(ConsoleTreeError::InvalidCurrent) => json!("current"),
        Some(ConsoleTreeError::InvalidKey) => json!("key"),
        Some(ConsoleTreeError::CannotRewind) => json!("rewind"),
        Some(ConsoleTreeError::Cycle(value)) => json!([
            "LogicException",
            format!("Cycle detected at node: \"{value}\".")
        ]),
        None => json!(["RuntimeException", error.to_string()]),
    }
}
struct Generator {
    position: i64,
    max: i64,
    trace: Arc<parking_lot::Mutex<Vec<String>>>,
    closed: bool,
}
impl Generator {
    fn close(&mut self) {
        if !self.closed {
            self.closed = true;
            self.trace.lock().push("close".into());
        }
    }
}
impl Drop for Generator {
    fn drop(&mut self) {
        self.close()
    }
}
impl Iterator for Generator {
    type Item = Result<TreeNode, ConsoleError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.position >= self.max {
            self.close();
            return None;
        }
        let n = self.position;
        self.position += 1;
        self.trace.lock().push(format!("yield{n}"));
        Some(Ok(TreeNode::new(format!("n{n}"))))
    }
}
#[test]
fn source_recursive_cursor_mutation_rewind_and_drop_sequences() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/tree-cursor-oracle.json"
    ))
    .unwrap();
    for c in cases {
        let trace = Arc::new(parking_lot::Mutex::new(vec![]));
        let root = TreeNode::new("Root");
        let t = trace.clone();
        let max = c["n"].as_i64().unwrap();
        root.add_child(TreeChild::lazy(move || {
            t.lock().push("open".into());
            Generator {
                position: 0,
                max,
                trace: t.clone(),
                closed: false,
            }
        }));
        let mut output = Output::default();
        let mut helper =
            TreeHelper::create_tree(&mut output, root.clone(), Vec::<(i64, &str)>::new(), None)
                .unwrap();
        trace.lock().push("created".into());
        if c["mutation"] == true {
            root.add_child("late");
        }
        for _ in 0..c["advance"].as_u64().unwrap() {
            helper.advance().unwrap();
        }
        let mut steps = vec![];
        for step in c["steps"].as_array().unwrap() {
            let op = step["op"].as_str().unwrap();
            let result = match op {
                "valid" => Ok(json!(helper.valid())),
                "current" => helper.current().map(|node| json!(node.to_string())),
                "key" => helper.key().map(|key| json!(key)),
                "rewind" => helper.rewind().map(|_| Value::Null),
                "hasChildren" => helper.has_children().map(|value| json!(value)),
                "getChildren" => helper
                    .get_children()
                    .map(|child| json!({"valid":child.valid()})),
                "render" => helper.render().map(|_| Value::Null),
                "next" => helper.advance().map(|_| Value::Null),
                _ => unreachable!(),
            };
            steps.push(match result {
                Ok(value) => json!({"op":op,"value":value,"error":null}),
                Err(error) => json!({"op":op,"value":null,"error":tag(&error)}),
            });
        }
        drop(helper);
        drop(root);
        let got = json!({"n":c["n"],"advance":c["advance"],"mutation":c["mutation"],"steps":steps,"trace":*trace.lock(),"output":output.stdout});
        assert_eq!(got, c, "{c}");
    }
}
fn style(name: &str) -> TreeStyle {
    match name {
        "default" => TreeStyle::default(),
        "box" => TreeStyle::r#box(),
        "boxDouble" => TreeStyle::box_double(),
        "compact" => TreeStyle::compact(),
        "light" => TreeStyle::light(),
        "minimal" => TreeStyle::minimal(),
        "rounded" => TreeStyle::rounded(),
        "custom" => TreeStyle::new("E+", "E-", "L", "M+", "M-", "R"),
        _ => unreachable!(),
    }
}
fn render_fixture(mode: &str, trace: Arc<parking_lot::Mutex<Vec<String>>>) -> TreeNode {
    let root = TreeNode::new("Root");
    let a = TreeNode::new("<info>A</info>\na2");
    match mode {
        "ordinary" => {
            let child = TreeNode::with_children("child", ["leaf"]);
            root.add_child(TreeNode::with_children("A", [child]))
                .add_child("B");
        }
        "multiline" => {
            a.add_child("x\nx2");
            root.add_child(a).add_child("");
        }
        "empty" => {}
        "dag" => {
            root.add_child(a.clone()).add_child(a);
        }
        "cycle" => {
            root.add_child(root.clone());
        }
        "lazy-error" => {
            root.add_child(TreeChild::lazy(move || {
                trace.lock().push("factory".into());
                let trace = trace.clone();
                (0..2).map(move |n| {
                    if n == 0 {
                        Ok(TreeNode::new("one"))
                    } else {
                        trace.lock().push("error".into());
                        Err(ConsoleError::new("failure"))
                    }
                })
            }));
        }
        "child-error" => {
            a.add_child(TreeChild::lazy(move || {
                trace.lock().push("factory".into());
                [Err(ConsoleError::new("failure"))]
            }));
            root.add_child(a).add_child("last");
        }
        "grandchild-error" => {
            a.add_child(TreeChild::lazy(move || {
                trace.lock().push("factory".into());
                let trace = trace.clone();
                (0..2).map(move |n| {
                    if n == 0 {
                        Ok(TreeNode::new("one"))
                    } else {
                        trace.lock().push("error".into());
                        Err(ConsoleError::new("failure"))
                    }
                })
            }));
            root.add_child(a).add_child("last");
        }
        _ => unreachable!(),
    }
    root
}
#[test]
fn all_seven_presets_and_custom_prefixes_match_source_including_error_phases() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/tree-render-oracle.json"
    ))
    .unwrap();
    for c in cases {
        let trace = Arc::new(parking_lot::Mutex::new(vec![]));
        let root = render_fixture(c["mode"].as_str().unwrap(), trace.clone());
        let mut output = Output::default();
        let error = match TreeHelper::create_tree(
            &mut output,
            root.clone(),
            Vec::<(i64, &str)>::new(),
            Some(style(c["style"].as_str().unwrap())),
        ) {
            Ok(mut helper) => {
                trace.lock().push("created".into());
                helper.render().err().map(|error| tag(&error))
            }
            Err(error) => Some(tag(&error)),
        };
        let actual = json!({"style":c["style"],"mode":c["mode"],"trace":*trace.lock(),"error":error,"output":output.stdout});
        root.clear_children();
        assert_eq!(actual, c, "{c}");
    }
}
#[test]
fn cursor_weak_numeric_keys_use_existing_scalar_rules() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/tree-keys-oracle.json"
    ))
    .unwrap();
    for c in cases {
        let key = if let Some(key) = c["input"].as_i64() {
            ArrayKey::Integer(key)
        } else {
            ArrayKey::String(c["input"].as_str().unwrap().into())
        };
        let root = TreeNode::new("root");
        root.add_child(TreeChild::lazy_keyed(move || {
            [Ok((key.clone(), TreeNode::new("child")))]
        }));
        let mut output = Output::default();
        let mut helper =
            TreeHelper::create_tree(&mut output, root, Vec::<(i64, &str)>::new(), None).unwrap();
        let result = helper.key();
        if c["error"].is_null() {
            assert_eq!(result.unwrap(), c["key"].as_i64().unwrap());
            helper.render().unwrap();
        } else {
            assert_eq!(tag(&result.unwrap_err()), json!("key"));
        }
        drop(helper);
        assert_eq!(output.stdout, c["output"].as_str().unwrap());
    }
}
#[test]
fn actual_prefix_application_and_direct_native_iterator_preserve_nodes() {
    let root = TreeNode::with_children("root", ["a", "b"]);
    let mut output = Output::default();
    let mut helper =
        TreeHelper::create_tree(&mut output, root.clone(), Vec::<(i64, &str)>::new(), None)
            .unwrap();
    let first = helper.current().unwrap();
    let mut recursive = helper.recursive_iterator();
    TreeStyle::new("+", "-", "[", "|", " ", "]").apply_prefixes(&mut recursive);
    let items: Vec<_> = recursive.map(|entry| entry.unwrap()).collect();
    assert!(items[0].node.ptr_eq(&first));
    assert_eq!(
        items
            .iter()
            .map(|entry| entry.line.as_str())
            .collect::<Vec<_>>(),
        ["[+]a", "[-]b"]
    );
    let mut other = Output::default();
    let mut helper =
        TreeHelper::create_tree(&mut other, root, Vec::<(i64, &str)>::new(), None).unwrap();
    assert_eq!(
        (&mut helper)
            .into_iter()
            .map(|node| node.unwrap().to_string())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert!(!helper.valid());
    let mut empty = Output::default();
    TreeHelper::create_tree(&mut empty, None, Vec::<(i64, &str)>::new(), None)
        .unwrap()
        .render()
        .unwrap();
    assert_eq!(empty.stdout, "\n");
    let mut values = Output::default();
    TreeHelper::create_tree(&mut values, "root", [(0_i64, TreeValue::from("a"))], None)
        .unwrap()
        .render()
        .unwrap();
    assert_eq!(values.stdout, "root\n└── a\n");
}
fn scene_values() -> openrust::console::TreeValues {
    vec![
        (0i64.into(), TreeValue::from("A")),
        (
            ArrayKey::from("Group"),
            TreeValue::branch([
                (0i64.into(), TreeValue::from("x")),
                (ArrayKey::from("leaf"), TreeValue::branch([(0i64, "y")])),
            ]),
        ),
        (1i64.into(), TreeValue::from("B")),
    ]
}
trait TreeScene: openrust::console::AsOutput {
    fn write_before(&mut self) -> Result<(), ConsoleError>;
    fn tree(
        &mut self,
        values: openrust::console::TreeValues,
        root: &str,
    ) -> Result<(), ConsoleError>;
    fn create(
        &mut self,
        values: openrust::console::TreeValues,
        root: &str,
    ) -> Result<TreeHelper<'_>, ConsoleError>;
    fn after(&mut self) -> Result<(), ConsoleError>;
    fn counts(&self) -> Value;
    fn errors(&mut self) -> openrust::console::ConsoleStyle<'_>;
}
macro_rules! tree_scene {
    ($ty:ident,$counts:expr) => {
        impl TreeScene for openrust::console::$ty<'_> {
            fn write_before(&mut self) -> Result<(), ConsoleError> {
                self.write("Before", false, openrust::console::OutputOptions::default())
            }
            fn tree(
                &mut self,
                values: openrust::console::TreeValues,
                root: &str,
            ) -> Result<(), ConsoleError> {
                openrust::console::$ty::tree(self, values, root)
            }
            fn create(
                &mut self,
                values: openrust::console::TreeValues,
                root: &str,
            ) -> Result<TreeHelper<'_>, ConsoleError> {
                self.create_tree(values, root)
            }
            fn after(&mut self) -> Result<(), ConsoleError> {
                self.text("After")
            }
            fn counts(&self) -> Value {
                ($counts)(self)
            }
            fn errors(&mut self) -> openrust::console::ConsoleStyle<'_> {
                self.get_error_style()
            }
        }
    };
}
tree_scene!(
    OutputStyle,
    |style: &openrust::console::OutputStyle<'_>| json!([
        style.new_lines_written(),
        style.new_line_written()
    ])
);
tree_scene!(ConsoleStyle, |_: &openrust::console::ConsoleStyle<'_>| {
    Value::Null
});
fn style_action(style: &mut dyn TreeScene, c: &Value) -> Value {
    if c["before"] == true {
        style.write_before().unwrap();
    }
    let meta = if c["mode"] == "tree" {
        style.tree(scene_values(), "").unwrap();
        Value::Null
    } else {
        let mut tree = if c["mode"] == "direct" {
            TreeHelper::create_tree(style, "<info>Root</info>", scene_values(), None).unwrap()
        } else {
            style.create(scene_values(), "<info>Root</info>").unwrap()
        };
        let data = json!({"current":tree.current().unwrap().to_string(),"key":tree.key().unwrap(),"has_children":tree.has_children().unwrap()});
        tree.render().unwrap();
        data
    };
    style.after().unwrap();
    meta
}
fn style_chain(
    output: &mut dyn openrust::console::AsOutput,
    input: &openrust::console::Input,
    shape: &[u8],
    c: &Value,
) -> (Value, Vec<Value>) {
    macro_rules! layer {
        ($ty:ident) => {{
            let mut style = openrust::console::$ty::new(input, output);
            let (meta, mut counts) = if shape.len() == 1 {
                (
                    if c["error"] == true {
                        style_action(&mut style.errors(), c)
                    } else {
                        style_action(&mut style, c)
                    },
                    vec![],
                )
            } else {
                style_chain(&mut style, input, &shape[1..], c)
            };
            counts.insert(0, TreeScene::counts(&style));
            (meta, counts)
        }};
    }
    if shape[0] == b'L' {
        layer!(OutputStyle)
    } else {
        layer!(ConsoleStyle)
    }
}
#[test]
fn both_style_factories_keep_selected_sections_and_mixed_newline_boundaries() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/tree-style-oracle.json"
    ))
    .unwrap();
    let input = openrust::console::CommandDefinition::from_signature("trees")
        .unwrap()
        .parse(&[])
        .unwrap();
    for c in cases {
        let mut output = Output::default();
        output.set_decorated(c["decorated"].as_bool().unwrap());
        output.set_verbosity(match c["level"].as_u64().unwrap() {
            32 => openrust::console::OutputVerbosity::Normal,
            16 => openrust::console::OutputVerbosity::Quiet,
            8 => openrust::console::OutputVerbosity::Silent,
            _ => unreachable!(),
        });
        let (meta, counts) = if c["section"] == true {
            let mut section = output.section();
            style_chain(
                &mut section,
                &input,
                c["shape"].as_str().unwrap().as_bytes(),
                &c,
            )
        } else {
            style_chain(
                &mut output,
                &input,
                c["shape"].as_str().unwrap().as_bytes(),
                &c,
            )
        };
        let mut actual = c.clone();
        actual["meta"] = meta;
        actual["counts"] = json!(counts);
        actual["output"] = json!(output.stdout);
        assert_eq!(actual, c, "{c}");
    }
}
struct ProbeIterator {
    position: usize,
    call: usize,
    mode: String,
    trace: Arc<parking_lot::Mutex<Vec<String>>>,
    closed: bool,
}
impl ProbeIterator {
    fn close(&mut self) {
        if !self.closed {
            self.closed = true;
            self.trace.lock().push(format!("close{}", self.call));
        }
    }
}
impl Drop for ProbeIterator {
    fn drop(&mut self) {
        self.close();
    }
}
impl Iterator for ProbeIterator {
    type Item = Result<TreeNode, ConsoleError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.position == 0 {
            self.position = 1;
            if self.mode == "probe-fails" || (self.mode == "second-fails" && self.call == 2) {
                self.close();
                return Some(Err(ConsoleError::new("probe failure")));
            }
            if self.mode != "empty" || self.call == 1 {
                return Some(Ok(TreeNode::new("child")));
            }
            self.position = 2;
        }
        if self.position == 1 {
            self.position = 2;
            self.trace.lock().push(format!("after{}", self.call));
        }
        self.close();
        None
    }
}
#[test]
fn changing_factory_results_and_visited_object_lifetimes_match_source() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/tree-lifecycle-oracle.json"
    ))
    .unwrap();
    for c in cases.iter().filter(|c| c["mode"] != "weak") {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let trace = Arc::new(parking_lot::Mutex::new(vec![]));
        let t = trace.clone();
        let mode = c["mode"].as_str().unwrap().to_owned();
        let a = TreeNode::new("A");
        a.add_child(TreeChild::lazy(move || {
            let call = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            t.lock().push(format!("call{call}"));
            ProbeIterator {
                position: 0,
                call,
                mode: mode.clone(),
                trace: t.clone(),
                closed: false,
            }
        }));
        let root = TreeNode::with_children("Root", [TreeChild::from(a), TreeChild::from("B")]);
        let mut output = Output::default();
        let mut helper =
            TreeHelper::create_tree(&mut output, root, Vec::<(i64, &str)>::new(), None).unwrap();
        let error = helper.render().err().map(|error| error.to_string());
        drop(helper);
        assert_eq!(
            json!({"mode":c["mode"],"trace":*trace.lock(),"error":error,"output":output.stdout}),
            *c
        );
    }
    let c = cases.last().unwrap();
    let weak = Arc::new(parking_lot::Mutex::new(Vec::<
        openrust::console::WeakTreeNode,
    >::new()));
    let checked = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let w = weak.clone();
    let check = checked.clone();
    let root = TreeNode::new("Root");
    root.add_child(TreeChild::lazy(move || {
        let w = w.clone();
        let check = check.clone();
        (0..256).map(move |n| {
            if n > 2 && w.lock()[0].upgrade().is_none() {
                check.store(false, std::sync::atomic::Ordering::SeqCst);
            }
            let node = TreeNode::new("same");
            w.lock().push(node.downgrade());
            Ok(node)
        })
    }));
    let mut output = Output::default();
    let mut helper =
        TreeHelper::create_tree(&mut output, root, Vec::<(i64, &str)>::new(), None).unwrap();
    helper.render().unwrap();
    let released = weak.lock().iter().all(|node| node.upgrade().is_none());
    let held = checked.load(std::sync::atomic::Ordering::SeqCst);
    drop(helper);
    assert!(weak.lock().iter().all(|node| node.upgrade().is_none()));
    assert_eq!(
        json!({"mode":"weak","held_during_render":held,"released_after_render":released,"output":output.stdout}),
        *c
    );
}
fn register_trees() -> Result<(), ConsoleError> {
    openrust::support::facades::Artisan::command_with(
        "trees:show {--mode=create} {--fail}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                if input.option("fail").and_then(|value| value.as_bool()) == Some(true) {
                    let root = TreeNode::new("Failure");
                    let child = TreeNode::new("same");
                    root.add_child(child.clone()).add_child(child);
                    return TreeHelper::create_tree(output, root, Vec::<(i64, &str)>::new(), None)?
                        .render()
                        .map(|_| 0);
                }
                let c=std::env::var("TREE_CASE").ok().map(|value|serde_json::from_str::<Value>(&value).unwrap()).unwrap_or_else(||json!({"mode":input.option("mode").and_then(|value|value.as_str().map(str::to_owned)).unwrap_or_else(||"create".into()),"shape":"L","error":false,"decorated":false,"level":32,"before":true}));
                let (meta, counts) =
                    style_chain(output, input, c["shape"].as_str().unwrap().as_bytes(), &c);
                if std::env::var_os("TREE_CASE").is_some() {
                    eprintln!("TREE_META{}", json!({"meta":meta,"counts":counts}));
                }
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
#[test]
#[ignore = "由父消费者运行真实Artisan子进程并核对物理stdout/stderr"]
fn tree_console_cli_driver() {
    use openrust::foundation::{Application, configuration::Routing};
    use std::io::Write as _;
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register_trees))
        .create()
        .unwrap();
    let c: Value = serde_json::from_str(&std::env::var("TREE_CASE").unwrap()).unwrap();
    let mut args = vec![
        "trees:show".into(),
        "--no-interaction".into(),
        if c["decorated"] == true {
            "--ansi".into()
        } else {
            "--no-ansi".into()
        },
    ];
    if c["level"] == 16 {
        args.push("--quiet".into());
    }
    if c["fatal"] == true {
        args.push("--fail".into());
    }
    print!("TREE_BEGIN");
    std::io::stdout().flush().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let code = runtime.block_on(app.handle_command(&args)).unwrap();
    drop(runtime);
    drop(root);
    std::process::exit(code);
}
#[test]
fn actual_console_artisan_matches_source_streams_and_factory_class_identity() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/tree-console-oracle.json"
    ))
    .unwrap();
    for c in cases {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tree_console_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("TREE_CASE", c.to_string())
            .env("COLUMNS", "80")
            .output()
            .unwrap();
        assert_eq!(
            result.status.code(),
            Some(0),
            "{c} {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let stdout = String::from_utf8(result.stdout).unwrap();
        let stdout = stdout.split_once("TREE_BEGIN").unwrap().1;
        let stderr = String::from_utf8(result.stderr).unwrap();
        let (stderr, meta) = stderr.rsplit_once("TREE_META").unwrap();
        assert_eq!(stdout, c["stdout"].as_str().unwrap(), "{c}");
        assert_eq!(stderr, c["stderr"].as_str().unwrap(), "{c}");
        assert_eq!(
            serde_json::from_str::<Value>(meta).unwrap(),
            c["meta"],
            "{c}"
        );
    }
}
#[test]
fn actual_artisan_duplicate_node_failure_keeps_partial_output_and_exits_one() {
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tree_console_cli_driver",
            "--ignored",
            "--nocapture",
        ])
        .env(
            "TREE_CASE",
            json!({"decorated":false,"level":32,"fatal":true}).to_string(),
        )
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8(result.stdout).unwrap();
    assert!(
        stdout
            .split_once("TREE_BEGIN")
            .unwrap()
            .1
            .starts_with("Failure\n├── same\n")
    );
}
fn tree_routes() -> Result<(), openrust::routing::RoutingError> {
    for (path, mode, fail) in [
        ("/trees", "create", false),
        ("/trees/direct", "direct", false),
        ("/trees/failure", "create", true),
    ] {
        openrust::support::facades::Route::get(path, move || async move {
            let mut output = Output::default();
            let mut args: Vec<std::ffi::OsString> =
                vec![format!("--mode={mode}").into(), "--no-ansi".into()];
            if fail {
                args.push("--fail".into());
            }
            openrust::support::facades::Artisan::call("trees:show", &args, &mut output)
                .await
                .map_err(|source| openrust::http::HttpError::Internal {
                    source: Box::new(source),
                })?;
            openrust::http::Response::json(http::StatusCode::OK, json!({"output":output.stdout}))
        })?;
    }
    Ok(())
}
#[tokio::test]
async fn real_http_tree_factories_cycle_errors_recover_and_parallel_requests_are_isolated() {
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
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/tree-style-oracle.json"
    ))
    .unwrap();
    let expected = cases
        .into_iter()
        .find(|case| {
            case["mode"] == "create"
                && case["shape"] == "L"
                && case["section"] == false
                && case["level"] == 32
                && case["decorated"] == false
                && case["before"] == true
        })
        .unwrap()["output"]
        .clone();
    let console: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/tree-console-oracle.json"
    ))
    .unwrap();
    let direct = console
        .into_iter()
        .find(|case| {
            case["mode"] == "direct"
                && case["shape"] == "L"
                && case["error"] == false
                && case["level"] == 32
                && case["decorated"] == false
        })
        .unwrap()["stdout"]
        .clone();
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register_trees).web(tree_routes))
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
        ("/trees", 200),
        ("/trees/direct", 200),
        ("/trees/failure", 500),
        ("/trees", 200),
    ] {
        let response = reqwest::Client::new()
            .get(format!("http://{address}{path}"))
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        let body: Value = response.json().await.unwrap();
        if status == 200 {
            assert_eq!(
                body["output"],
                if path == "/trees/direct" {
                    direct.clone()
                } else {
                    expected.clone()
                }
            );
        }
    }
    let mut tasks = vec![];
    for _ in 0..12 {
        let expected = expected.clone();
        tasks.push(tokio::spawn(async move {
            let body: Value = reqwest::get(format!("http://{address}/trees"))
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            assert_eq!(body["output"], expected);
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
}
#[test]
fn rendering_validates_generator_keys_after_the_root_is_written() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/tree-key-render-oracle.json"
    ))
    .unwrap();
    for c in cases {
        let key = if let Some(key) = c["key"].as_i64() {
            ArrayKey::Integer(key)
        } else {
            ArrayKey::String(c["key"].as_str().unwrap().into())
        };
        let root = TreeNode::new("Root");
        root.add_child(TreeChild::lazy_keyed(move || {
            [Ok((key.clone(), TreeNode::new("child")))]
        }));
        let mut output = Output::default();
        let error = TreeHelper::create_tree(&mut output, root, Vec::<(i64, &str)>::new(), None)
            .unwrap()
            .render()
            .err();
        assert_eq!(error.is_some(), !c["error"].is_null(), "{c}");
        if let Some(error) = error {
            assert_eq!(tag(&error), json!("key"));
        }
        assert_eq!(output.stdout, c["output"].as_str().unwrap());
    }
}
