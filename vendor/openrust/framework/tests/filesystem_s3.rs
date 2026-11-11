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
    filesystem::{FilesystemError, FilesystemManager, S3DiskConfig, Visibility, WriteOptions},
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
        let repository = Repository::new(BTreeMap::from([(
            "filesystems".into(),
            json!({
                "default": "s3", "disks": {
                    "s3": disk("tenant-a", true), "isolated": disk("tenant-b", false),
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
