use std::{
    collections::BTreeMap,
    env, io,
    path::PathBuf,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, Instant},
};

use aws_sdk_s3::{
    Client,
    config::{BehaviorVersion, Credentials, Region},
    types::{ObjectCannedAcl, ObjectOwnership, OwnershipControls, OwnershipControlsRule},
};
use openrust::{
    config::Repository,
    filesystem::{
        FilesystemError, FilesystemManager, S3DiskConfig, TemporaryUploadUrlOptions,
        TemporaryUrlOptions, Visibility, WriteOptions,
    },
    http::HeaderMap,
};
use serde_json::json;
use tokio::io::{AsyncRead, AsyncReadExt as _, ReadBuf};

struct Service {
    endpoint: String,
    key: String,
    secret: String,
    bucket: String,
    client: Client,
}

impl Service {
    async fn start(variable: &str) -> Option<Self> {
        let endpoint = env::var(format!("{variable}_ENDPOINT")).ok()?;
        let key = env::var(format!("{variable}_ACCESS_KEY")).expect("test access key");
        let secret = env::var(format!("{variable}_SECRET_KEY")).expect("test secret key");
        let config = aws_sdk_s3::config::Builder::new()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new("us-east-1"))
            .endpoint_url(&endpoint)
            .force_path_style(true)
            .credentials_provider(Credentials::new(&key, &secret, None, None, "integration"))
            .build();
        let client = Client::from_conf(config);
        let bucket = format!("openrust-{:016x}", fastrand::u64(..));
        client.create_bucket().bucket(&bucket).send().await.unwrap();
        Some(Self {
            endpoint,
            key,
            secret,
            bucket,
            client,
        })
    }

    fn manager(&self) -> FilesystemManager {
        let disk = |root: &str, throw| {
            json!({
                "driver": "s3", "bucket": self.bucket, "region": "us-east-1",
                "key": self.key, "secret": self.secret, "endpoint": self.endpoint,
                "root": root, "use_path_style_endpoint": true, "throw": throw,
            })
        };
        let mut temporary = disk("tenant-a", true);
        temporary["temporary_url"] = json!("http://download.example.test:8080/ignored?unused=1");
        let mut invalid_temporary = disk("tenant-a", true);
        invalid_temporary["temporary_url"] = json!("ftp://download.example.test");
        let repository = Repository::new(BTreeMap::from([(
            "filesystems".into(),
            json!({
                "default": "s3", "disks": {
                    "s3": disk("tenant-a", true), "isolated": disk("tenant-b", false),
                    "temporary": temporary, "invalid-temporary": invalid_temporary,
                    "cdn": {
                        "driver": "s3", "bucket": self.bucket, "region": "us-east-1",
                        "root": "tenant-a", "url": "https://cdn.example.test/files/",
                    },
                }
            }),
        )]));
        FilesystemManager::new(Arc::new(repository), PathBuf::from("."))
    }

    async fn set_ownership(&self, ownership: ObjectOwnership) {
        self.client
            .put_bucket_ownership_controls()
            .bucket(&self.bucket)
            .ownership_controls(
                OwnershipControls::builder()
                    .rules(
                        OwnershipControlsRule::builder()
                            .object_ownership(ownership)
                            .build()
                            .unwrap(),
                    )
                    .build()
                    .unwrap(),
            )
            .send()
            .await
            .unwrap();
    }

    async fn uploads(&self) -> usize {
        self.client
            .list_multipart_uploads()
            .bucket(&self.bucket)
            .send()
            .await
            .unwrap()
            .uploads()
            .len()
    }

    async fn clear(self) {
        let contents = self
            .client
            .list_objects_v2()
            .bucket(&self.bucket)
            .send()
            .await
            .unwrap();
        for item in contents.contents() {
            self.client
                .delete_object()
                .bucket(&self.bucket)
                .key(item.key().unwrap())
                .send()
                .await
                .unwrap();
        }
        self.client
            .delete_bucket()
            .bucket(&self.bucket)
            .send()
            .await
            .unwrap();
    }
}

struct FailRead;

impl AsyncRead for FailRead {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Ready(Err(io::Error::other("source interrupted")))
    }
}

#[tokio::test]
async fn configured_s3_disks_stream_real_objects_and_clean_failed_uploads() {
    let Some(service) = Service::start("OPENRUST_TEST_S3").await else {
        return;
    };
    let manager = service.manager();
    let disk = manager.disk(None).unwrap();
    let isolated = manager.disk(Some("isolated")).unwrap();
    assert!(Arc::ptr_eq(&disk, &manager.disk(Some("s3")).unwrap()));
    assert_eq!(
        disk.path("a/space + 你好.txt").unwrap(),
        PathBuf::from("tenant-a/a/space + 你好.txt")
    );
    assert!(matches!(
        disk.put("../escape", b"bad").await,
        Err(FilesystemError::InvalidPath)
    ));
    assert!(disk.put("a/space + 你好.txt", b"hello").await.unwrap());
    let suffix = "tenant-a/a/space%20%2B%20%E4%BD%A0%E5%A5%BD.txt";
    assert_eq!(
        disk.url("a/space + 你好.txt").unwrap(),
        format!("{}/{}/{suffix}", service.endpoint, service.bucket)
    );
    assert_eq!(
        manager
            .disk(Some("cdn"))
            .unwrap()
            .url("a/space + 你好.txt")
            .unwrap(),
        format!("https://cdn.example.test/files/{suffix}")
    );
    let (headers, body) = disk
        .download("a/space + 你好.txt", Some("report.txt"), HeaderMap::new())
        .await
        .unwrap()
        .into_parts();
    assert_eq!(headers.headers["content-length"], "5");
    assert_eq!(
        headers.headers["content-disposition"],
        "attachment; filename=report.txt"
    );
    assert_eq!(
        axum::body::to_bytes(body, 5).await.unwrap().as_ref(),
        b"hello"
    );
    assert_eq!(
        disk.get("a/space + 你好.txt").await.unwrap().unwrap(),
        b"hello"
    );
    assert!(disk.directoryExists("a").await.unwrap());
    assert!(!disk.fileExists("a").await.unwrap());
    assert!(disk.exists("a").await.unwrap());
    assert!(isolated.missing("a/space + 你好.txt").await.unwrap());
    assert!(isolated.get("a/space + 你好.txt").await.unwrap().is_none());
    assert!(matches!(
        disk.get("missing").await,
        Err(FilesystemError::NotFound)
    ));
    let direct = service
        .client
        .get_object()
        .bucket(&service.bucket)
        .key("tenant-a/a/space + 你好.txt")
        .send()
        .await
        .unwrap();
    assert_eq!(
        direct.body.collect().await.unwrap().into_bytes().as_ref(),
        b"hello"
    );
    assert!(disk.put("empty", b"").await.unwrap());
    assert!(disk.get("empty").await.unwrap().unwrap().is_empty());

    let mut large = tokio::io::repeat(42).take(9 * 1024 * 1024);
    assert!(disk.writeStream("large", &mut large).await.unwrap());
    let mut stream = disk.readStream("large").await.unwrap().unwrap();
    let mut received = Vec::new();
    stream.read_to_end(&mut received).await.unwrap();
    assert_eq!(received.len(), 9 * 1024 * 1024);
    assert!(received.iter().all(|&value| value == 42));

    let mut left = tokio::io::repeat(65).take(9 * 1024 * 1024);
    let mut right = tokio::io::repeat(66).take(9 * 1024 * 1024);
    let (left, right) = tokio::join!(
        disk.writeStream("concurrent", &mut left),
        disk.writeStream("concurrent", &mut right)
    );
    assert!(left.unwrap() && right.unwrap());
    let final_bytes = disk.get("concurrent").await.unwrap().unwrap();
    assert_eq!(final_bytes.len(), 9 * 1024 * 1024);
    assert!(matches!(final_bytes[0], 65 | 66));
    assert!(final_bytes.iter().all(|byte| *byte == final_bytes[0]));

    let mut failed = tokio::io::repeat(11).take(8 * 1024 * 1024).chain(FailRead);
    assert!(matches!(
        disk.writeStream("a/space + 你好.txt", &mut failed).await,
        Err(FilesystemError::Backend { .. })
    ));
    assert_eq!(
        disk.get("a/space + 你好.txt").await.unwrap().unwrap(),
        b"hello"
    );
    assert_eq!(service.uploads().await, 0);

    let (_open, reader) = tokio::io::duplex(32);
    let mut interrupted = tokio::io::repeat(11).take(8 * 1024 * 1024).chain(reader);
    let task = {
        let disk = Arc::clone(&disk);
        tokio::spawn(async move { disk.writeStream("cancelled", &mut interrupted).await })
    };
    wait_for_uploads(&service, 1).await;
    assert!(disk.missing("cancelled").await.unwrap());
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    wait_for_uploads(&service, 0).await;
    assert!(disk.missing("cancelled").await.unwrap());

    let denied = manager
        .buildS3(S3DiskConfig {
            bucket: service.bucket.clone(),
            region: "us-east-1".into(),
            key: Some(service.key.clone()),
            secret: Some("wrong-secret".into()),
            token: None,
            endpoint: Some(service.endpoint.clone()),
            temporary_url: None,
            root: "".into(),
            use_path_style_endpoint: true,
            visibility: None,
            throw: false,
            report: false,
        })
        .unwrap();
    assert!(!denied.put("rejected", b"wrong credentials").await.unwrap());
    assert!(!disk.fileExists("rejected").await.unwrap());
    assert!(disk.delete("a/space + 你好.txt").await.unwrap());
    assert!(disk.missing("a/space + 你好.txt").await.unwrap());

    service.clear().await;
}

async fn wait_for_uploads(service: &Service, expected: usize) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while service.uploads().await != expected {
        assert!(
            Instant::now() < deadline,
            "expected {expected} multipart uploads"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn acl_compatible_backend_persists_public_and_private_visibility() {
    let Some(service) = Service::start("OPENRUST_TEST_S3_ACL").await else {
        return;
    };
    service.set_ownership(ObjectOwnership::ObjectWriter).await;
    let disk = service.manager().disk(None).unwrap();
    assert!(
        disk.putWithOptions(
            "acl",
            b"payload",
            WriteOptions {
                visibility: Some(Visibility::Public),
            }
        )
        .await
        .unwrap()
    );
    assert_eq!(disk.getVisibility("acl").await.unwrap(), Visibility::Public);
    assert_eq!(anonymous_status(&service, "tenant-a/acl").await, 200);
    assert!(
        disk.setVisibility("acl", Visibility::Private)
            .await
            .unwrap()
    );
    assert_eq!(
        disk.getVisibility("acl").await.unwrap(),
        Visibility::Private
    );

    assert_eq!(anonymous_status(&service, "tenant-a/acl").await, 403);
    let put = service
        .client
        .put_object_acl()
        .bucket(&service.bucket)
        .key("tenant-a/acl")
        .acl(ObjectCannedAcl::PublicRead)
        .send()
        .await;
    assert!(put.is_ok());
    assert_eq!(disk.getVisibility("acl").await.unwrap(), Visibility::Public);
    service.clear().await;
}

#[tokio::test]
async fn acl_disabled_backend_does_not_report_a_public_write_as_successful() {
    let Some(service) = Service::start("OPENRUST_TEST_S3_NO_ACL").await else {
        return;
    };
    service
        .set_ownership(ObjectOwnership::BucketOwnerEnforced)
        .await;
    let disk = service.manager().disk(None).unwrap();
    assert!(disk.put("acl", b"payload").await.unwrap());
    assert!(matches!(
        disk.setVisibility("acl", Visibility::Public).await,
        Err(FilesystemError::UnsupportedOperation)
    ));
    assert!(matches!(
        disk.putWithOptions(
            "acl",
            b"payload",
            WriteOptions {
                visibility: Some(Visibility::Public)
            }
        )
        .await,
        Err(FilesystemError::UnsupportedOperation | FilesystemError::PublishOutcomeUnknown)
    ));
    service.clear().await;
}

async fn anonymous_status(service: &Service, key: &str) -> u16 {
    use tokio::io::AsyncWriteExt as _;

    let endpoint = url::Url::parse(&service.endpoint).unwrap();
    let address = endpoint.socket_addrs(|| None).unwrap()[0];
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    socket
        .write_all(
            format!(
                "GET /{}/{key} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n",
                service.bucket
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = String::new();
    socket.read_to_string(&mut response).await.unwrap();
    response
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap()
}

#[tokio::test]
async fn presigned_download_and_upload_work_without_client_credentials() {
    let Some(service) = Service::start("OPENRUST_TEST_S3").await else {
        assert!(env::var_os("OPENRUST_REQUIRE_S3_PRESIGN").is_none());
        return;
    };
    let disk = service.manager().disk(None).unwrap();
    let path = "private/signed + 文本.txt";
    let expires = std::time::SystemTime::now() + Duration::from_secs(60);
    let upload = disk
        .temporaryUploadUrlWith(
            path,
            expires,
            TemporaryUploadUrlOptions {
                content_type: Some("text/plain".into()),
                metadata: Some([("probe".into(), "signed".into())].into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(upload.headers["content-type"], "text/plain");
    assert_eq!(upload.headers["x-amz-meta-probe"], "signed");
    let mut client = tokio::process::Command::new("curl");
    client.args(["--fail", "--silent", "--max-time", "10", "-X", "PUT"]);
    for (name, value) in &upload.headers {
        client
            .arg("-H")
            .arg(format!("{name}: {}", value.to_str().unwrap()));
    }
    let response = client
        .args(["--data-binary", "presigned payload", &upload.url])
        .output()
        .await
        .unwrap();
    assert!(response.status.success(), "presigned PUT failed");
    let download = disk
        .temporaryUrlWith(
            path,
            expires,
            TemporaryUrlOptions {
                response_content_type: Some("application/octet-stream".into()),
                response_content_disposition: Some("attachment; filename=report.txt".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let response = tokio::process::Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--include",
            "--max-time",
            "10",
            &download,
        ])
        .output()
        .await
        .unwrap();
    assert!(response.status.success(), "presigned GET failed");
    let body = String::from_utf8(response.stdout).unwrap();
    let (headers, payload) = body.split_once("\r\n\r\n").unwrap();
    assert!(
        headers
            .to_ascii_lowercase()
            .contains("content-type: application/octet-stream")
    );
    assert!(
        headers
            .to_ascii_lowercase()
            .contains("content-disposition: attachment; filename=report.txt")
    );
    assert_eq!(payload, "presigned payload");
    let tampered = download.replace("report.txt", "other.txt");
    assert_ne!(tampered, download);
    let response = tokio::process::Command::new("curl")
        .args(["--fail", "--silent", "--max-time", "10", &tampered])
        .output()
        .await
        .unwrap();
    assert!(
        !response.status.success(),
        "tampered download options were accepted"
    );
    let partial = disk
        .temporaryDownloadRequest(path, expires, |request| {
            request
                .bucket("ignored-bucket")
                .key("ignored-key")
                .range("bytes=0-8")
        })
        .await
        .unwrap();
    assert_eq!(partial.headers["range"], "bytes=0-8");
    let response = tokio::process::Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--max-time",
            "10",
            "-H",
            "Range: bytes=0-8",
            &partial.url,
        ])
        .output()
        .await
        .unwrap();
    assert!(response.status.success(), "presigned range GET failed");
    assert_eq!(response.stdout, b"presigned");
    let response = tokio::process::Command::new("curl")
        .args(["--fail", "--silent", "--max-time", "10", &partial.url])
        .output()
        .await
        .unwrap();
    assert!(
        !response.status.success(),
        "GET accepted a missing signed Range"
    );

    let conditional = disk
        .temporaryUploadRequest("private/create-only.txt", expires, |request| {
            request
                .bucket("ignored-bucket")
                .key("ignored-key")
                .if_none_match("*")
        })
        .await
        .unwrap();
    assert_eq!(conditional.headers["if-none-match"], "*");
    for (body, status) in [("first", b"200"), ("second", b"412")] {
        let response = tokio::process::Command::new("curl")
            .args([
                "--silent",
                "--max-time",
                "10",
                "--output",
                "/dev/null",
                "--write-out",
                "%{http_code}",
                "-X",
                "PUT",
                "-H",
                "If-None-Match: *",
                "--data-binary",
                body,
                &conditional.url,
            ])
            .output()
            .await
            .unwrap();
        assert_eq!(&response.stdout, status);
    }
    assert_eq!(
        disk.get("private/create-only.txt").await.unwrap().unwrap(),
        b"first"
    );
    let unsigned = download.split_once('?').unwrap().0;
    let response = tokio::process::Command::new("curl")
        .args(["--fail", "--silent", "--max-time", "10", unsigned])
        .output()
        .await
        .unwrap();
    assert!(!response.status.success(), "unsigned GET was accepted");
    assert!(
        disk.temporaryUrl(path, std::time::SystemTime::now() - Duration::from_secs(1))
            .await
            .is_err()
    );
    service.clear().await;
}

#[tokio::test]
async fn temporary_origin_preserves_signed_path_and_version() {
    use aws_sdk_s3::{
        primitives::ByteStream,
        types::{BucketVersioningStatus, VersioningConfiguration},
    };
    let Some(service) = Service::start("OPENRUST_TEST_S3").await else {
        assert!(env::var_os("OPENRUST_REQUIRE_S3_PRESIGN").is_none());
        return;
    };
    let manager = service.manager();
    assert!(matches!(
        manager.disk(Some("invalid-temporary")),
        Err(FilesystemError::InvalidConfiguration)
    ));
    let disk = manager.disk(Some("temporary")).unwrap();
    let path = "nested/version + % 文本.txt";
    let key = format!("tenant-a/{path}");
    let expires = std::time::SystemTime::now() + Duration::from_secs(60);
    service
        .client
        .put_bucket_versioning()
        .bucket(&service.bucket)
        .versioning_configuration(
            VersioningConfiguration::builder()
                .status(BucketVersioningStatus::Enabled)
                .build(),
        )
        .send()
        .await
        .unwrap();
    let mut versions = Vec::new();
    for body in ["original", "updated"] {
        let result = service
            .client
            .put_object()
            .bucket(&service.bucket)
            .key(&key)
            .body(ByteStream::from_static(body.as_bytes()))
            .send()
            .await
            .unwrap();
        versions.push(result.version_id().expect("versioned PUT").to_owned());
    }
    let signed = disk
        .temporaryUrlWith(
            path,
            expires,
            TemporaryUrlOptions {
                version_id: Some(versions[0].clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let signed = url::Url::parse(&signed).unwrap();
    assert_eq!(
        signed.origin().ascii_serialization(),
        "http://download.example.test:8080"
    );
    assert!(!signed.path().contains("/ignored"));
    assert!(!signed.query_pairs().any(|(key, _)| key == "unused"));
    assert!(
        signed
            .query_pairs()
            .any(|(key, value)| key == "versionId" && value == versions[0])
    );
    let target = service.endpoint.strip_prefix("http://").unwrap();
    let route = format!("download.example.test:8080:{target}");
    // Emulate a proxy restoring the Host that the SDK signed.
    let response = tokio::process::Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--noproxy",
            "*",
            "--max-time",
            "10",
            "--connect-to",
            &route,
            "-H",
            &format!("Host: {target}"),
            signed.as_str(),
        ])
        .output()
        .await
        .unwrap();
    assert!(response.status.success(), "versioned GET failed");
    assert_eq!(response.stdout, b"original");

    let upload = disk.temporaryUploadUrl(path, expires).await.unwrap();
    assert!(upload.url.starts_with("http://download.example.test:8080/"));
    assert!(!upload.url.contains("/ignored"));
    let mut request = tokio::process::Command::new("curl");
    request.args([
        "--fail",
        "--silent",
        "--noproxy",
        "*",
        "--max-time",
        "10",
        "--connect-to",
        &route,
        "-H",
        &format!("Host: {target}"),
        "-X",
        "PUT",
    ]);
    for (name, value) in &upload.headers {
        request
            .arg("-H")
            .arg(format!("{name}: {}", value.to_str().unwrap()));
    }
    let result = request
        .args(["--data-binary", "third", "--include", &upload.url])
        .output()
        .await
        .unwrap();
    assert!(result.status.success(), "rewritten PUT failed");
    let latest = service
        .client
        .get_object()
        .bucket(&service.bucket)
        .key(&key)
        .send()
        .await
        .unwrap();
    versions.push(latest.version_id().unwrap().to_owned());
    assert_eq!(
        latest.body.collect().await.unwrap().into_bytes().as_ref(),
        b"third"
    );
    for version in versions {
        service
            .client
            .delete_object()
            .bucket(&service.bucket)
            .key(&key)
            .version_id(version)
            .send()
            .await
            .unwrap();
    }
    service.clear().await;
}
