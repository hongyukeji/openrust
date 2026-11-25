use super::*;
use openrust::{
    database::eloquent::Model,
    support::facades::{App, Config, Facade as _},
};
async fn server() -> Server {
    let server = start_security_catalog_mail(
        "full",
        false,
        false,
        "api",
        false,
        Some(auth_mail_queue_settings()),
    )
    .await;
    resource_cleanup::schema_and_disk(&server).await;
    let app = server._kernel.get_application();
    App::set_facade_application(Some(app), async {
        Config::set("filesystems.disks.public.visibility", json!("public")).unwrap();
        use openrust::database::migrations::{MigrationEntry, MigrationRegistry, Migrator};
        const PATH: &str =
            "database/photo-migrations/0001_01_01_000025_add_profile_photo_to_users_table.rs";
        static ENTRIES: [MigrationEntry; 1] = [MigrationEntry::new(
            PATH,
            &profile_photo_migration::Definition,
        )];
        let root = &server._root.0;
        std::fs::create_dir_all(root.join("database/photo-migrations")).unwrap();
        std::fs::write(
            root.join(PATH),
            include_str!("../../stubs/starter/migrations/profile-photo.rs"),
        )
        .unwrap();
        let manager = app
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap();
        let migrator = Migrator::new(
            manager,
            Arc::new(MigrationRegistry(&ENTRIES)),
            "photo_migrations".into(),
            root.clone(),
        );
        let paths = [root.join("database/photo-migrations")];
        assert_eq!(
            migrator
                .run(None, &paths, Default::default())
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(
            migrator
                .run(None, &paths, Default::default())
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            migrator
                .rollback(None, &paths, Default::default())
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            migrator
                .run(None, &paths, Default::default())
                .await
                .unwrap()
                .len(),
            1
        );
    })
    .await;
    register_token_user(&server).await;
    server
}
async fn token(server: &Server) -> String {
    let (status, _, body) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    assert_eq!(status, 201, "{body}");
    body["data"]["token"].as_str().unwrap().into()
}
async fn multipart(
    server: &Server,
    token: &str,
    proof: Option<&str>,
    bytes: &[u8],
) -> (u16, String, Value) {
    let boundary = "openrust-avatar-boundary";
    let mut body=format!("--{boundary}\r\nContent-Disposition: form-data; name=\"photo\"; filename=\"../../unsafe.svg\"\r\nContent-Type: image/jpeg\r\n\r\n").into_bytes();
    body.extend(bytes);
    body.extend(format!("\r\n--{boundary}--\r\n").as_bytes());
    let proof = proof
        .map(|v| format!("X-Auth-Confirmation: {v}\r\n"))
        .unwrap_or_default();
    let mut stream = TcpStream::connect(server.address).await.unwrap();
    stream.write_all(format!("POST /api/user/profile-photo HTTP/1.1\r\nHost: {}\r\nAccept: text/html\r\nAuthorization: Bearer {token}\r\n{proof}Content-Type: multipart/form-data; boundary={boundary}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",server.address,body.len()).as_bytes()).await.unwrap();
    stream.write_all(&body).await.unwrap();
    let mut wire = String::new();
    stream.read_to_string(&mut wire).await.unwrap();
    let (headers, body) = wire.split_once("\r\n\r\n").unwrap();
    (
        headers.split_whitespace().nth(1).unwrap().parse().unwrap(),
        headers.into(),
        serde_json::from_str(body).unwrap_or_else(|error| panic!("{error}: {body}")),
    )
}
#[tokio::test]
async fn avatar_real_decode_requires_recent_proof_and_replacement_returns_private_cleanup_receipt()
{
    let server = server().await;
    let token = token(&server).await;
    let bytes = include_bytes!("../fixtures/avatar.png");
    let (status, _, body) = multipart(&server, &token, None, bytes).await;
    assert_eq!(status, 401, "{body}");
    let proof = confirmation(&server, &token, "avatar").await;
    let (status, headers, body) = multipart(&server, &token, Some(&proof), bytes).await;
    assert_eq!(status, 200, "{body}");
    assert!(headers.contains("no-store"));
    assert!(
        body["data"]["avatar_url"]
            .as_str()
            .unwrap()
            .ends_with(".png")
    );
    assert!(body["data"].get("profile_photo_path").is_none());
    let first = body["data"]["avatar_url"].clone();
    let (status, _, _) = multipart(&server, &token, Some(&proof), bytes).await;
    assert_eq!(status, 401);
    let proof = confirmation(&server, &token, "avatar").await;
    let (status, _, body) = multipart(&server, &token, Some(&proof), bytes).await;
    assert_eq!(status, 202, "{body}");
    assert_ne!(first, body["data"]["avatar_url"]);
    assert_eq!(
        body["meta"]["cleanup"]["receipt"].as_str().unwrap().len(),
        64
    );
    let app = server._kernel.get_application();
    App::set_facade_application(Some(app), async {
        let user = app::models::User::query()
            .unwrap()
            .first()
            .await
            .unwrap()
            .unwrap();
        let disk = openrust::support::facades::Storage::disk(Some("public")).unwrap();
        assert!(
            disk.exists(match user.get_attribute("profile_photo_path").unwrap() {
                openrust::database::Value::Text(path) => path,
                _ => panic!("头像路径必须为字符串"),
            })
            .await
            .unwrap()
        );
    })
    .await;
}
#[tokio::test]
async fn avatar_rejects_forged_content_and_excess_dimensions_without_persisting_files() {
    let server = server().await;
    let token = token(&server).await;
    let proof = confirmation(&server, &token, "avatar").await;
    let (status, _, body) = multipart(&server, &token, Some(&proof), b"not an image").await;
    assert_eq!(status, 422, "{body}");
    // PNG签名通过类型探测，但被截断的数据必须在真实解码时失败。
    let truncated = &include_bytes!("../fixtures/avatar.png")[..55];
    assert_eq!(
        multipart(&server, &token, Some(&proof), truncated).await.0,
        422
    );
    let app = server._kernel.get_application();
    App::set_facade_application(Some(app), async {
        Config::set("user.avatar.max_dimension", json!(1)).unwrap()
    })
    .await;
    let (status, _, body) = multipart(
        &server,
        &token,
        Some(&proof),
        include_bytes!("../fixtures/avatar.png"),
    )
    .await;
    assert_eq!(status, 422, "{body}");
    assert!(body.to_string().contains("尺寸"));
}
async fn receipt(
    server: &Server,
    id: &str,
    secret: Option<&str>,
    query: bool,
) -> (u16, String, Value) {
    let path = format!(
        "/api/auth/resource-cleanups/{id}{}",
        if query { "?receipt=forbidden" } else { "" }
    );
    let header = secret
        .map(|s| format!("X-Operation-Receipt: {s}\r\n"))
        .unwrap_or_default();
    let mut stream = TcpStream::connect(server.address).await.unwrap();
    stream.write_all(format!("GET {path} HTTP/1.1\r\nHost: {}\r\nAccept: text/html\r\n{header}Connection: close\r\n\r\n",server.address).as_bytes()).await.unwrap();
    let mut wire = String::new();
    stream.read_to_string(&mut wire).await.unwrap();
    let (headers, body) = wire.split_once("\r\n\r\n").unwrap();
    (
        headers.split_whitespace().nth(1).unwrap().parse().unwrap(),
        headers.into(),
        serde_json::from_str(body).unwrap(),
    )
}
#[tokio::test]
async fn avatar_delete_returns_202_and_status_needs_only_header_receipt_without_session() {
    let server = server().await;
    let token = token(&server).await;
    let proof = confirmation(&server, &token, "avatar").await;
    assert_eq!(
        multipart(
            &server,
            &token,
            Some(&proof),
            include_bytes!("../fixtures/avatar.png")
        )
        .await
        .0,
        200
    );
    let proof = confirmation(&server, &token, "avatar").await;
    let (status, headers, body) = account_wire_request(
        server.address,
        &token,
        "DELETE",
        "/api/user/profile-photo",
        Value::Null,
        Some(&proof),
    )
    .await;
    assert_eq!(status, 202, "{body}");
    assert!(headers.contains("no-store"));
    assert!(body["data"]["avatar_url"].is_null());
    let info = &body["meta"]["cleanup"];
    let id = info["id"].as_str().unwrap();
    let secret = info["receipt"].as_str().unwrap();
    for secret in [None, Some("wrong")] {
        let (status, headers, body) = receipt(&server, id, secret, false).await;
        assert_eq!(status, 404, "{body}");
        assert!(headers.contains("no-store"));
        assert!(!headers.to_ascii_lowercase().contains("set-cookie:"));
    }
    assert_eq!(receipt(&server, id, Some(secret), true).await.0, 400);
    let (status, headers, status_body) = receipt(&server, id, Some(secret), false).await;
    assert_eq!(status, 200, "{status_body}");
    assert!(headers.contains("no-store"));
    assert!(!headers.to_ascii_lowercase().contains("set-cookie:"));
    assert_eq!(status_body["data"]["status"], "pending");
    assert!(!status_body.to_string().contains(secret));
    assert!(status_body["data"].get("resources").is_none());
    let app = server._kernel.get_application();
    App::set_facade_application(Some(app), async {
        let result = resource_cleanup::worker().await;
        assert_eq!(result, 0);
    })
    .await;
    assert_eq!(
        receipt(&server, id, Some(secret), false).await.2["data"]["status"],
        "completed"
    );
}
async fn web_upload(client: &mut Client, bytes: &[u8]) -> (u16, String, String) {
    let boundary = "openrust-web-avatar";
    let mut body=format!("--{boundary}\r\nContent-Disposition: form-data; name=\"photo\"; filename=\"avatar.png\"\r\nContent-Type: image/png\r\n\r\n").into_bytes();
    body.extend(bytes);
    body.extend(format!("\r\n--{boundary}--\r\n").as_bytes());
    let cookies = client
        .cookies
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("; ");
    let mut socket = TcpStream::connect(client.address).await.unwrap();
    socket.write_all(format!("POST /user/profile-photo HTTP/1.1\r\nHost: {}\r\nAccept: text/html\r\nCookie: {cookies}\r\nX-CSRF-TOKEN: {}\r\nContent-Type: multipart/form-data; boundary={boundary}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",client.address,client.csrf,body.len()).as_bytes()).await.unwrap();
    socket.write_all(&body).await.unwrap();
    let mut wire = String::new();
    socket.read_to_string(&mut wire).await.unwrap();
    let (headers, body) = wire.split_once("\r\n\r\n").unwrap();
    for line in headers.lines() {
        if let Some(cookie) = line.strip_prefix("set-cookie: ") {
            let (k, v) = cookie.split(';').next().unwrap().split_once('=').unwrap();
            client.cookies.insert(k.into(), v.into());
        }
    }
    (
        headers.split_whitespace().nth(1).unwrap().parse().unwrap(),
        headers.into(),
        body.into(),
    )
}
#[tokio::test]
async fn avatar_native_html_form_requires_csrf_and_recent_password_then_serves_image() {
    let server = server().await;
    let mut client = Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    client.send("GET", "/login", "", false).await;
    assert_eq!(
        client
            .send(
                "POST",
                "/login",
                r#"{"email":"device@example.test","password":"long-password-123"}"#,
                true
            )
            .await
            .0,
        200
    );
    let (status, body) = client.send("GET", "/home", "", false).await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("multipart/form-data"));
    assert!(body.contains("上传头像"));
    let bytes = include_bytes!("../fixtures/avatar.png");
    let (status, headers, body) = web_upload(&mut client, bytes).await;
    assert_eq!(status, 302, "{body}");
    assert!(headers.contains("confirm-password"));
    client.send("GET", "/home", "", false).await;
    assert_eq!(
        client
            .send(
                "POST",
                "/user/confirm-password",
                r#"{"password":"long-password-123"}"#,
                true
            )
            .await
            .0,
        201
    );
    let csrf = std::mem::take(&mut client.csrf);
    assert_eq!(web_upload(&mut client, bytes).await.0, 419);
    client.csrf = csrf;
    let (status, _, body) = web_upload(&mut client, bytes).await;
    assert_eq!(status, 302, "{body}");
    let (status, body) = client.send("GET", "/home", "", false).await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("头像已更新"));
    let url = body
        .split("<img src=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    let path = format!("/storage/{}", url.split("/storage/").nth(1).unwrap());
    let mut socket = TcpStream::connect(server.address).await.unwrap();
    socket
        .write_all(
            format!(
                "GET {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
                server.address
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut wire = Vec::new();
    socket.read_to_end(&mut wire).await.unwrap();
    let at = wire.windows(4).position(|v| v == b"\r\n\r\n").unwrap();
    let headers = String::from_utf8_lossy(&wire[..at]);
    assert!(headers.starts_with("HTTP/1.1 200"), "{headers}");
    assert!(headers.contains("image/png"));
    assert_eq!(&wire[at + 4..], bytes);
    let (status, headers, body) =
        form_request(&mut client, "/user/profile-photo", "_method=DELETE").await;
    assert_eq!(status, 302, "{body}");
    assert!(!headers.contains("confirm-password"), "{headers} {body}");
    assert!(
        !client
            .send("GET", "/home", "", false)
            .await
            .1
            .contains("<img src=")
    );
}

#[tokio::test]
async fn avatar_client_auth_errors_remain_json_with_empty_or_multilevel_prefix_and_html_accept() {
    for prefix in ["", "api/admin"] {
        let server = start_security("api", false, false, prefix).await;
        let mut client = Client {
            address: server.address,
            cookies: Default::default(),
            csrf: String::new(),
        };
        let path = format!("/{}/user/profile-photo", prefix.trim_matches('/')).replace("//", "/");
        let (status, headers, body) = client.exchange("DELETE", &path, "", false).await;
        assert_eq!(status, 401, "{prefix}: {body}");
        assert!(headers.contains("application/json"));
        assert!(headers.contains("no-store"));
        assert!(!headers.to_ascii_lowercase().contains("set-cookie:"));
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap()["code"],
            "unauthenticated"
        );
    }
}

struct RejectPhoto;
impl openrust::database::eloquent::concerns::ModelEventListener<app::models::User> for RejectPhoto {
    fn handle<'a>(
        &'a self,
        _: openrust::database::eloquent::concerns::ModelEvent,
        model: &'a mut app::models::User,
        _: &'a openrust::database::Connection,
    ) -> openrust::database::DatabaseFuture<
        'a,
        openrust::database::DatabaseResult<
            openrust::database::eloquent::concerns::ModelEventResponse,
        >,
    > {
        Box::pin(async move {
            use openrust::database::eloquent::concerns::ModelEventResponse as Event;
            Ok(
                if model.get_attribute("profile_photo_path")
                    != model.get_raw_original("profile_photo_path")
                {
                    Event::Cancel
                } else {
                    Event::Continue
                },
            )
        })
    }
}
#[tokio::test]
async fn avatar_cancelled_model_save_rolls_back_reference_and_compensates_new_file() {
    let server = server().await;
    let token = token(&server).await;
    let proof = confirmation(&server, &token, "avatar").await;
    app::models::User::register_model_event(
        server._kernel.get_application(),
        openrust::database::eloquent::concerns::ModelEvent::Saving,
        RejectPhoto,
    )
    .unwrap();
    let (status, headers, _) = multipart(
        &server,
        &token,
        Some(&proof),
        include_bytes!("../fixtures/avatar.png"),
    )
    .await;
    assert_eq!(status, 500);
    assert!(headers.contains("no-store"));
    let app = server._kernel.get_application();
    App::set_facade_application(Some(app), async {
        let user = app::models::User::query()
            .unwrap()
            .first()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            user.get_attribute("profile_photo_path"),
            Some(&openrust::database::Value::Null)
        );
        let disk = openrust::support::facades::Storage::disk(Some("public")).unwrap();
        assert!(disk.files(Some("profile-photos")).await.unwrap().is_empty());
        let db = app
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap()
            .connection(None)
            .unwrap();
        assert_eq!(
            db.table("user_resource_cleanups")
                .r#where("status", "=", "processing")
                .count("*")
                .await
                .unwrap(),
            1
        );
    })
    .await;
    assert_eq!(bearer_request(server.address, &token).await, 200);
}

#[tokio::test]
async fn avatar_concurrent_replacement_keeps_only_current_reference_after_worker() {
    let server = server().await;
    let token = token(&server).await;
    let left = confirmation(&server, &token, "avatar").await;
    let right = confirmation(&server, &token, "avatar").await;
    let bytes = include_bytes!("../fixtures/avatar.png");
    let (a, b) = tokio::join!(
        multipart(&server, &token, Some(&left), bytes),
        multipart(&server, &token, Some(&right), bytes)
    );
    let mut statuses = [a.0, b.0];
    statuses.sort();
    assert_eq!(statuses, [200, 202], "{} {}", a.2, b.2);
    let app = server._kernel.get_application();
    App::set_facade_application(Some(app), async {
        let user = app::models::User::query()
            .unwrap()
            .first()
            .await
            .unwrap()
            .unwrap();
        let path = match user.get_attribute("profile_photo_path").unwrap() {
            openrust::database::Value::Text(path) => path,
            _ => panic!("头像路径必须为字符串"),
        };
        let disk = openrust::support::facades::Storage::disk(Some("public")).unwrap();
        assert_eq!(disk.files(Some("profile-photos")).await.unwrap().len(), 2);
        assert_eq!(resource_cleanup::worker().await, 0);
        assert!(disk.exists(path).await.unwrap());
        assert_eq!(
            disk.files(Some("profile-photos")).await.unwrap(),
            vec![path.clone()]
        );
    })
    .await;
}
#[tokio::test]
async fn avatar_storage_write_failure_keeps_account_and_tracks_any_staged_cleanup() {
    let server = server().await;
    let token = token(&server).await;
    let proof = confirmation(&server, &token, "avatar").await;
    let root = server._root.0.join("public-photos");
    std::fs::write(&root, b"owned blocked directory fixture").unwrap();
    let (status, headers, _) = multipart(
        &server,
        &token,
        Some(&proof),
        include_bytes!("../fixtures/avatar.png"),
    )
    .await;
    assert_eq!(status, 500);
    assert!(headers.contains("no-store"));
    std::fs::remove_file(&root).unwrap();
    std::fs::create_dir(&root).unwrap();
    let app = server._kernel.get_application();
    App::set_facade_application(Some(app), async {
        let user = app::models::User::query()
            .unwrap()
            .first()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            user.get_attribute("profile_photo_path"),
            Some(&openrust::database::Value::Null)
        );
        let db = app
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap()
            .connection(None)
            .unwrap();
        db.table("user_resource_cleanups")
            .update([("created_at".into(), 0.into())].into())
            .await
            .unwrap();
        db.table("jobs")
            .r#where("queue", "=", "user-resources")
            .update([("available_at".into(), 0.into())].into())
            .await
            .unwrap();
        assert_eq!(resource_cleanup::worker().await, 0);
        assert!(
            openrust::support::facades::Storage::disk(Some("public"))
                .unwrap()
                .files(Some("profile-photos"))
                .await
                .unwrap()
                .is_empty()
        );
    })
    .await;
    assert_eq!(bearer_request(server.address, &token).await, 200);
}

#[tokio::test]
async fn avatar_disabled_projection_does_not_parse_old_photo_metadata_or_resolve_storage() {
    let server = server().await;
    let app = server._kernel.get_application();
    App::set_facade_application(Some(app), async {
        Config::set("user.avatar.enabled", json!(false)).unwrap();
        let mut user = app::models::User::query()
            .unwrap()
            .first()
            .await
            .unwrap()
            .unwrap();
        user.set_attribute("profile_photo_path", "legacy-unmanaged-path");
        user.set_attribute("profile_photo_disk", 42);
        assert!(
            openrust_fortify::starter::UserResource::make(&user).unwrap()["avatar_url"].is_null()
        );
    })
    .await;
}
