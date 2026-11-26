use std::{error::Error, fs, sync::Arc, time::Duration};

use openrust::{
    foundation::configuration::Routing,
    http::{HttpError, Response, StatusCode},
    support::facades::Route,
    testing::TestApp,
};
use openrust_ai::{Agent, Ai, AiError, PromptOptions, agent};
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    time::timeout,
};

#[tokio::test]
async fn on_demand_providers_rebuild_and_expire_with_the_application_scope()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let upstream = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/tenant/v1", upstream.local_addr()?);
    let fixture = tokio::spawn(async move {
        let (socket, _) = timeout(Duration::from_secs(5), upstream.accept())
            .await
            .unwrap()
            .unwrap();
        let mut socket = BufReader::new(socket);
        let mut line = String::new();
        socket.read_line(&mut line).await.unwrap();
        assert_eq!(line.trim_end(), "POST /tenant/v1/chat/completions HTTP/1.1");
        let mut size = 0;
        let mut authorized = false;
        loop {
            line.clear();
            assert_ne!(socket.read_line(&mut line).await.unwrap(), 0);
            if line == "\r\n" {
                break;
            }
            let (key, value) = line.split_once(':').unwrap();
            if key.eq_ignore_ascii_case("authorization") {
                authorized = value.trim() == "Bearer fixture-key";
            }
            if key.eq_ignore_ascii_case("content-length") {
                size = value.trim().parse().unwrap();
            }
        }
        assert!(authorized);
        let mut body = vec![0; size];
        socket.read_exact(&mut body).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap()["model"],
            "tenant-model"
        );
        let payload = r#"{"choices":[{"message":{"content":"ready"}}]}"#;
        let wire = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        );
        socket.get_mut().write_all(wire.as_bytes()).await.unwrap();
    });

    let mut app = TestApp::create(|builder| {
        builder
            .with_providers(openrust_ai::openrust_providers())
            .with_routing(Routing::default().web(|| {
                Route::get("/build", || async {
                    let manager = Ai::manager().unwrap();
                    let regular = manager.text_provider(Some("deepseek")).unwrap();
                    let mut config = manager.get_instance_config("deepseek").unwrap();
                    config["name"] = "cohere".into();
                    assert!(matches!(
                        Ai::build(config.clone()),
                        Err(AiError::ProviderNameTaken)
                    ));
                    config["name"] = "deepseek".into();
                    assert!(matches!(
                        Ai::build(config.clone()),
                        Err(AiError::ProviderNameTaken)
                    ));
                    config["name"] = "tenant".into();
                    config["models"]["text"]["default"] = "tenant-model".into();
                    let created = Ai::build(config.clone()).unwrap();
                    assert!(Arc::ptr_eq(
                        &created,
                        &manager.text_provider(Some("tenant")).unwrap()
                    ));
                    assert_eq!(
                        manager.get_instance_config("tenant").unwrap()["ondemand"],
                        true
                    );
                    let mut invalid = config.clone();
                    invalid["url"] = String::new().into();
                    assert!(Ai::build(invalid).is_err());
                    assert!(Arc::ptr_eq(
                        &created,
                        &manager.text_provider(Some("tenant")).unwrap()
                    ));
                    manager.forget_instance("tenant");
                    assert!(!Arc::ptr_eq(
                        &created,
                        &manager.text_provider(Some("tenant")).unwrap()
                    ));

                    config.as_object_mut().unwrap().remove("name");
                    let generated = Ai::build(config.clone()).unwrap();
                    assert!(
                        generated.name().starts_with("ondemand_") && generated.name().len() == 41
                    );
                    let next = Ai::build(config).unwrap();
                    assert_eq!(next.name(), generated.name());
                    assert!(!Arc::ptr_eq(&next, &generated));

                    let result = agent("")
                        .prompt_with(
                            "fixture",
                            PromptOptions {
                                provider: Some("tenant"),
                                ..PromptOptions::default()
                            },
                        )
                        .await
                        .unwrap();
                    assert_eq!(result.text, "ready");
                    assert_eq!(result.provider, "tenant");
                    Ai::flush_state().unwrap();
                    assert!(matches!(
                        manager.get_instance_config(generated.name()),
                        Err(AiError::ProviderNotConfigured)
                    ));
                    assert!(Arc::ptr_eq(
                        &regular,
                        &manager.text_provider(Some("deepseek")).unwrap()
                    ));
                    let mut transient = manager.get_instance_config("deepseek").unwrap();
                    transient["name"] = "transient".into();
                    Ai::build(transient).unwrap();
                    Ok::<_, HttpError>(Response::empty(StatusCode::OK))
                })?;
                Route::get("/check", || async {
                    assert!(matches!(
                        Ai::text_provider(Some("transient")),
                        Err(AiError::ProviderNotConfigured)
                    ));
                    Ok::<_, HttpError>(Response::empty(StatusCode::OK))
                })
            }))
    })?;
    fs::write(
        app.root().join(".env.testing"),
        format!("DEEPSEEK_URL={url}\nDEEPSEEK_API_KEY=fixture-key\nDEEPSEEK_MODEL=default-model\n"),
    )?;
    assert_eq!(app.get("/build").await?.status(), StatusCode::OK);
    assert_eq!(app.get("/check").await?.status(), StatusCode::OK);
    fixture.await?;
    Ok(())
}
