use openrust::{
    contracts::{foundation::Application as _, http::Kernel as _},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, Request},
};
use serde_json::json;

#[tokio::test]
async fn health_negotiates_content_and_preserves_application_isolation() {
    let mut kernels = Vec::new();
    for name in ["<First & 'app'>", "Second"] {
        let application = Application::configure(std::env::current_dir().unwrap())
            .withRouting(Routing::default().health("/up"))
            .create()
            .unwrap();
        let mut kernel = Kernel::new(application);
        kernel.bootstrap().await.unwrap();
        kernel
            .get_application()
            .config()
            .set("app.name", json!(name))
            .unwrap();
        kernels.push(kernel);
    }
    for (kernel, title) in kernels
        .iter()
        .zip(["&lt;First &amp; &#039;app&#039;&gt;", "Second"])
    {
        for (accept, is_json) in [
            ("text/html", false),
            ("application/json", true),
            ("application/problem+json", true),
            ("application/json;q=0.1, text/html;q=0.9", false),
        ] {
            for method in ["GET", "HEAD"] {
                let incoming = axum::http::Request::builder()
                    .method(method)
                    .uri("/up")
                    .header("accept", accept)
                    .body(Body::empty())
                    .unwrap();
                let (parts, body) = incoming.into_parts();
                let mut request = Request::from_parts(parts, body);
                let response = kernel.handle(&mut request).await;
                assert_eq!(response.status(), 200);
                assert_eq!(response.headers()["cache-control"], "no-cache, private");
                assert_eq!(
                    response.headers()["content-type"],
                    if is_json {
                        "application/json"
                    } else {
                        "text/html; charset=utf-8"
                    }
                );
                let body = axum::body::to_bytes(response.into_parts().1, 4096)
                    .await
                    .unwrap();
                if method == "HEAD" {
                    assert!(body.is_empty());
                } else if is_json {
                    assert_eq!(
                        serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
                        json!({"status":"up"})
                    );
                } else {
                    let html = std::str::from_utf8(&body).unwrap();
                    assert!(html.contains(&format!("<title>{title}</title>")));
                    assert!(html.contains("Application up"));
                }
            }
        }
    }
}
