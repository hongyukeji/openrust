use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "openrust-view-finder-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

use openrust::view::{FileViewFinder, ViewError, ViewFinder};

#[test]
fn finder_resolves_ordered_locations_extensions_and_namespaces() {
    let root = TempDir::new();
    let first = root.path().join("first");
    let second = root.path().join("second");
    for path in [&first, &second] {
        fs::create_dir_all(path.join("admin")).unwrap();
    }
    let fallback = second.join("admin/index.html");
    let primary = first.join("admin/index.html");
    let compiled = first.join("admin/index.blade.html");
    fs::write(&fallback, "fallback").unwrap();
    fs::write(&primary, "primary").unwrap();
    fs::write(&compiled, "compiled").unwrap();

    let mut finder = FileViewFinder::new([first.clone(), second.clone()]);
    assert_eq!(finder.find(" admin.index ").unwrap(), primary);
    finder.add_extension("blade.html").unwrap();
    assert_eq!(finder.find("admin.index").unwrap(), compiled);
    fs::remove_file(&compiled).unwrap();
    assert_eq!(finder.find("admin.index").unwrap(), primary);
    fs::remove_file(&primary).unwrap();
    assert_eq!(finder.find("admin.index").unwrap(), fallback);

    fs::write(&primary, "primary").unwrap();
    finder.flush();
    assert_eq!(finder.find("admin.index").unwrap(), primary);
    finder.add_namespace("package", &[&second]).unwrap();
    assert_eq!(finder.find("package::admin.index").unwrap(), fallback);
    finder.prepend_namespace("package", &[&first]).unwrap();
    assert_eq!(finder.find("package::admin.index").unwrap(), primary);
    finder.replace_namespace("package", &[&second]).unwrap();
    assert_eq!(finder.find("package::admin.index").unwrap(), fallback);
    assert!(matches!(
        finder.find("unknown::admin.index"),
        Err(ViewError::NotFound)
    ));
    for invalid in [
        "../admin",
        "/admin",
        "package::other::index",
        "admin\\index",
    ] {
        assert!(matches!(finder.find(invalid), Err(ViewError::InvalidName)));
    }
}

#[cfg(unix)]
#[test]
fn finder_rejects_symlinks_outside_a_configured_root() {
    let root = TempDir::new();
    let views = root.path().join("views");
    fs::create_dir(&views).unwrap();
    let outside = root.path().join("secret.html");
    fs::write(&outside, "outside").unwrap();
    std::os::unix::fs::symlink(outside, views.join("escape.html")).unwrap();
    let finder = FileViewFinder::new([views]);
    assert!(matches!(finder.find("escape"), Err(ViewError::OutsideRoot)));
}

#[test]
fn blade_compiles_escapes_and_refreshes_a_cached_template() {
    use openrust::view::{
        Factory, ViewData,
        compilers::{Compiler as _, DirectiveOutput},
    };
    use serde_json::json;

    let root = TempDir::new();
    let directory = root.path().join("views");
    let cache = root.path().join("cache");
    fs::create_dir(&directory).unwrap();
    let plain = directory.join("welcome.html");
    let blade = directory.join("welcome.blade.html");
    fs::write(&plain, "plain fallback").unwrap();
    fs::write(
        &blade,
        r#"{{ $user->name }}|{!! $trusted !!}|@{{ client }}|{{-- ignored --}}@if ($none)bad{{ '' }}@elseif ($yes)yes{{ '' }}@else bad{{ '' }}@endif|@foreach ($items as $item){{ $item }}@endforeach|@forelse ($missing  as  $item){{ $item }}@empty none{{ '' }}@endforelse|@isset ($yes)ready{{ '' }}@endisset|@empty ($missing)empty{{ '' }}@endempty"#,
    )
    .unwrap();
    let factory = Factory::new(FileViewFinder::new([directory]), cache);
    let data = ViewData::from_iter([
        ("user".to_owned(), json!({"name": "<&'\""}).into()),
        ("trusted".to_owned(), json!("<em>safe</em>").into()),
        ("yes".to_owned(), json!(true).into()),
        ("none".to_owned(), json!(false).into()),
        ("missing".to_owned(), json!([]).into()),
        ("items".to_owned(), json!(["one", "<two>"]).into()),
    ]);
    let engine = factory.get_engine_resolver().resolve("blade").unwrap();
    let openrust::view::engines::Engine::Blade(engine) = engine else {
        panic!("blade resolves to the compiled engine");
    };
    let compiler = engine.get_compiler();
    assert!(compiler.is_expired(&blade).unwrap());
    assert_eq!(
        factory.render("welcome", &data).unwrap(),
        "&lt;&amp;&#039;&quot;|<em>safe</em>|{{ client }}|yes|one&lt;two&gt;| none|ready|empty"
    );
    assert!(!compiler.is_expired(&blade).unwrap());
    let artifact = compiler.get_compiled_path(&blade).unwrap();
    assert!(artifact.is_file());
    let mut legacy: serde_json::Value =
        serde_json::from_slice(&fs::read(&artifact).unwrap()).unwrap();
    legacy.as_object_mut().unwrap().remove("compiler_version");
    fs::write(&artifact, serde_json::to_vec(&legacy).unwrap()).unwrap();
    assert!(compiler.is_expired(&blade).unwrap());
    compiler.compile(&blade).unwrap();
    assert!(!compiler.is_expired(&blade).unwrap());
    compiler
        .r#if("ready", |arguments| Ok(arguments.is_empty()))
        .unwrap();
    assert!(compiler.is_expired(&blade).unwrap());
    assert!(compiler.check("ready", &[]).unwrap());
    assert!(compiler.check("unknown", &[]).is_err());
    fs::write(&blade, "@formatName($user->name)").unwrap();
    assert_eq!(
        factory.render("welcome", &data).unwrap(),
        "@formatName($user->name)"
    );
    compiler
        .directive("formatName", |args| {
            Ok(DirectiveOutput::Text(args[0].as_str().unwrap().to_owned()))
        })
        .unwrap();
    assert!(compiler.is_expired(&blade).unwrap());
    compiler.compile(&blade).unwrap();
    assert!(!compiler.is_expired(&blade).unwrap());
    assert_eq!(
        factory.render("welcome", &data).unwrap(),
        "&lt;&amp;&#039;&quot;"
    );
    assert_eq!(
        factory
            .render_inline(r#"<b>@formatName($user->name)</b>"#, &data)
            .unwrap(),
        "<b>&lt;&amp;&#039;&quot;</b>"
    );
    assert!(matches!(
        factory.render_inline("@formatName($x + 1)", &data),
        Err(ViewError::RenderFailed(_))
    ));
    fs::write(&blade, "@unless ($none)updated{{ '' }}@endunless").unwrap();
    assert!(compiler.is_expired(&blade).unwrap());
    assert_eq!(factory.render("welcome", &data).unwrap(), "updated");
    fs::write(&artifact, "incomplete data").unwrap();
    assert_eq!(factory.render("welcome", &data).unwrap(), "updated");
    fs::write(&blade, "@if ($yes)broken").unwrap();
    assert!(matches!(
        factory.render("welcome", &data),
        Err(ViewError::CompileFailed(_))
    ));
    fs::write(&blade, "@csrf").unwrap();
    assert!(matches!(
        factory.render("welcome", &data),
        Err(ViewError::RenderFailed(_))
    ));
    compiler.clear().unwrap();
    assert!(!artifact.exists());
    fs::remove_file(&blade).unwrap();
    assert_eq!(factory.render("welcome", &data).unwrap(), "plain fallback");
}

#[test]
fn view_contracts_resolve_files_namespaces_and_engines() {
    use openrust::{
        contracts::view::{Engine as _, Factory as ViewFactory},
        view::{Factory, ViewData},
    };
    use std::sync::{Arc, Mutex};

    let root = TempDir::new();
    let app = root.path().join("app");
    let old = root.path().join("old");
    let replacement = root.path().join("replacement");
    for folder in [&app, &old, &replacement] {
        fs::create_dir(folder).unwrap();
    }
    let explicit = app.join("literal.blade.html");
    fs::write(&explicit, "{{ $title }}|{{ $base }}").unwrap();
    fs::write(old.join("index.html"), "old").unwrap();
    fs::write(replacement.join("index.html"), "new").unwrap();
    let factory = Factory::new(
        FileViewFinder::new([app.clone()]),
        root.path().join("compiled"),
    );
    let contract: &dyn ViewFactory = &factory;
    let trace = Arc::new(Mutex::new(Vec::new()));
    let record = trace.clone();
    contract.creator(
        &["literal", "pkg::index"],
        Arc::new(move |view| {
            record.lock().unwrap().push("created");
            view.with("base", "creator".into());
            Ok(())
        }),
    );
    let record = trace.clone();
    contract.composer(
        &["literal"],
        Arc::new(move |view| {
            record.lock().unwrap().push("composed");
            assert_eq!(view.get_data()["base"], "creator");
            Ok(())
        }),
    );
    assert!(contract.exists("literal"));
    assert!(!contract.exists("missing"));
    let mut view = contract
        .make(
            "literal",
            ViewData::from_iter([("title".into(), "<data>".into())]),
            ViewData::from_iter([
                ("title".into(), "merged".into()),
                ("base".into(), "merged".into()),
            ]),
        )
        .unwrap();
    assert_eq!(view.name(), "literal");
    assert_eq!(view.render().unwrap(), "&lt;data&gt;|creator");
    view.with("title", "again".into());
    assert_eq!(view.render().unwrap(), "again|creator");
    assert_eq!(*trace.lock().unwrap(), ["created", "composed", "composed"]);

    assert_eq!(contract.share("base", "shared".into()), "shared");
    let mut file = contract
        .file(
            Path::new("literal.blade.html"),
            ViewData::new(),
            ViewData::new(),
        )
        .unwrap();
    file.with("title", "file".into());
    assert_eq!(file.render().unwrap(), "file|shared");
    let outside = root.path().join("outside.html");
    fs::write(&outside, "private").unwrap();
    assert!(matches!(
        contract.file(&outside, ViewData::new(), ViewData::new()),
        Err(ViewError::OutsideRoot)
    ));
    assert!(matches!(
        contract.file(
            Path::new("../outside.html"),
            ViewData::new(),
            ViewData::new()
        ),
        Err(ViewError::OutsideRoot)
    ));
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, app.join("escape.html")).unwrap();
        assert!(matches!(
            contract.file(Path::new("escape.html"), ViewData::new(), ViewData::new()),
            Err(ViewError::OutsideRoot)
        ));
    }
    assert!(matches!(
        contract.file(Path::new("missing.html"), ViewData::new(), ViewData::new()),
        Err(ViewError::NotFound)
    ));

    contract.add_namespace("pkg", &[&old]).unwrap();
    let mut namespaced = contract
        .make("pkg::index", ViewData::new(), ViewData::new())
        .unwrap();
    assert_eq!(namespaced.render().unwrap(), "old");
    contract.replace_namespace("pkg", &[&replacement]).unwrap();
    assert_eq!(namespaced.render().unwrap(), "new");
    assert!(matches!(
        contract.add_namespace("../invalid", &[&old]),
        Err(ViewError::InvalidName)
    ));

    assert!(matches!(
        contract.file(
            Path::new("unsupported.txt"),
            ViewData::new(),
            ViewData::new()
        ),
        Err(ViewError::UnknownEngine)
    ));
    let engine = factory.get_engine_resolver().resolve("blade").unwrap();
    let data = ViewData::from_iter([
        ("title".into(), "engine".into()),
        ("base".into(), "safe".into()),
    ]);
    assert_eq!(engine.get(&app, &explicit, &data).unwrap(), "engine|safe");
    assert!(matches!(
        engine.get(&app, &outside, &data),
        Err(ViewError::OutsideRoot)
    ));
}

fn runtime_css_view() -> openrust::view::View {
    openrust::view::view("runtime_css")
        .with("caption", "<tag>")
        .with(
            "classes",
            serde_json::json!({
                "00": "base", "active": true, "hidden": "0", "1e2": 9, "safe<&": 1,
            }),
        )
        .with(
            "styles",
            serde_json::json!({
                "0": "display: block;;", "color: red": true, "border: 1": false,
            }),
        )
}

#[tokio::test]
async fn layouts_and_composers_isolate_concurrent_http_requests() {
    use axum::{extract::State, handler::Handler as _};
    use openrust::{
        auth::{
            AuthError, AuthFuture, AuthIdentifier, AuthManager, AuthResult,
            access::{Argument, GateRegistry, Policy, PolicyTarget, Response as Decision},
        },
        container::ServiceKey,
        contracts::{
            auth::Authenticatable, container::Container as _, foundation::Application as _,
            http::Kernel as _,
        },
        foundation::{Application, configuration::Routing, http::Kernel},
        http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
        support::facades::{Route, View as ViewFacade},
        view::{
            Component, ComponentOutput, Factory, ViewData, ViewResult, compilers::DirectiveOutput,
            view,
        },
    };
    use std::{path::PathBuf, sync::Arc};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    struct ViewIdentity;
    struct ViewSummary(&'static str);
    struct Price(u16);
    struct HiddenCard;

    struct PricePolicy(bool);

    impl Policy<Price> for PricePolicy {
        fn has_ability(&self, ability: &str) -> bool {
            matches!(ability, "view-price" | "create-price")
        }

        fn evaluate<'a>(
            &'a self,
            ability: &'a str,
            _: Option<&'a dyn Authenticatable>,
            price: Option<&'a Price>,
            arguments: &'a [&'a Argument],
        ) -> AuthFuture<'a, AuthResult<Option<Decision>>> {
            Box::pin(async move {
                tokio::task::yield_now().await;
                let allowed = match ability {
                    "create-price" => price.is_none(),
                    "view-price" => {
                        self.0
                            && price.is_some_and(|price| price.0 == 42)
                            && arguments
                                .last()
                                .and_then(|argument| {
                                    argument.downcast_ref::<openrust::view::ViewValue>()
                                })
                                .and_then(openrust::view::ViewValue::as_str)
                                == Some("retail")
                    }
                    _ => false,
                };
                Ok(Some(Decision::from(allowed)))
            })
        }
    }

    impl Component for HiddenCard {
        fn should_render(&self) -> bool {
            false
        }

        fn render(&self) -> ViewResult<ComponentOutput> {
            panic!("a hidden component must not render")
        }
    }

    impl Authenticatable for ViewIdentity {
        fn get_auth_identifier_name(&self) -> &str {
            "id"
        }
        fn get_auth_identifier(&self) -> AuthIdentifier {
            AuthIdentifier::String("blade".into())
        }
        fn get_auth_password_name(&self) -> &str {
            "password"
        }
        fn get_auth_password(&self) -> Option<&str> {
            None
        }
        fn get_remember_token_name(&self) -> &str {
            "remember_token"
        }
        fn get_remember_token(&self) -> Option<&str> {
            None
        }
        fn set_remember_token(&mut self, _: Option<String>) {}
    }

    struct Badge;
    impl Component for Badge {
        fn render(&self) -> ViewResult<ComponentOutput> {
            Ok(ComponentOutput::InlineTemplate(
                "<mark>{{ $text }}</mark>".to_owned(),
            ))
        }
    }

    struct ScopedPanel {
        caption: String,
        render_id: usize,
    }

    impl Component for ScopedPanel {
        fn data(&self) -> ViewData {
            ViewData::from_iter([
                ("caption".into(), self.caption.clone().into()),
                ("renderId".into(), self.render_id.into()),
            ])
        }

        fn render(&self) -> ViewResult<ComponentOutput> {
            Ok(ComponentOutput::InlineTemplate(
                "<article>{{ $caption }}#{{ $renderId }}:{{ $heading }}:{{ $slot }}</article>"
                    .to_owned(),
            ))
        }
    }

    fn render_card(factory: &dyn openrust::contracts::view::Factory) -> ViewResult<String> {
        let mut view = factory.make("contract_card", ViewData::new(), ViewData::new())?;
        view.with("title", "<card>".into());
        view.render()
    }

    fn contract_route(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            let factory = ViewFacade::contract()?;
            let html = tokio::task::spawn_blocking(move || render_card(factory.as_ref())).await??;
            Ok(Response::new(StatusCode::OK, Body::from(html)))
        })
    }

    async fn transport(
        State(kernel): State<Arc<Kernel>>,
        incoming: axum::extract::Request,
    ) -> axum::response::Response {
        let (parts, body) = incoming.into_parts();
        let (parts, body) = kernel
            .handle(&mut Request::from_parts(parts, body))
            .await
            .into_parts();
        axum::response::Response::from_parts(parts, body)
    }

    async fn start(
        path: PathBuf,
        app: &'static str,
    ) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        let package_views = path.join("resources/views/package");
        let extra_components = path.join("extra-components");
        let mut application = Application::configure(path)
            .with_config(|_, _| {
                Ok([("auth".to_owned(), serde_json::json!({
                "defaults":{"guard":"web"},
                "guards":{"web":{"driver":"view-default"}, "alternate":{"driver":"view-alternate"}}
            }))].into())
            })
            .with_routing(Routing::default().web(|| {
                Route::get("/page", || view("page").with("title", "route"))?;
                Route::get("/plain", || view("layout"))?;
                Route::get("/contract", contract_route)?;
                Route::get("/missing-include", || view("missing_include"))?;
                Route::get("/loops", || {
                    view("loops").with(
                        "groups",
                        serde_json::json!({"<first>": ["<&", "two"], "second": []}),
                    )
                })?;
                Route::get("/broken-loop", || view("broken_loop"))?;
                Route::get("/dynamic", || {
                    view("dynamic")
                        .with("componentName", "accordion")
                        .with("label", "<title>")
                })?;
                Route::get("/dynamic-invalid", || {
                    view("dynamic").with("componentName", "../accordion")
                })?;
                Route::get("/dynamic-missing", || {
                    view("dynamic").with("componentName", "missing")
                })?;
                Route::get("/props", || view("props").with("userValue", "\"<&"))?;
                Route::get("/invalid-props", || view("invalid_props"))?;
                Route::get("/invalid-attributes", || {
                    view("invalid_attributes").with("array", serde_json::json!([1, 2]))
                })?;
                Route::get("/stacks", || view("stacks"))?;
                Route::get("/empty-stack", || view("stack_layout"))?;
                Route::get("/broken-stack", || view("broken_stack"))?;
                Route::get("/fragment", || view("fragment_page").fragment("left"))?;
                Route::get("/fragments", || {
                    view("fragment_page").fragments_if(true, ["right", "missing", "left"])
                })?;
                Route::get("/full-fragments", || {
                    view("fragment_page").fragment_if(false, "left")
                })?;
                Route::get("/all-fragments", || view("fragment_page").all_fragments())?;
                Route::get("/fragment-composed", || {
                    view("fragment_composed").fragments(["right", "missing", "left", "left"])
                })?;
                Route::get("/all-fragments-composed", || {
                    view("fragment_composed").all_fragments()
                })?;
                Route::get("/empty-fragments", || {
                    view("fragment_composed").fragments(Vec::<String>::new())
                })?;
                Route::get("/missing-fragment", || {
                    view("fragment_page").fragment("missing")
                })?;
                Route::get("/broken-fragment", || {
                    view("broken_fragment").fragment("left")
                })?;
                Route::get("/stream-loop", || {
                    view("stream_loop")
                        .with_iterator("lazy", || {
                            let mut pulled = false;
                            std::iter::from_fn(move || {
                                assert!(!pulled, "early break pulled a second item");
                                pulled = true;
                                Some(("key".into(), 10.into()))
                            })
                        })
                        .with_exact_size_iterator("known", || {
                            [10, 20, 30]
                                .into_iter()
                                .enumerate()
                                .map(|(key, value)| (key.into(), value.into()))
                        })
                        .with_iterator("empty", std::iter::empty)
                })?;
                Route::get("/array-expressions", || {
                    view("array_expressions")
                        .with("indexKey", "4")
                        .with("label", "<&")
                })?;
                Route::get("/expressions", || {
                    view("expressions").with("ready", true).with("count", 2)
                })?;
                Route::get("/runtime-css", runtime_css_view)?;
                Route::get("/invalid-expression", || view("invalid_expression"))?;
                Route::get("/directive", || view("custom_directive").with("text", "<&"))?;
                Route::get("/broken-directive", || view("broken_directive"))?;
                Route::get("/includes", || {
                    view("includes")
                        .with("label", "outer")
                        .with("caller", "caller")
                        .with("values", serde_json::json!({"label":"<map>"}))
                })?;
                Route::get("/keyed-includes", || {
                    view("keyed_includes").with("caller", "outer").with(
                        "candidates",
                        serde_json::json!({
                            "include_row": "missing",
                            "z": "include_row",
                            "a": "each_empty",
                        }),
                    )
                })?;
                Route::get("/missing-keyed-includes", || {
                    view("missing_keyed_includes")
                        .with("candidates", serde_json::json!({"include_row":"missing"}))
                })?;
                Route::get("/section-defaults", || {
                    view("section_defaults").with("defaultText", "<&")
                })?;
                Route::get("/stream-each", || {
                    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
                    let runs = Arc::new(AtomicUsize::new(0));
                    let pulls = Arc::new(AtomicUsize::new(0));
                    let empty_runs = Arc::new(AtomicUsize::new(0));
                    let known_runs = Arc::new(AtomicUsize::new(0));
                    let known_pulls = Arc::new(AtomicUsize::new(0));
                    view("stream_each")
                        .with("caller", "private")
                        .with_iterator("lazy", {
                            let (runs, pulls) = (runs.clone(), pulls.clone());
                            move || {
                                let run = runs.fetch_add(1, Relaxed);
                                let pulls = pulls.clone();
                                [("b", "<&"), ("a", "V")]
                                    .into_iter()
                                    .map(move |(key, value)| {
                                        pulls.fetch_add(1, Relaxed);
                                        (
                                            key.into(),
                                            if run == 0 { "count-only" } else { value }.into(),
                                        )
                                    })
                            }
                        })
                        .with_exact_size_iterator("known", {
                            let (runs, pulls) = (known_runs.clone(), known_pulls.clone());
                            move || {
                                runs.fetch_add(1, Relaxed);
                                let pulls = pulls.clone();
                                [("k", "known")].into_iter().map(move |(key, value)| {
                                    pulls.fetch_add(1, Relaxed);
                                    (key.into(), value.into())
                                })
                            }
                        })
                        .with_iterator("emptyStream", {
                            let empty_runs = empty_runs.clone();
                            move || {
                                empty_runs.fetch_add(1, Relaxed);
                                std::iter::empty()
                            }
                        })
                        .with_exact_size_iterator("probe", move || {
                            assert_eq!((runs.load(Relaxed), pulls.load(Relaxed)), (2, 4));
                            assert_eq!(
                                (known_runs.load(Relaxed), known_pulls.load(Relaxed)),
                                (1, 1)
                            );
                            assert_eq!(empty_runs.load(Relaxed), 1);
                            std::iter::empty()
                        })
                })?;
                Route::get("/bad-includes", || view("bad_includes"))?;
                Route::get("/each", || view("each").with("caller", "private"))?;
                Route::get("/bad-each", || view("bad_each").with("items", false))?;
                Route::get("/loop-control", || view("loop_control").with("limit", 4))?;
                Route::get("/bad-loop-control", || view("bad_loop_control"))?;
                Route::get("/loop-limit", || view("loop_limit"))?;
                Route::get("/slot-attributes", || {
                    view("slot_attributes")
                        .with("title", "<Title>")
                        .with("id", "<&")
                        .with("namedSlot", "meta")
                })?;
                Route::get("/bad-component", || view("bad_component"))?;
                Route::get("/bad-attributes", || view("bad_attributes"))?;
                Route::get("/attribute-values", || view("attribute_values"))?;
                Route::get("/unknown-filter", || view("unknown_filter"))?;
                Route::get("/fonts", || view("fonts_all"))?;
                Route::get("/font-aliases", || {
                    view("font_aliases").with("alias", "mono")
                })?;
                Route::get("/unknown-font", || view("unknown_font"))?;
                Route::get("/echo-formatters", || {
                    view("echo_formatters")
                        .with_stringable("price", &Price(42))
                        .unwrap()
                        .with("items", serde_json::json!(["<one>", "two"]))
                })?;
                Route::get("/custom-condition", || view("custom_condition"))?;
                Route::get("/bad-condition", || view("bad_condition"))?;
                Route::get("/switch", || view("switch_cases"))?;
                Route::get("/bad-switch", || view("bad_switch"))?;
                Route::get("/aware", || view("aware_page").with("color", "<blue>"))?;
                Route::get("/anonymous-mounts", || {
                    view("anonymous_mounts").with("dynamic", "external::notice")
                })?;
                Route::get("/anonymous-missing", || view("anonymous_missing"))?;
                Route::get("/blade-auth/{state}", || view("blade_auth"))?;
                Route::get("/blade-auth-failed", || view("blade_auth_failed"))?;
                Route::get("/blade-gate/{state}", || {
                    view("blade_gate")
                        .with_gate_argument("subject", Price(42))
                        .with_gate_argument("target", PolicyTarget::of::<Price>())
                        .with("label", "retail")
                        .with(
                            "abilities",
                            serde_json::json!(["absent", "preview", "error"]),
                        )
                })?;
                Route::get("/bad-gate", || view("bad_gate"))?;
                Route::get("/gate-error", || view("gate_error"))?;
                Route::get("/layout-fallback", || {
                    view("layout_fallback").with("site", "parent")
                })?;
                Route::get("/bad-push", || view("bad_push"))?;
                Route::get("/view-injection", || view("view_injection"))?;
                Route::get("/encoded", || {
                    view("encoded").with("message", "<&amp; &copy; &#X27; &apos; &#x110000; &copy")
                })?;
                Route::get("/missing-view-service", || view("missing_view_service"))?;
                Route::get("/scoped-slot", || {
                    view("scoped_slot").with("component", "outside")
                })?;
                Route::get("/javascript", || {
                    let text = "</script><b>'&\"\n\u{2028}";
                    view("javascript")
                        .with("text", text)
                        .with("payload", serde_json::json!({"text": text}))
                })?;
                Route::get("/broken", || view("broken"))
            }))
            .create()
            .unwrap();
        if app == "B" {
            application.use_public_path("hot-public");
        }
        let mut kernel = Kernel::new(application);
        kernel.bootstrap().await.unwrap();
        kernel
            .get_application()
            .container()
            .instance(Arc::new(ViewSummary(app)))
            .unwrap();
        let auth = kernel
            .get_application()
            .container()
            .get::<AuthManager>("auth")
            .unwrap();
        for (driver, suffix) in [("view-default", "/signed"), ("view-alternate", "/named")] {
            auth.via_request(driver, move |request| {
                Box::pin(async move {
                    tokio::task::yield_now().await;
                    if request.parts().uri.path().ends_with(suffix) {
                        openrust::log::context::Context::add("blade.guard", driver.into()).unwrap();
                        Ok(Some(Box::new(ViewIdentity) as Box<dyn Authenticatable>))
                    } else {
                        Ok(None)
                    }
                })
            });
        }
        let gate = kernel
            .get_application()
            .container()
            .get::<GateRegistry>("gate")
            .unwrap();
        gate.policy::<Price>(PricePolicy(app == "A"));
        gate.define_guest("preview", |_, _| Box::pin(async { Ok(Decision::allow()) }));
        gate.define_guest("hidden", |_, _| {
            Box::pin(async { Ok(Decision::deny_as_not_found("hidden")) })
        });
        gate.define_guest("error", |_, _| {
            Box::pin(async { Err(AuthError::InvalidIdentity) })
        });
        gate.define_guest("typed-present", |_, args| {
            Box::pin(async move {
                Ok(Decision::from(
                    args.iter().any(|argument| argument.is::<Price>()),
                ))
            })
        });
        let factory = kernel
            .get_application()
            .container()
            .get::<Factory>(ServiceKey::typed::<Factory>())
            .unwrap();
        factory
            .register_view_service("blade.summary", |summary: &ViewSummary| {
                Ok(serde_json::json!({"label": format!("<{}>", summary.0)}).into())
            })
            .unwrap();
        let contract = kernel
            .get_application()
            .container()
            .get::<dyn openrust::contracts::view::Factory>(ServiceKey::typed::<
                dyn openrust::contracts::view::Factory,
            >())
            .unwrap();
        contract.share("contract_app", app.into());
        contract.add_namespace("demo", &[&package_views]).unwrap();
        factory
            .anonymous_component_namespace("widgets", Some("external"))
            .unwrap();
        factory
            .anonymous_component_path(&extra_components, Some("external"))
            .unwrap();
        factory
            .anonymous_component_path(&extra_components, None)
            .unwrap();
        assert!(
            factory
                .anonymous_component_namespace("../outside", Some("bad"))
                .is_err()
        );
        contract.composer(
            &["contract_card"],
            Arc::new(|view| {
                view.with("suffix", "!".into());
                Ok(())
            }),
        );
        factory.component("badge", |_| Ok(Badge));
        let renders = std::sync::atomic::AtomicUsize::new(0);
        factory.component("scoped-panel", move |_| {
            Ok(ScopedPanel {
                caption: format!("<{app}>"),
                render_id: renders.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1,
            })
        });
        factory.component("hidden-card", |_| Ok(HiddenCard));
        let openrust::view::engines::Engine::Blade(engine) =
            factory.get_engine_resolver().resolve("blade").unwrap()
        else {
            panic!("the default view compiler uses Blade");
        };
        engine
            .get_compiler()
            .r#if("appIs", move |arguments| {
                Ok(arguments.first().and_then(|value| value.as_str()) == Some(app))
            })
            .unwrap();
        engine
            .get_compiler()
            .stringable(move |price: &Price| Ok(format!("<{app}:{}>", price.0)));
        engine
            .get_compiler()
            .attribute_filter("client", |name, value| {
                name.starts_with("data-") && value.as_str().is_some_and(|text| text != "ignored")
            })
            .unwrap();
        if app == "A" {
            engine.get_compiler().stringable_iterable(|items| {
                Ok(items
                    .get(0)
                    .and_then(openrust::view::ViewValue::as_str)
                    .unwrap_or_default()
                    .to_owned())
            });
        }
        assert!(engine.get_compiler().apply_echo_handler(&0i64).is_err());
        if app == "A" {
            engine.get_compiler().without_double_encoding();
            engine
                .get_compiler()
                .directive("formatName", move |args| {
                    Ok(DirectiveOutput::Text(format!(
                        "{app}:{}",
                        args[0].as_str().unwrap()
                    )))
                })
                .unwrap();
        }
        engine
            .get_compiler()
            .directive("htmlBadge", move |_| {
                Ok(DirectiveOutput::Html(format!("<mark>{app}</mark>")))
            })
            .unwrap();
        factory.share("app", app);
        factory.share("title", "shared");
        factory.creator("page", |view| {
            view.data_mut().insert("title".into(), "creator".into());
            Ok(())
        });
        factory.creator("fragment_composed", |view| {
            view.data_mut().insert("pass".into(), 0.into());
            Ok(())
        });
        factory.composer("fragment_composed", |view| {
            let pass = view.data()["pass"].as_u64().unwrap() + 1;
            view.data_mut().insert("pass".into(), pass.into());
            ViewFacade::share("sharedFragmentPass", pass)?;
            Ok(())
        });
        factory.composer("pa*", |view| {
            assert_eq!(view.data()["title"], "route");
            assert!(ViewFacade::exists("layout")?);
            view.data_mut().insert("label".into(), "<composer>".into());
            Ok(())
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let service = transport.with_state(Arc::new(kernel)).into_make_service();
        let task = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
        (address, task)
    }

    async fn send(address: std::net::SocketAddr, path: &str, status: u16) -> String {
        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        let request =
            format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n");
        socket.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).await.unwrap();
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        assert!(
            headers.starts_with(&format!("HTTP/1.1 {status} ")),
            "{path}: {response}"
        );
        body.to_owned()
    }

    let root = TempDir::new();
    let sources = root.path().join("resources/views");
    fs::create_dir_all(sources.join("components/accordion")).unwrap();
    fs::create_dir_all(sources.join("components/ranked")).unwrap();
    fs::create_dir_all(sources.join("package/components/panel")).unwrap();
    fs::create_dir_all(sources.join("widgets")).unwrap();
    let extra = root.path().join("extra-components");
    fs::create_dir_all(extra.join("notice")).unwrap();
    fs::write(extra.join("notice/index.blade.html"), "path {{ $app }}").unwrap();
    fs::write(extra.join("standalone.blade.html"), "unprefixed").unwrap();
    for (name, source) in [
        (
            "layout",
            "<main>{{ $app }}|@yield('content', '<fallback>')</main>",
        ),
        ("middle", "@extends('layout')@section('content', 'parent')"),
        (
            "page",
            r#"@extends('middle')@section('content')@parent|<x-card :title="$label"><x-slot:header><b>{{ $title }}</b></x-slot:header>@include('partial')@includeIf('missing')@includeIf('partial')@includeWhen($label, 'partial')@includeWhen(false, 'missing')@includeUnless(true, 'missing')@includeUnless(false, 'partial')<x-badge text="&"/></x-card>@endsection"#,
        ),
        (
            "components/card",
            "<article>{{ $header }}<h2>{{ $title }}</h2>{{ $slot }}</article>",
        ),
        ("partial", "<i>{{ $label }}</i>"),
        (
            "contract_card",
            "{{ $contract_app }}:{{ $title }}{{ $suffix }}",
        ),
        ("broken", "@extends('missing')"),
        ("missing_include", "@includeWhen(true, 'missing')"),
        (
            "loops",
            concat!(
                "@foreach ($groups as $key => $items)",
                "{{ $key }}={{ $loop->index }}/{{ $loop->iteration }}/",
                "{{ $loop->remaining }}/{{ $loop->count }}",
                "@if ($loop->first)F{{ '' }}@endif{{ '' }}@if ($loop->last)L{{ '' }}@endif",
                "{{ '' }}@if ($loop->odd)O{{ '' }}@endif{{ '' }}@if ($loop->even)E{{ '' }}@endif:",
                "@include('loop_items')({{ $loop->depth }});",
                "@endforeach{{ '' }}@isset($loop)leaked{{ '' }}@endisset",
            ),
        ),
        (
            "loop_items",
            concat!(
                "@forelse ($items as $index=>$item)",
                "{{ $index }}={{ $item }}[{{ $loop->parent->iteration }}:",
                "{{ $loop->depth }}]",
                "@empty empty[{{ $loop->iteration }}]@endforelse",
            ),
        ),
        (
            "broken_loop",
            "@foreach ($items as key => $item)@endforeach",
        ),
        (
            "dynamic",
            concat!(
                r#"<x-dynamic-component :component="$componentName" :title="$label">"#,
                "<x-slot:header>{{ $app }}</x-slot:header>",
                r#"<x-dynamic-component component="badge" text="<&"/>"#,
                "</x-dynamic-component><x-ranked/>",
            ),
        ),
        (
            "components/accordion/accordion",
            "<details>{{ $header }}|{{ $title }}|{{ $slot }}@isset($component)leaked{{ '' }}@endisset</details>",
        ),
        ("components/ranked", "root"),
        ("components/ranked/index", "index"),
        ("components/ranked/ranked", "leaf"),
        (
            "props",
            concat!(
                r#"<x-alert display-name="Override" class="wide" :data-owner="$userValue" "#,
                r#"id="outer" role="log" style="color: red" disabled :hidden="false" :title="null">"#,
                r#"<x-plain :data-child="$app"/>"#,
                "</x-alert><x-alert/>",
            ),
        ),
        (
            "components/alert",
            concat!(
                "@props(['displayName' => 'Notice', 'variant',])",
                r#"<aside {{ $attributes->merge(['class' => 'panel', 'role' => 'status', 'style' => 'padding: 0;']) }}>"#,
                "{{ $displayName }}|@isset($id)leaked{{ '' }}@endisset{{ $slot }}</aside>",
            ),
        ),
        ("components/plain", "<span {{ $attributes }}></span>"),
        ("invalid_props", "@props(['not an identifier' => 'bad'])"),
        ("invalid_attributes", r#"<x-plain :data-invalid="$array"/>"#),
        (
            "stacks",
            concat!(
                "@extends('stack_layout')@push('scripts', 'A')@prepend('scripts', 'a')",
                "@include('stack_partial')@include('stack_partial')",
                "@push('scripts', 'C')@once('shared')duplicate{{ '' }}@endonce",
            ),
        ),
        (
            "stack_partial",
            concat!(
                "@push('scripts')B{{ '' }}@endpush",
                "{{ '' }}@pushOnce('scripts')D{{ '' }}@endPushOnce",
                "{{ '' }}@prependOnce('scripts', 'shared')b{{ '' }}@endPrependOnce",
                "{{ '' }}@once[once]@endonce",
            ),
        ),
        (
            "stack_layout",
            "<main>@hasStack('scripts'){{ $app }}:@endif{{ '' }}@stack('scripts', '<empty>')</main>",
        ),
        ("broken_stack", "@push('scripts')oops{{ '' }}@endprepend"),
        (
            "fragment_page",
            concat!(
                "<main>@fragment('left')<b>{{ $app }}</b>@endfragment",
                "{{ '' }}@include('fragment_partial')</main>",
            ),
        ),
        (
            "fragment_partial",
            "@fragment('right')<i>{{ $app }}</i>@endfragment",
        ),
        (
            "fragment_composed",
            concat!(
                "@fragment('left')@once L{{ $pass }}:{{ $sharedFragmentPass }}@endonce @endfragment",
                "{{ '' }}@fragment('right')R{{ $pass }}:{{ $sharedFragmentPass }}@endfragment",
            ),
        ),
        (
            "stream_loop",
            concat!(
                "@foreach ($known as $key => $item)",
                "[{{ $key }}:{{ $item }}:{{ $loop->count }}:{{ $loop->remaining }}:",
                "@bool($loop->first):@bool($loop->last)]@endforeach",
                "{{ '' }}@foreach ($lazy as $key => $item)",
                "@foreach ($known as $nested)",
                "{{ $key }}:{{ $item }}:{{ $loop->depth }}:{{ $loop->parent->iteration }}:",
                "@bool($loop->parent->first):",
                "{{ $loop->parent->count ?? 'null' }}:",
                "{{ $loop->parent->remaining ?? 'null' }}:",
                "{{ $loop->parent->last ?? 'null' }}",
                "@break(2) @endforeach @endforeach",
                "{{ '' }}@forelse ($empty as $item)bad{{ '' }}@empty|empty{{ '' }}@endforelse",
            ),
        ),
        (
            "array_expressions",
            concat!(
                "@foreach ([$indexKey => ['value' => 'old', 'value' => $label], '08' => 2, 'next', true => 9, null => 'nil',] as $key => $item)",
                "{{ $key }}={{ $item['value'] ?? $item }};@endforeach",
                "{{ ['00' => 'zero']['00'] }}|{{ array(-4 => 'first', 'second')[-3] }}|",
                "{{ [2.5 => $label][2] }}|{{ [null][4] ?? 'missing' }}|",
                "{{ [false => 'x', 'y'][1] }}|{{ [0 => 'a', 'b'][0] }}|",
                "{{ ['valid' => 'value']['missing'] ?? 'invalid-key' }}|",
                "{{ [9223372036854775807 => 'full'][0] ?? 'overflow' }}",
            ),
        ),
        ("broken_fragment", "@fragment('left')broken"),
        (
            "expressions",
            concat!(
                r#"<input @checked($ready && $count === 2) @disabled(!$ready) "#,
                r#"@class(['field', 'active<&' => $ready]) "#,
                r#"@style(['color: red' => $ready, 'display: none' => !$ready])>"#,
                "@if (false || $ready && ($count !== '2'))ok{{ '' }}@endif",
                "{{ $missing ?? $absent ?? '<fallback>' }}",
            ),
        ),
        (
            "runtime_css",
            concat!(
                r#"<div @class($classes) @style($styles)></div>"#,
                r#"<p @class('solo') @style(['margin: 0;;'])></p>"#,
                r#"<i @class @style></i><b @class(null) @style(null)></b>"#,
                r#"<x-css-sample :$classes :$styles class="incoming" style="margin: 1px" data-keep="component">"#,
                r#"<x-slot:footer class="footer" style="border: 0" data-keep="slot">slot</x-slot:footer>"#,
                r#"<x-slot:unchanged class="kept" style="width: 1em">empty</x-slot:unchanged>"#,
                "</x-css-sample>",
                r#"<x-css-tag @class($classes) @style($styles) bind:caption="$caption">"#,
                r#"<x-slot:note @class($classes) @style($styles)>tag</x-slot:note></x-css-tag>"#,
            ),
        ),
        (
            "components/css-sample",
            concat!(
                "@props(['classes','styles'])",
                r#"<aside {{ $attributes->only(['class','style','data-keep'])->class($classes)->style($styles) }}>"#,
                r#"<footer {{ $footer->attributes->whereStartsWith(['class','style','data-'])->style($styles)->class($classes) }}>{{ $footer }}</footer>"#,
                r#"<small {{ $unchanged->attributes->class(null)->style([]) }}>{{ $unchanged }}</small>"#,
                r#"<x-css-forward style="inset: 0" data-keep="child" {{ $attributes->only(['class','style','data-keep'])->class($classes)->style($styles) }}/>"#,
                "</aside>",
            ),
        ),
        (
            "components/css-tag",
            concat!(
                "@props(['caption'])",
                r#"<section {{ $attributes }}>{{ $caption }}<label {{ $note->attributes }}>{{ $note }}</label></section>"#,
            ),
        ),
        ("components/css-forward", "<code {{ $attributes }}></code>"),
        ("invalid_expression", "@if ($x ===)broken{{ '' }}@endif"),
        ("custom_directive", "<b>@formatName($text)</b>@htmlBadge"),
        ("broken_directive", "@formatName($text + 1)"),
        (
            "includes",
            concat!(
                "@includeFirst(['absent', 'include_row'], ['label' => '<&'])",
                "@includeWhen(true, 'include_row', $values)",
                "@includeIsolated('include_row', ['label' => 'isolated'])",
                "@includeIf('absent', true)",
            ),
        ),
        (
            "include_row",
            "[{{ $app }}:{{ $label }}:{{ $caller ?? '' }}]",
        ),
        (
            "keyed_includes",
            concat!(
                "@includeFirst(['include_row' => 'missing', 'z' => 'include_row', 'a' => 'each_empty'], ['label' => 'literal'])",
                "@includeFirst($candidates, ['label' => 'runtime'])",
            ),
        ),
        ("missing_keyed_includes", "@includeFirst($candidates)"),
        (
            "section_defaults",
            concat!(
                "@section('present', '0')@section('blank', ' ')@section('real', 'content')",
                "@hasSection('missing', $defaultText)A{{ '' }}@endif",
                "{{ '' }}@sectionMissing('absent', ' 0 ')B{{ '' }}@endif",
                "{{ '' }}@sectionMissing('present', 'fallback')C{{ '' }}@endif",
                "{{ '' }}@sectionMissing('blank', 'fallback')D{{ '' }}@endif",
                "{{ '' }}@hasSection('real', '')E{{ '' }}@endif",
                r#"{{ '' }}@hasSection('absent', "\u00a0")F{{ '' }}@endif"#,
                "{{ '' }}@sectionMissing('absent', false)G{{ '' }}@endif",
                "{{ '' }}@hasSection('absent', true)H{{ '' }}@endif",
                "{{ '' }}@yield('unknown', $defaultText)",
            ),
        ),
        (
            "stream_each",
            concat!(
                "@each('each_row', $lazy, 'item')",
                "@each('each_row', $known, 'item')",
                "@each('each_row', $emptyStream, 'item', 'each_empty')",
                "@each('each_row', $probe, 'item', 'raw|<ok>')",
            ),
        ),
        ("bad_includes", "@includeFirst(['no_such_view'])"),
        (
            "each",
            concat!(
                r#"@each('each_row', ["<&"], 'item')"#,
                r#"@each('each_row', {"name":"map"}, 'item')"#,
                "@each('missing', [], 'item')",
                "@each('missing', [], 'item', 'each_empty')",
                "@each('missing', [], 'item', 'raw|<em>empty</em>')",
            ),
        ),
        (
            "each_row",
            "[{{ $app }}:{{ $key }}:{{ $item }}:{{ $caller ?? '' }}]",
        ),
        ("each_empty", "{{ $app }}:{{ $caller ?? '' }}"),
        ("bad_each", "@each('each_row', $items, 'item')"),
        (
            "loop_control",
            concat!(
                "@for ($i = 0; $i < $limit; ++$i)",
                "@continue($i === 1){{ $i }}@break($i === 2)@endfor|",
                "@for ($i = 3; $i >= 1; $i -= 2){{ $i }}@endfor|",
                "@foreach ([1, 2] as $item){{ $item }}",
                "@while(true)@continue(2)@endwhile unreachable",
                "{{ '' }}@endforeach|@while(true)once{{ '' }}@break @endwhile|",
                "@forelse ([] as $item)bad{{ '' }}@empty empty{{ '' }}@endforelse",
            ),
        ),
        (
            "bad_loop_control",
            "@foreach ([1] as $item)@break(2)@endforeach",
        ),
        ("loop_limit", "@while(true)@endwhile"),
        (
            "slot_attributes",
            concat!(
                r#"<x-demo::panel :$title ::class="active" role="status">"#,
                r#"<x-slot:footer-actions :data-id="$id" class="extra" title="remove"> action </x-slot>"#,
                r#"<x-slot :name="$namedSlot" data-note="note"> meta </x-slot>"#,
                " content </x-demo::panel>",
            ),
        ),
        (
            "package/components/panel/index",
            concat!(
                "@props(['title'])",
                r#"<main {{ $attributes->whereDoesntStartWith(['role', 'aria-'])->only(':class') }}>{{ $app }}:{{ $title }}"#,
                r#"<footer {{ $footerActions->attributes->whereStartsWith(['class', 'data-'])->class(['base'])->merge(['aria-label' => 'a->b']) }}>{{ $footerActions }}</footer>"#,
                r#"<i {{ $meta->attributes->thatStartWith('data-')->except('class') }}>{{ $meta }}</i>"#,
                "{{ $slot }}</main>",
            ),
        ),
        ("bad_component", "<x-demo::../panel/>"),
        ("bad_attributes", "{{ $attributes->only('id')->unknown() }}"),
        (
            "attribute_values",
            concat!(
                r#"<x-inspect data-controller="child" data-empty="" data-id="<x>" data-skip="ignored" id="absent">"#,
                "<x-slot:title><!-- only a comment --></x-slot:title>",
                "<x-slot:footer><br></x-slot:footer>",
                "</x-inspect>",
                "<x-hidden-card>@include('should-never-load')</x-hidden-card>",
            ),
        ),
        (
            "components/inspect",
            concat!(
                r#"<div {{ $attributes->filter('client')->merge(['data-controller' => $attributes->prepends('base'), 'data-empty' => $attributes->prepends('fallback')]) }}>"#,
                "@if($attributes->has(['data-id', 'data-controller']))all{{ '' }}@endif",
                "{{ '' }}@if($attributes->hasAny(['href','data-id']))any{{ '' }}@endif",
                "|{{ $attributes->whereStartsWith('data-id')->first() }}|{{ $attributes->get('missing', 'default') }}",
                "@if($slot->isEmpty())|empty{{ '' }}@endif{{ '' }}@if($title->isNotEmpty())|comment{{ '' }}@endif",
                "{{ '' }}@if($title->hasActualContent())|unexpected{{ '' }}@elseif($footer->hasActualContent())|markup{{ '' }}@endif",
                "</div>",
            ),
        ),
        ("unknown_filter", r#"<x-filter-probe id="visible"/>"#),
        (
            "components/filter-probe",
            "{{ $attributes->filter('unknown') }}",
        ),
        ("fonts_all", "<head>@fonts</head>"),
        (
            "font_aliases",
            "@fonts($alias)|{{ Vite::fonts(['mono', 'sans']) }}",
        ),
        ("unknown_font", "@fonts('missing')"),
        (
            "echo_formatters",
            "{{ $price }}|{!! $price !!}|{{ $items }}|{!! $items !!}",
        ),
        (
            "custom_condition",
            concat!(
                "@appIs('A')A{{ '' }}@elseappIs('B')B{{ '' }}@else other{{ '' }}@endappIs",
                "{{ '' }}@unlessappIs('X')!@appIs('A')1{{ '' }}@else 2{{ '' }}@endappIs{{ '' }}@endappIs",
            ),
        ),
        ("bad_condition", "@appIs('A')missing end"),
        (
            "switch_cases",
            concat!(
                "@extends('switch_layout')@section('banner', 'Ready')",
                "@switch('2')@default wrong{{ '' }}@break @case(2)two{{ '' }}@case(3)three{{ '' }}@break @endswitch|",
                "@for($i = 0; $i < 2; $i++)",
                "@switch($i)@case(0)@continue(2)@default good{{ '' }}@endswitch{{ $i }}@endfor|",
                "@bool(true)@bool(false)|",
            ),
        ),
        (
            "switch_layout",
            concat!(
                "@hasSection('banner')@yield('banner')@endif",
                "{{ '' }}@sectionMissing('sidebar')/no sidebar{{ '' }}@endif",
            ),
        ),
        (
            "bad_switch",
            "@switch(0)@default x{{ '' }}@default y{{ '' }}@endswitch",
        ),
        (
            "anonymous_mounts",
            r#"<x-external::banner/>|<x-dynamic-component :component="$dynamic"/>|<x-standalone/>"#,
        ),
        ("widgets/banner", "namespace"),
        ("anonymous_missing", "<x-external::none/>"),
        (
            "blade_auth",
            concat!(
                "@auth default{{ '' }}@elseauth('alternate') named{{ '' }}@elseguest guest{{ '' }}@endauth",
                "{{ '' }}@guest('alternate')|no-alt{{ '' }}@endguest",
                "{{ '' }}@if(false)@auth('unknown')unreachable{{ '' }}@endauth{{ '' }}@endif",
                "{{ '' }}@context('blade.guard')|{{ $value }}@endcontext",
            ),
        ),
        (
            "blade_auth_failed",
            "@auth('unknown')unreachable{{ '' }}@endauth",
        ),
        (
            "blade_gate",
            concat!(
                "@can('view-price', [$subject, $label])policy{{ '' }}@elsecanany(['absent','preview'])preview{{ '' }}@endcan|",
                "@cannot(['preview','hidden'])denied{{ '' }}@endcannot|",
                "@canany($abilities)any{{ '' }}@elsecan('error')unexpected{{ '' }}@endcanany|",
                "@can('hidden')unexpected{{ '' }}@elsecan('preview')elsecan{{ '' }}@elsecannot('hidden')unexpected{{ '' }}@endcan|",
                "@canany([])unexpected{{ '' }}@elsecannot('hidden')elsecannot{{ '' }}@endcanany|",
                "@can('create-price', $target)create{{ '' }}@else guest{{ '' }}@endcan|",
                "@can('preview')@cannot('hidden')nested{{ '' }}@endcannot{{ '' }}@endcan|",
                "@include('gate_partial'):@includeIsolated('gate_partial', ['subject' => null])",
            ),
        ),
        (
            "gate_partial",
            "@can('typed-present', $subject)typed{{ '' }}@else clear{{ '' }}@endcan",
        ),
        ("bad_gate", "@can('preview')mismatched{{ '' }}@endcannot"),
        (
            "gate_error",
            "@cannot('error')unexpected{{ '' }}@else unexpected{{ '' }}@endcannot",
        ),
        (
            "layout_fallback",
            concat!(
                "@extendsFirst(['absent', 'layout_variants'], ['site' => 'override'])",
                "@section('banner')first{{ '' }}@stop",
                "{{ '' }}@section('banner')second{{ '' }}@append",
                "{{ '' }}@section('footer')old{{ '' }}@endsection",
                "{{ '' }}@section('footer')new{{ '' }}@overwrite",
                "{{ '' }}@pushIf(false, 'scripts')wrong{{ '' }}@elsePushIf('2' == 2 && 10 != '11', 'scripts')chosen{{ '' }}@endPushIf",
            ),
        ),
        (
            "layout_variants",
            "{{ $site }}:@yield('banner'):@yield('footer'):@stack('scripts')",
        ),
        ("bad_push", "@pushIf(true, 'scripts')missing end"),
        (
            "view_injection",
            "@inject('summary', 'blade.summary')<b>{{ $summary->label }}</b>",
        ),
        ("missing_view_service", "@inject('summary', 'missing')"),
        ("encoded", "{{ $message }}"),
        (
            "scoped_slot",
            concat!(
                "<x-scoped-panel><x-slot:heading>{{ $component->caption }}</x-slot:heading>",
                "{{ $component->renderId }}</x-scoped-panel>|{{ $component }}",
            ),
        ),
        (
            "javascript",
            concat!(
                "<script>const text = {{ Js::from($text) }};",
                "const payload = @js($payload);",
                "const empty = {{ Js::from([]) }};",
                "const list = {{ Js::from([\"a\",\"b\"]) }};</script>",
            ),
        ),
        (
            "aware_page",
            concat!(
                "<x-frame :$color><x-tone/><x-tone color=\"near\"/>",
                "<x-slot:details><x-tone/></x-slot></x-frame><x-frame><x-slot:details></x-slot></x-frame><x-tone/>",
            ),
        ),
        (
            "components/frame",
            "@props(['color' => 'default'])<b>{{ $color }}</b>{{ $slot }}{{ $details }}<x-tone/>",
        ),
        (
            "components/tone",
            "{{-- component preamble --}}@props(['shape' => 'circle'])@aware(['color' => 'fallback'])<i>{{ $color }}:{{ $shape }}</i>",
        ),
    ] {
        fs::write(sources.join(format!("{name}.blade.html")), source).unwrap();
    }
    let ((first, first_task), (second, second_task)) = tokio::join!(
        start(root.path().to_owned(), "A"),
        start(root.path().to_owned(), "B"),
    );
    assert_eq!(send(first, "/fonts", 200).await, "<head></head>");
    let build = root.path().join("public/build");
    fs::create_dir_all(&build).unwrap();
    fs::write(build.join("all.css"), "/* all fonts */").unwrap();
    fs::write(
        build.join("fonts-manifest.json"),
        serde_json::json!({
            "version": 1,
            "families": {"mono": {}, "sans": {}},
            "preloads": [
                {"alias": "mono", "file": "assets/mono.woff2", "type": "font/woff2", "crossorigin": "anonymous"},
                {"alias": "sans", "file": "sans.woff2"}
            ],
            "style": {
                "file": "all.css",
                "familyStyles": {"mono": "/* mono */", "sans": "/* sans */"},
                "variables": {"mono": "--mono: Mono;", "sans": "--sans: Sans;"}
            }
        }).to_string(),
    ).unwrap();
    let hot = root.path().join("hot-public");
    fs::create_dir_all(&hot).unwrap();
    fs::write(hot.join("hot"), "http://127.0.0.1:5173").unwrap();
    fs::write(
        hot.join("fonts-manifest.dev.json"),
        r#"{"version":1,"families":{"mono":{}},"preloads":[{"alias":"mono","url":"http://127.0.0.1:5173/mono.woff2?v=1&x=2"}],"style":{"inline":"/* hot */"}}"#,
    ).unwrap();
    let (fonts, development) =
        tokio::join!(send(first, "/fonts", 200), send(second, "/fonts", 200));
    assert!(fonts.contains(&format!(r#"href="http://{first}/build/assets/mono.woff2" type="font/woff2" crossorigin="anonymous""#)), "{fonts}");
    assert!(
        fonts.contains(&format!(r#"href="http://{first}/build/sans.woff2""#)),
        "{fonts}"
    );
    assert!(
        fonts.ends_with("<style>\n/* all fonts */\n</style></head>"),
        "{fonts}"
    );
    assert_eq!(
        development,
        "<head><link rel=\"preload\" as=\"font\" href=\"http://127.0.0.1:5173/mono.woff2?v=1&x=2\" />\n<style>\n/* hot */\n</style></head>"
    );
    let aliases = send(first, "/font-aliases", 200).await;
    let (mono, both) = aliases.split_once('|').unwrap();
    assert!(
        mono.contains("mono.woff2") && mono.contains("--mono: Mono;") && !mono.contains("sans")
    );
    assert!(
        both.contains("sans.woff2")
            && both.contains("--sans: Sans;")
            && !both.contains("mono.woff2")
    );
    assert!(send(first, "/unknown-font", 500).await.contains("500"));
    fs::write(build.join("all.css"), "</STYLE><img>").unwrap();
    assert!(send(first, "/fonts", 500).await.contains("500"));
    assert_eq!(send(second, "/fonts", 200).await, development);
    let (a, b) = tokio::join!(
        send(first, "/echo-formatters", 200),
        send(second, "/echo-formatters", 500),
    );
    assert_eq!(a, "&lt;A:42&gt;|<A:42>|&lt;one&gt;|<one>");
    assert!(b.contains("500"));
    let (a, b, unrelated) = tokio::join!(
        send(first, "/page", 200),
        send(second, "/page", 200),
        send(first, "/plain", 200),
    );
    assert_eq!(
        a,
        "<main>A|parent|<article><b>route</b><h2>&lt;composer&gt;</h2><i>&lt;composer&gt;</i><i>&lt;composer&gt;</i><i>&lt;composer&gt;</i><i>&lt;composer&gt;</i><mark>&amp;</mark></article></main>"
    );
    assert_eq!(
        b,
        "<main>B|parent|<article><b>route</b><h2>&lt;composer&gt;</h2><i>&lt;composer&gt;</i><i>&lt;composer&gt;</i><i>&lt;composer&gt;</i><i>&lt;composer&gt;</i><mark>&amp;</mark></article></main>"
    );
    assert_eq!(unrelated, "<main>A|&lt;fallback&gt;</main>");
    assert!(send(first, "/broken", 500).await.contains("500"));
    assert!(send(first, "/missing-include", 500).await.contains("500"));
    assert!(send(first, "/broken-loop", 500).await.contains("500"));
    let (a, b) = tokio::join!(send(first, "/loops", 200), send(second, "/loops", 200));
    assert_eq!(
        a,
        "&lt;first&gt;=0/1/1/2FO:0=&lt;&amp;[1:2]1=two[1:2](1);second=1/2/0/2LE:empty[2](1);"
    );
    assert_eq!(a, b);
    let (a, b) = tokio::join!(send(first, "/dynamic", 200), send(second, "/dynamic", 200));
    assert_eq!(
        a,
        "<details>A|&lt;title&gt;|<mark>&lt;&amp;</mark></details>root"
    );
    assert_eq!(
        b,
        "<details>B|&lt;title&gt;|<mark>&lt;&amp;</mark></details>root"
    );
    assert!(send(first, "/dynamic-invalid", 500).await.contains("500"));
    assert!(send(first, "/dynamic-missing", 500).await.contains("500"));
    fs::remove_file(sources.join("components/ranked.blade.html")).unwrap();
    assert!(
        send(first, "/dynamic", 200)
            .await
            .ends_with("</details>index")
    );
    let (a, b) = tokio::join!(send(first, "/props", 200), send(second, "/props", 200));
    for (html, app) in [(a, "A"), (b, "B")] {
        assert_eq!(
            html,
            format!(
                concat!(
                    r#"<aside class="panel wide" role="log" style="padding: 0; color: red;" data-owner="&quot;&lt;&amp;" id="outer" disabled="disabled">"#,
                    r#"Override|<span data-child="{}"></span></aside>"#,
                    r#"<aside class="panel" role="status" style="padding: 0;">Notice|</aside>"#,
                ),
                app
            )
        );
    }
    assert!(send(first, "/invalid-props", 500).await.contains("500"));
    assert!(
        send(first, "/invalid-attributes", 500)
            .await
            .contains("500")
    );
    assert!(send(first, "/broken-stack", 500).await.contains("500"));
    let (a, b) = tokio::join!(send(first, "/stacks", 200), send(second, "/stacks", 200));
    assert_eq!(a, "[once]<main>A:baACBDB</main>");
    assert_eq!(b, "[once]<main>B:baACBDB</main>");
    assert_eq!(
        send(first, "/empty-stack", 200).await,
        "<main><empty></main>"
    );
    let (a, b) = tokio::join!(
        send(first, "/fragments", 200),
        send(second, "/fragments", 200),
    );
    assert_eq!(a, "<i>A</i><main><b>A</b><i>A</i></main><b>A</b>");
    assert_eq!(b, "<i>B</i><main><b>B</b><i>B</i></main><b>B</b>");
    assert_eq!(send(first, "/fragment", 200).await, "<b>A</b>");
    assert_eq!(send(first, "/all-fragments", 200).await, "<b>A</b><i>A</i>");
    assert_eq!(
        send(first, "/fragment-composed", 200).await,
        "R1:1L2:2 R2:2 L3:3  L4:4 "
    );
    assert_eq!(
        send(first, "/all-fragments-composed", 200).await,
        " L1:1 R1:1"
    );
    assert_eq!(send(first, "/empty-fragments", 200).await, "");
    assert_eq!(
        send(first, "/missing-fragment", 200).await,
        "<main><b>A</b><i>A</i></main>"
    );
    assert_eq!(
        send(first, "/full-fragments", 200).await,
        "<main><b>A</b><i>A</i></main>"
    );
    assert_eq!(
        send(first, "/stream-loop", 200).await,
        "[0:10:3:2:true:false][1:20:3:1:false:false][2:30:3:0:false:true]key:10:2:1:true:null:null:null|empty"
    );
    assert!(send(first, "/broken-fragment", 500).await.contains("500"));
    assert_eq!(
        send(first, "/array-expressions", 200).await,
        "4=&lt;&amp;;08=2;5=next;1=9;=nil;zero|second|&lt;&amp;|missing|y|a|invalid-key|overflow"
    );
    assert_eq!(
        send(first, "/expressions", 200).await,
        r#"<input checked  class="field active&lt;&amp;" style="color: red;">ok&lt;fallback&gt;"#
    );
    let css = send(first, "/runtime-css", 200).await;
    for expected in [
        r#"<div class="base active 9 safe&lt;&amp;" style="display: block; color: red;"></div>"#,
        r#"<p class="solo" style="margin: 0;"></p>"#,
        r#"<i class="" style=""></i><b class="" style=""></b>"#,
        r#"<aside style="display: block; color: red; margin: 1px;" class="base active 9 safe&lt;&amp; incoming" data-keep="component">"#,
        r#"<footer class="base active 9 safe&lt;&amp; footer" style="display: block; color: red; border: 0;" data-keep="slot">slot</footer>"#,
        r#"<small style="width: 1em;" class="kept">empty</small>"#,
        r#"<section class="base active 9 safe&lt;&amp;" style="display: block; color: red;">&lt;tag&gt;<label class="base active 9 safe&lt;&amp;" style="display: block; color: red;">tag</label></section>"#,
        r#"<code style="inset: 0; display: block; color: red; margin: 1px;" data-keep="component" class="base active 9 safe&lt;&amp; incoming"></code>"#,
    ] {
        assert!(css.contains(expected), "{css}");
    }
    assert!(
        send(first, "/invalid-expression", 500)
            .await
            .contains("500")
    );
    let (a, b) = tokio::join!(
        send(first, "/directive", 200),
        send(second, "/directive", 200)
    );
    assert_eq!(a, "<b>A:&lt;&amp;</b><mark>A</mark>");
    assert_eq!(b, "<b>@formatName($text)</b><mark>B</mark>");
    assert!(send(first, "/broken-directive", 500).await.contains("500"));
    assert_eq!(
        send(first, "/includes", 200).await,
        "[A:&lt;&amp;:caller][A:&lt;map&gt;:caller][A:isolated:]"
    );
    assert_eq!(
        send(second, "/includes", 200).await,
        "[B:&lt;&amp;:caller][B:&lt;map&gt;:caller][B:isolated:]"
    );
    assert_eq!(
        send(first, "/keyed-includes", 200).await,
        "[A:literal:outer][A:runtime:outer]"
    );
    assert!(
        send(first, "/missing-keyed-includes", 500)
            .await
            .contains("500")
    );
    assert_eq!(
        send(first, "/section-defaults", 200).await,
        "ABCDEFGH&lt;&amp;"
    );
    let (a, b) = tokio::join!(
        send(first, "/stream-each", 200),
        send(second, "/stream-each", 200)
    );
    assert_eq!(a, "[A:b:&lt;&amp;:][A:a:V:][A:k:known:]A:<ok>");
    assert_eq!(b, "[B:b:&lt;&amp;:][B:a:V:][B:k:known:]B:<ok>");
    assert!(send(first, "/bad-includes", 500).await.contains("500"));
    let (a, b) = tokio::join!(send(first, "/each", 200), send(second, "/each", 200));
    assert_eq!(a, "[A:0:&lt;&amp;:][A:name:map:]A:<em>empty</em>");
    assert_eq!(b, "[B:0:&lt;&amp;:][B:name:map:]B:<em>empty</em>");
    assert!(send(first, "/bad-each", 500).await.contains("500"));
    assert_eq!(
        send(first, "/loop-control", 200).await,
        "02|31|12|once| empty"
    );
    assert!(send(first, "/bad-loop-control", 500).await.contains("500"));
    assert!(send(first, "/loop-limit", 500).await.contains("500"));
    let (a, b) = tokio::join!(
        send(first, "/slot-attributes", 200),
        send(second, "/slot-attributes", 200),
    );
    for (html, label) in [(a, "A"), (b, "B")] {
        assert_eq!(
            html,
            format!(
                r#"<main :class="active">{label}:&lt;Title&gt;<footer aria-label="a-&gt;b" class="base extra" data-id="&lt;&amp;">action</footer><i data-note="note">meta</i>content</main>"#
            )
        );
    }
    assert!(send(first, "/bad-component", 500).await.contains("500"));
    assert!(send(first, "/bad-attributes", 500).await.contains("500"));
    let (a, b) = tokio::join!(
        send(first, "/attribute-values", 200),
        send(second, "/attribute-values", 200)
    );
    for html in [a, b] {
        assert_eq!(
            html,
            r#"<div data-controller="base child" data-empty="fallback" data-id="&lt;x&gt;">allany|&lt;x&gt;|default|empty|comment|markup</div>"#
        );
    }
    assert!(send(first, "/unknown-filter", 500).await.contains("500"));
    let (a, b) = tokio::join!(
        send(first, "/custom-condition", 200),
        send(second, "/custom-condition", 200),
    );
    assert_eq!(a, "A!1");
    assert_eq!(b, "B! 2");
    assert!(send(first, "/bad-condition", 500).await.contains("500"));
    assert_eq!(
        send(first, "/switch", 200).await,
        "twothree| good1|truefalse|Ready/no sidebar"
    );
    assert!(send(first, "/bad-switch", 500).await.contains("500"));
    let (a, b) = tokio::join!(
        send(first, "/anonymous-mounts", 200),
        send(second, "/anonymous-mounts", 200)
    );
    assert_eq!(a, "namespace|path A|unprefixed");
    assert_eq!(b, "namespace|path B|unprefixed");
    assert!(send(first, "/anonymous-missing", 500).await.contains("500"));
    assert_eq!(
        send(first, "/blade-auth/signed", 200).await,
        "default|no-alt|view-default"
    );
    assert_eq!(
        send(first, "/blade-auth/named", 200).await,
        "named|view-alternate"
    );
    assert_eq!(send(second, "/blade-auth/off", 200).await, "guest|no-alt");
    assert!(send(first, "/blade-auth-failed", 500).await.contains("500"));
    let (a, b, visitor) = tokio::join!(
        send(first, "/blade-gate/signed", 200),
        send(second, "/blade-gate/signed", 200),
        send(first, "/blade-gate/off", 200),
    );
    let authenticated_tail = "|denied|any|elsecan|elsecannot|create|nested|typed:clear";
    assert_eq!(a, format!("policy{authenticated_tail}"));
    assert_eq!(b, format!("preview{authenticated_tail}"));
    assert_eq!(
        visitor,
        "preview|denied|any|elsecan|elsecannot| guest|nested|typed:clear"
    );
    assert!(send(first, "/bad-gate", 500).await.contains("500"));
    assert!(send(first, "/gate-error", 500).await.contains("500"));
    assert_eq!(
        send(first, "/layout-fallback", 200).await,
        "override:firstsecond:new:chosen"
    );
    assert!(send(first, "/bad-push", 500).await.contains("500"));
    let (a, b) = tokio::join!(
        send(first, "/view-injection", 200),
        send(second, "/view-injection", 200)
    );
    assert_eq!(a, "<b>&lt;A&gt;</b>");
    assert_eq!(b, "<b>&lt;B&gt;</b>");
    let (a, b) = tokio::join!(send(first, "/encoded", 200), send(second, "/encoded", 200));
    assert_eq!(
        a,
        "&lt;&amp; &copy; &#X27; &amp;apos; &amp;#x110000; &amp;copy"
    );
    assert_eq!(
        b,
        "&lt;&amp;amp; &amp;copy; &amp;#X27; &amp;apos; &amp;#x110000; &amp;copy"
    );
    assert!(
        send(first, "/missing-view-service", 500)
            .await
            .contains("500")
    );
    let (a, b) = tokio::join!(
        send(first, "/scoped-slot", 200),
        send(second, "/scoped-slot", 200)
    );
    assert_eq!(a, "<article>&lt;A&gt;#1:&lt;A&gt;:1</article>|outside");
    assert_eq!(b, "<article>&lt;B&gt;#1:&lt;B&gt;:1</article>|outside");
    assert_eq!(
        send(first, "/scoped-slot", 200).await,
        "<article>&lt;A&gt;#2:&lt;A&gt;:2</article>|outside"
    );
    let html = send(first, "/javascript", 200).await;
    assert_eq!(html.matches("</script>").count(), 1);
    assert!(html.contains(
        r#"const text = '\u003C\/script\u003E\u003Cb\u003E\u0027\u0026\u0022\n\u2028';"#
    ));
    assert!(html.contains(r"const payload = JSON.parse('{\u0022text\u0022:\u0022\\u003C"));
    assert!(
        html.contains(
            r"const empty = [];const list = JSON.parse('[\u0022a\u0022,\u0022b\u0022]');"
        )
    );
    assert_eq!(
        send(first, "/aware", 200).await,
        concat!(
            "<b>&lt;blue&gt;</b><i>&lt;blue&gt;:circle</i><i>near:circle</i>",
            "<i>&lt;blue&gt;:circle</i><i>&lt;blue&gt;:circle</i>",
            "<b>default</b><i>fallback:circle</i><i>fallback:circle</i>",
        )
    );
    assert_eq!(send(first, "/plain", 200).await, unrelated);
    let (a, b) = tokio::join!(
        send(first, "/contract", 200),
        send(second, "/contract", 200)
    );
    assert_eq!(a, "A:&lt;card&gt;!");
    assert_eq!(b, "B:&lt;card&gt;!");
    first_task.abort();
    second_task.abort();
}
