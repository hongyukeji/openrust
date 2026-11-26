use std::{error::Error, fs, time::Duration};

use openrust::{
    filesystem::Storage,
    foundation::configuration::Routing,
    http::{HttpError, Response, StatusCode},
    support::{Str, facades::Route},
    testing::TestApp,
};
use openrust_ai::{AiError, Image, ImageAttachment, ImageFormat};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    time::timeout,
};

const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAIAAAB7QOjdAAAAD0lEQVR4nGPg2S9/4D8DAAjEAqoqtepMAAAAAElFTkSuQmCC";

#[tokio::test]
async fn sdk_image_generation_edits_and_storage_use_the_public_entry_points()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let upstream = TcpListener::bind("127.0.0.1:0").await?;
    let address = upstream.local_addr()?;
    let provider = tokio::spawn(async move {
        for (path, valid) in [
            ("generations", true),
            ("edits", true),
            ("generations", false),
        ] {
            let (socket, _) = timeout(Duration::from_secs(5), upstream.accept())
                .await
                .unwrap()
                .unwrap();
            let mut socket = BufReader::new(socket);
            let mut line = String::new();
            socket.read_line(&mut line).await.unwrap();
            assert_eq!(line.trim_end(), format!("POST /v1/images/{path} HTTP/1.1"));
            let mut size = 0;
            let mut kind = String::new();
            let mut authorized = false;
            loop {
                line.clear();
                assert_ne!(socket.read_line(&mut line).await.unwrap(), 0);
                if line == "\r\n" {
                    break;
                }
                let (key, value) = line.split_once(':').unwrap();
                let value = value.trim();
                match key.to_ascii_lowercase().as_str() {
                    "content-length" => size = value.parse().unwrap(),
                    "authorization" => authorized = value == "Bearer image-fixture",
                    "content-type" => kind = value.into(),
                    _ => (),
                }
            }
            assert!(authorized);
            let mut payload = vec![0; size];
            socket.read_exact(&mut payload).await.unwrap();
            if path == "generations" {
                let body: Value = serde_json::from_slice(&payload).unwrap();
                assert_eq!(kind, "application/json");
                if valid {
                    assert_eq!(
                        body,
                        json!({
                            "model":"image-default","prompt":"Sketch a pear","quality":"high","size":"1024x1536",
                        })
                    );
                }
            } else {
                assert!(kind.starts_with("multipart/form-data; boundary="));
                let body = String::from_utf8_lossy(&payload);
                assert_eq!(body.matches("name=\"image[]\"").count(), 2);
                for expected in [
                    "image/png",
                    "reference-0.png",
                    "reference-1.png",
                    "image-override",
                    "1536x1024",
                    "Edit the pear",
                ] {
                    assert!(body.contains(expected), "{expected}");
                }
                let png = Str::from_base64(PNG).unwrap();
                assert_eq!(
                    payload
                        .windows(png.len())
                        .filter(|window| *window == png)
                        .count(),
                    2
                );
            }
            let output=json!({
                "data":[{"b64_json":if valid {PNG} else {"%%%"},"revised_prompt":"A simple pear"}],
                "model":"image-response","usage":{"input_tokens":20,"output_tokens":15,
                "input_tokens_details":{"image_tokens":12},"output_tokens_details":{"image_tokens":13}},
            }).to_string();
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                output.len()
            );
            socket.get_mut().write_all(head.as_bytes()).await.unwrap();
            socket.get_mut().write_all(output.as_bytes()).await.unwrap();
        }
    });
    let mut app = TestApp::create(|builder| {
        builder
            .with_providers(openrust_ai::openrust_providers())
            .with_config(|base, _| {
                Ok([(
                    "filesystems".into(),
                    json!({"default":"local","disks":{
                        "local":{"driver":"local","root":base.join("private"),"throw":true},
                    }}),
                )]
                .into())
            })
            .with_routing(Routing::default().web(|| {
                Route::get("/images", || async {
                    assert!(matches!(Image::of("   "), Err(AiError::InvalidPrompt)));
                    assert!(matches!(
                        ImageAttachment::from_bytes(b"text".to_vec()),
                        Err(AiError::InvalidImage)
                    ));
                    let response = Image::of("Sketch a pear")
                        .unwrap()
                        .portrait()
                        .quality("high")
                        .generate()
                        .await
                        .unwrap();
                    assert_eq!(response.model, "image-response");
                    assert_eq!(response.usage.tokens.prompt_tokens, Some(20));
                    assert_eq!(response.usage.image_input_tokens, Some(12));
                    assert_eq!(response.usage.image_output_tokens, Some(13));
                    let image = response.first_image().unwrap();
                    assert_eq!(image.revised_prompt.as_deref(), Some("A simple pear"));
                    assert_eq!(image.format(), ImageFormat::Png);
                    let decoder = openrust::image::ImageManager::new(Default::default());
                    let image_size = decoder
                        .from_bytes(image.content().to_vec())
                        .dimensions()
                        .await
                        .unwrap();
                    assert_eq!(image_size, (2, 1));
                    assert_eq!(image.data_url(), format!("data:image/png;base64,{PNG}"));
                    let path = response
                        .store_as("generated", "pear.png", None)
                        .await
                        .unwrap();
                    assert_eq!(Storage::get(&path).await.unwrap().unwrap(), image.content());
                    assert!(
                        response
                            .store_as("generated", "../escape.png", None)
                            .await
                            .is_err()
                    );
                    let first = ImageAttachment::from_storage(&path, None).await.unwrap();
                    let second =
                        ImageAttachment::from_bytes(Str::from_base64(PNG).unwrap()).unwrap();
                    let response = Image::of("Edit the pear")
                        .unwrap()
                        .landscape()
                        .attachments([first, second])
                        .generate_with(Some("openai"), Some("image-override"))
                        .await
                        .unwrap();
                    assert_eq!(response.first_image().unwrap().content(), image.content());
                    assert!(matches!(
                        Image::of("Invalid upstream bytes")
                            .unwrap()
                            .generate()
                            .await,
                        Err(AiError::InvalidResponse)
                    ));
                    Ok::<_, HttpError>(Response::empty(StatusCode::OK))
                })
            }))
    })?;
    fs::write(
        app.root().join(".env.testing"),
        format!(
            "AI_IMAGE_PROVIDER=openai\nOPENAI_IMAGE_MODEL=image-default\nOPENAI_URL=http://{address}/v1\nOPENAI_API_KEY=image-fixture\n"
        ),
    )?;
    assert_eq!(app.get("/images").await?.status(), StatusCode::OK);
    provider.await?;
    Ok(())
}
