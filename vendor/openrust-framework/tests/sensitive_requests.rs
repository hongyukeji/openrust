use openrust::{
    contracts::http::{Kernel as _, Middleware},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{
        Body, HttpError, HttpFuture, Next, Request, Response, StatusCode,
        middleware::ProtectSensitiveData,
    },
    support::facades::Route,
};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};

struct InspectEarly;
impl Middleware for InspectEarly {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let expected = request.uri().path().starts_with("/reset/");
            assert_eq!(ProtectSensitiveData::current_path().is_some(), expected);
            if expected {
                assert_eq!(
                    ProtectSensitiveData::current_path().as_deref(),
                    Some("/reset/{token}")
                );
            }
            Ok(next.run(request).await)
        })
    }
}
fn respond(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        tokio::task::yield_now().await;
        assert!(
            tokio::spawn(async { ProtectSensitiveData::current_path() })
                .await?
                .is_none()
        );
        Response::json(
            StatusCode::OK,
            json!({"path": ProtectSensitiveData::current_path()}),
        )
    })
}
#[tokio::test]
async fn sensitive_routes_are_marked_before_global_middleware_and_do_not_leak_scope() {
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().to_path_buf())
        .with_config(|_, _| {
            Ok(BTreeMap::from([(
                "app".into(),
                json!({"env":"testing","locale":"en"}),
            )]))
        })
        .with_middleware(|middleware| {
            middleware.append(Arc::new(InspectEarly));
        })
        .with_routing(Routing::default().web(|| {
            Route::group(openrust::routing::RouteGroup::new(), |router| {
                router
                    .get("/reset/{token}", respond)?
                    .middleware_name("sensitive");
                Ok(())
            })?;
            Route::get("/ordinary", respond)?;
            Ok(())
        }))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    for path in ["/reset/SECRET?code=PRIVATE", "/ordinary", "/reset/OTHER"] {
        let mut request = Request::from(
            http::Request::builder()
                .uri(path)
                .body(Body::empty())
                .unwrap(),
        );
        let response = kernel.handle(&mut request).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(ProtectSensitiveData::current_path().is_none());
        if path.starts_with("/reset") {
            request.parts_mut().uri = "/ordinary".parse().unwrap();
            assert_eq!(kernel.handle(&mut request).await.status(), StatusCode::OK);
            assert!(ProtectSensitiveData::path(request.attributes()).is_none());
        }
    }
    let mut request = Request::from(
        http::Request::builder()
            .method("POST")
            .uri("/reset/PRIVATE")
            .body(Body::empty())
            .unwrap(),
    );
    assert_eq!(
        kernel.handle(&mut request).await.status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
    assert_eq!(
        ProtectSensitiveData::path(request.attributes()).as_deref(),
        Some("/reset/{token}")
    );
}
