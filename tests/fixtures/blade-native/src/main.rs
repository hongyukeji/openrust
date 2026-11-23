use std::{error::Error, process::ExitCode};

use openrust::{
    contracts::{foundation::Application as _, view::Factory as _},
    foundation::{Application, configuration::Routing, run_application},
    http::{HttpError, HttpFuture, Request, Response},
    support::facades::Route,
    view::{Factory, ViewData, view},
};

mod compiled {
    include!(concat!(env!("OUT_DIR"), "/packages.rs"));
}
fn suffix() -> &'static str {
    "native"
}

fn application() -> Result<Application, Box<dyn Error + Send + Sync>> {
    let root = Application::infer_base_path(
        (!cfg!(feature = "standalone")).then_some(env!("CARGO_MANIFEST_DIR")),
    )?;
    Ok(Application::configure(root)
        .with_config(|_, env| {
            Ok(std::collections::BTreeMap::from([(
                "app".into(),
                serde_json::json!({
                    "env":"testing","debug":true,"locale":"en","fallback_locale":"en",
                    "locale_query_parameter":null,"host":"127.0.0.1",
                    "port":env.get_utf8("APP_PORT")?.unwrap_or("8000").parse::<u16>().unwrap()
                }),
            )]))
        })
        .with_packages(compiled::PACKAGES)
        .with_routing(Routing::default().web(|| Route::get("/native", render)))
        .create()?)
}

fn render(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let name = request
            .headers()
            .get("x-name")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("anonymous");
        let result = view("http").with("name", name.to_owned());
        Response::view(
            openrust::http::StatusCode::OK,
            result,
            Default::default(),
            request,
        )
        .await
    })
}

struct RenderJob;
impl openrust::contracts::queue::JobHandler for RenderJob {
    fn handle<'a>(
        &'a self,
        _: &'a mut openrust::queue::JobContext<'_>,
        payload: &'a openrust::queue::Payload,
    ) -> openrust::queue::QueueFuture<'a, openrust::queue::QueueResult<()>> {
        Box::pin(async move {
            use openrust::support::facades::Facade;
            let n = payload.data()["number"].as_i64().unwrap();
            let factory =
                openrust::support::facades::View::get_facade_root().map_err(|source| {
                    openrust::queue::QueueError::Execution {
                        source: Box::new(source),
                    }
                })?;
            let data = ViewData::from_iter([("numbers".into(), vec![n].into())]);
            let rendered = factory.render("proof", &data).map_err(|source| {
                openrust::queue::QueueError::Execution {
                    source: Box::new(source),
                }
            })?;
            assert_eq!(rendered, format!("&lt;native:{n}&gt;|<native:{n}>"));
            assert!(data.get("result").is_none());
            Ok(())
        })
    }
}

async fn self_test() -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut app = application()?;
    app.boot().await?;
    let factory = app.make::<Factory>().await?;
    factory.add_namespace("extra", &[&app.base_path().join("templates")])?;
    assert_eq!(
        factory.render("extra::extra", &ViewData::new())?,
        "custom-directory"
    );
    assert_eq!(
        factory.render(
            "documentation",
            &ViewData::from_iter([("name".into(), "Alice".into())])
        )?,
        "&lt;ALICE&gt;"
    );
    assert_eq!(
        factory.render("loops", &ViewData::new())?,
        "0;3;6;|outer|while|outer|1"
    );
    let data = ViewData::from_iter([("numbers".into(), vec![1, 2, 2, 3].into())]);
    assert_eq!(
        factory.render("proof", &data)?,
        "&lt;native:6&gt;|<native:6>"
    );
    assert!(data.get("result").is_none());
    assert_eq!(
        factory.render(
            "scopes",
            &ViewData::from_iter([("numbers".into(), vec![1, 2].into())])
        )?,
        "branch|outer|0:2;1:4;|outer|include-local|outer|outer-component|outer"
    );
    assert_eq!(
        factory.render("child", &ViewData::new())?,
        "&lt;child&gt;-layout|child-body"
    );
    assert_eq!(factory.render("branch", &ViewData::new())?, "12");
    assert_eq!(
        factory.render(
            "values",
            &ViewData::from_iter([
                ("null".into(), openrust::view::ViewValue::Null),
                ("number".into(), 1.5.into()),
                ("flag".into(), true.into())
            ])
        )?,
        "None:1.5:true"
    );
    assert_eq!(
        factory.render("native::index", &ViewData::new())?,
        "package"
    );
    assert_eq!(
        factory.render("native::overridden", &ViewData::new())?,
        if std::env::var_os("NATIVE_PACKAGE_DEFAULT").is_some() {
            "package-default"
        } else {
            "published-override"
        }
    );
    assert!(factory.render("error", &ViewData::new()).is_err());
    assert!(
        factory
            .render(
                "error",
                &ViewData::from_iter([("required".into(), 7.into())])
            )
            .is_err()
    );
    let error = factory.render("failure", &ViewData::new()).unwrap_err();
    assert!(
        std::error::Error::source(&error)
            .unwrap()
            .to_string()
            .contains("明确业务失败")
    );
    assert!(factory.render("local", &ViewData::new()).is_err());
    assert!(
        factory
            .render("tokens", &ViewData::new())?
            .contains("@rust verbatim @endrust|@rust|@endrust {{-- preserved --}} @verbatim")
    );
    assert!(
        factory
            .render_inline("@rust context.set(\"new\",123); @endrust", &ViewData::new())
            .is_err()
    );
    let threads: Vec<_> = (1..=12)
        .map(|n| {
            let f = factory.clone();
            std::thread::spawn(move || {
                let data = ViewData::from_iter([("numbers".into(), vec![n].into())]);
                assert_eq!(
                    f.render("proof", &data).unwrap(),
                    format!("&lt;native:{n}&gt;|<native:{n}>")
                );
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    // 通过真实同步队列作用域重复执行任务，验证 Facade 与原生数据不会跨任务串值。
    app.config().unwrap().set(
        "queue.connections.sync",
        serde_json::json!({"driver":"sync"}),
    )?;
    let queue = app.make::<openrust::queue::QueueManager>().await?;
    queue.register("native.render", std::sync::Arc::new(RenderJob))?;
    for n in [101, 202, 303] {
        queue
            .dispatch(
                openrust::queue::Payload::new("native.render", serde_json::json!({"number":n}))?,
                openrust::queue::DispatchOptions {
                    connection: Some("sync".into()),
                    ..Default::default()
                },
                None,
            )
            .await?;
    }
    let mut second = application()?;
    second.boot().await?;
    let other = second.make::<Factory>().await?;
    assert_eq!(
        other.render(
            "proof",
            &ViewData::from_iter([("numbers".into(), vec![99].into())])
        )?,
        "&lt;native:99&gt;|<native:99>"
    );
    println!(
        "PASS native build/render, strict data/errors, scopes, packages, escaping, concurrent, queue job and application isolation"
    );
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("self-test") {
        match self_test().await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e:?}");
                ExitCode::FAILURE
            }
        }
    } else {
        run_application(application).await
    }
}
