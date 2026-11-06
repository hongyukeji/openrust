//! 验证真实 S3-compatible API、版本绑定 URL 与多段上传。

use bytes::Bytes;
use figment::providers::Format;
use futures_util::{StreamExt, stream};
use openrust::storage::{
    Storage, StoragePath, StorageServiceProvider, StorageStream, UploadInspector, UploadOptions,
    Visibility,
};
use openrust::{Error, Result, core::ProviderBuilder};
use std::{sync::Arc, time::Duration};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
#[ignore = "需 ACL-enabled 的隔离真实 S3 bucket 与凭据；默认 ACL-disabled bucket 不可代替"]
async fn s3_visibility_copy_acl_read_and_change() -> TestResult {
    let endpoint = std::env::var("OPENRUST_TEST_S3_ENDPOINT")?;
    let bucket = std::env::var("OPENRUST_TEST_S3_ACL_BUCKET")?;
    let region = std::env::var("OPENRUST_TEST_S3_REGION")?;
    let access = std::env::var("OPENRUST_TEST_S3_ACCESS_KEY_ID")?;
    let secret = std::env::var("OPENRUST_TEST_S3_SECRET_ACCESS_KEY")?;
    let root = format!("visibility/{}", unique_suffix());
    let configuration = format!(
        "[storage]\ndefault_disk = 'media'\n[storage.disks.media]\ndriver = 's3'\nvisibility = 'private'\nbucket = {}\nregion = {}\nendpoint = {}\naccess_key_id = {}\nsecret_access_key = {}\nroot = {}\n",
        serde_json::to_string(&bucket)?,
        serde_json::to_string(&region)?,
        serde_json::to_string(&endpoint)?,
        serde_json::to_string(&access)?,
        serde_json::to_string(&secret)?,
        serde_json::to_string(&root)?
    );
    let mut builder = ProviderBuilder::new();
    builder.provider(StorageServiceProvider::new());
    let app = openrust::App::from_provider_runtime(
        builder
            .start(figment::Figment::new().merge(figment::providers::Toml::string(&configuration)))
            .await?,
    )?;
    let disk = app
        .state()
        .resolve::<Storage>()
        .await?
        .default_disk()
        .await?;
    let path = StoragePath::new("copy acl with spaces/object.bin")?;
    disk.put_stream(
        path.clone(),
        Some("application/octet-stream".into()),
        Box::pin(stream::iter([Ok(Bytes::from_static(b"public payload"))])),
        UploadOptions {
            visibility: Some(Visibility::Public),
            ..Default::default()
        },
        None,
    )
    .await?;
    assert_eq!(disk.visibility(&path).await?, Visibility::Public);
    disk.set_visibility(&path, Visibility::Private).await?;
    assert_eq!(disk.visibility(&path).await?, Visibility::Private);
    disk.put_stream(
        path.clone(),
        None,
        Box::pin(stream::iter([Ok(Bytes::from_static(
            b"private replacement",
        ))])),
        UploadOptions::default(),
        None,
    )
    .await?;
    assert_eq!(disk.visibility(&path).await?, Visibility::Private);
    disk.delete(&path).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "需真实 ACL-disabled bucket；验证明确拒绝且目标对象未发布"]
async fn s3_acl_disabled_bucket_rejects_public_copy() -> TestResult {
    let endpoint = std::env::var("OPENRUST_TEST_S3_ENDPOINT")?;
    let bucket = std::env::var("OPENRUST_TEST_S3_ACL_DISABLED_BUCKET")?;
    let region = std::env::var("OPENRUST_TEST_S3_REGION")?;
    let access = std::env::var("OPENRUST_TEST_S3_ACCESS_KEY_ID")?;
    let secret = std::env::var("OPENRUST_TEST_S3_SECRET_ACCESS_KEY")?;
    let root = format!("visibility/{}", unique_suffix());
    let configuration = format!(
        "[storage]\n[storage.disks.local]\ndriver = 's3'\nbucket = {}\nregion = {}\nendpoint = {}\naccess_key_id = {}\nsecret_access_key = {}\nroot = {}\n",
        serde_json::to_string(&bucket)?,
        serde_json::to_string(&region)?,
        serde_json::to_string(&endpoint)?,
        serde_json::to_string(&access)?,
        serde_json::to_string(&secret)?,
        serde_json::to_string(&root)?
    );
    let mut builder = ProviderBuilder::new();
    builder.provider(StorageServiceProvider::new());
    let app = openrust::App::from_provider_runtime(
        builder
            .start(figment::Figment::new().merge(figment::providers::Toml::string(&configuration)))
            .await?,
    )?;
    let disk = app
        .state()
        .resolve::<Storage>()
        .await?
        .default_disk()
        .await?;
    let path = StoragePath::new("acl-disabled/object.bin")?;
    let result = disk
        .put_stream(
            path.clone(),
            None,
            Box::pin(stream::iter([Ok(Bytes::from_static(b"not public"))])),
            UploadOptions {
                visibility: Some(Visibility::Public),
                ..Default::default()
            },
            None,
        )
        .await;
    let code = result
        .err()
        .ok_or("public copy unexpectedly succeeded")?
        .code()
        .to_owned();
    assert!(matches!(
        code.as_str(),
        "storage_acl_disabled" | "storage_acl_access_denied"
    ));
    assert!(disk.stat(&path).await.is_err());
    Ok(())
}

#[tokio::test]
#[ignore = "需 ACL-enabled 真实 S3 与 HTTP 延迟代理：代理须先转发 CopyObject 并在成功响应前延迟超过 30 秒"]
async fn s3_late_copy_response_keeps_complete_target_and_cleans_staging() -> TestResult {
    let endpoint = std::env::var("OPENRUST_TEST_S3_ENDPOINT")?;
    let delayed = std::env::var("OPENRUST_TEST_S3_DELAYED_COPY_ENDPOINT")?;
    let bucket = std::env::var("OPENRUST_TEST_S3_ACL_BUCKET")?;
    let region = std::env::var("OPENRUST_TEST_S3_REGION")?;
    let access = std::env::var("OPENRUST_TEST_S3_ACCESS_KEY_ID")?;
    let secret = std::env::var("OPENRUST_TEST_S3_SECRET_ACCESS_KEY")?;
    let root = format!("visibility-late/{}", unique_suffix());
    let configuration = format!(
        "[storage]\ndefault_disk = 'normal'\n[storage.disks.normal]\ndriver = 's3'\nbucket = {}\nregion = {}\nendpoint = {}\naccess_key_id = {}\nsecret_access_key = {}\nroot = {}\n[storage.disks.delayed]\ndriver = 's3'\nbucket = {}\nregion = {}\nendpoint = {}\naccess_key_id = {}\nsecret_access_key = {}\nroot = {}\n",
        serde_json::to_string(&bucket)?,
        serde_json::to_string(&region)?,
        serde_json::to_string(&endpoint)?,
        serde_json::to_string(&access)?,
        serde_json::to_string(&secret)?,
        serde_json::to_string(&root)?,
        serde_json::to_string(&bucket)?,
        serde_json::to_string(&region)?,
        serde_json::to_string(&delayed)?,
        serde_json::to_string(&access)?,
        serde_json::to_string(&secret)?,
        serde_json::to_string(&root)?,
    );
    let mut builder = ProviderBuilder::new();
    builder.provider(StorageServiceProvider::new());
    let app = openrust::App::from_provider_runtime(
        builder
            .start(figment::Figment::new().merge(figment::providers::Toml::string(&configuration)))
            .await?,
    )?;
    let storage = app.state().resolve::<Storage>().await?;
    let normal = storage.disk("normal").await?;
    let slow = storage.disk("delayed").await?;
    let path = StoragePath::new("late-copy/object.bin")?;
    normal
        .put_stream(
            path.clone(),
            None,
            Box::pin(stream::iter([Ok(Bytes::from_static(b"old object"))])),
            UploadOptions::default(),
            None,
        )
        .await?;
    let mut old = normal.open(&path, None).await?;
    let mut content = Vec::new();
    while let Some(chunk) = old.body.next().await {
        content.extend_from_slice(&chunk?);
    }
    assert_eq!(content, b"old object");
    let result = slow
        .put_stream(
            path.clone(),
            None,
            Box::pin(stream::iter([Ok(Bytes::from_static(
                b"complete replacement",
            ))])),
            UploadOptions {
                visibility: Some(Visibility::Public),
                ..Default::default()
            },
            None,
        )
        .await;
    assert_eq!(
        result.err().map(|error| error.code().to_owned()),
        Some("storage_publication_uncertain".to_owned())
    );
    let mut current = normal.open(&path, None).await?;
    let mut content = Vec::new();
    while let Some(chunk) = current.body.next().await {
        content.extend_from_slice(&chunk?);
    }
    assert_eq!(content, b"complete replacement");
    assert_eq!(normal.visibility(&path).await?, Visibility::Public);
    let client = aws_sdk_s3::Client::from_conf(
        aws_sdk_s3::Config::builder()
            .behavior_version_latest()
            .region(aws_sdk_s3::config::Region::new(region))
            .endpoint_url(endpoint)
            .force_path_style(true)
            .credentials_provider(aws_sdk_s3::config::Credentials::new(
                access,
                secret,
                None,
                None,
                "openrust-storage-test",
            ))
            .build(),
    );
    let staged = client
        .list_objects_v2()
        .bucket(&bucket)
        .prefix(format!("{root}/.staging/"))
        .send()
        .await?;
    assert_eq!(staged.contents(), &[] as &[aws_sdk_s3::types::Object]);
    normal.delete(&path).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "需单独启动已开启 bucket versioning 的真实 S3-compatible 服务后显式运行"]
#[allow(clippy::too_many_lines)]
async fn s3_streams_multipart_cancellation_and_exact_version_presigned_reads() -> TestResult {
    let endpoint = std::env::var("OPENRUST_TEST_S3_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:19000".to_owned());
    let versioned_bucket = std::env::var("OPENRUST_TEST_S3_VERSIONED_BUCKET")
        .unwrap_or_else(|_| "openrust-storage-versioned".to_owned());
    let plain_bucket = std::env::var("OPENRUST_TEST_S3_PLAIN_BUCKET")
        .unwrap_or_else(|_| "openrust-storage-plain".to_owned());
    let access_key =
        std::env::var("OPENRUST_TEST_S3_ACCESS_KEY").unwrap_or_else(|_| "openrust-test".to_owned());
    let secret_key = std::env::var("OPENRUST_TEST_S3_SECRET_KEY")
        .unwrap_or_else(|_| "openrust-test-secret".to_owned());
    let versioned = serde_json::to_string(&versioned_bucket)?;
    let plain = serde_json::to_string(&plain_bucket)?;
    let endpoint = serde_json::to_string(&endpoint)?;
    let access_key = serde_json::to_string(&access_key)?;
    let secret_key = serde_json::to_string(&secret_key)?;
    let figment = figment::Figment::new().merge(figment::providers::Toml::string(&format!(
        "[storage]\nmax_upload_bytes = 33554432\nmax_chunk_bytes = 1048576\ndefault_disk = 'versioned'\n[storage.disks.versioned]\ndriver = 's3'\nbucket = {versioned}\nregion = 'us-east-1'\nendpoint = {endpoint}\naccess_key_id = {access_key}\nsecret_access_key = {secret_key}\n[storage.disks.plain]\ndriver = 's3'\nbucket = {plain}\nregion = 'us-east-1'\nendpoint = {endpoint}\naccess_key_id = {access_key}\nsecret_access_key = {secret_key}\n"
    )));
    let mut builder = ProviderBuilder::new();
    builder.provider(StorageServiceProvider::new());
    let app = openrust::App::from_provider_runtime(builder.start(figment).await?)?;
    let storage = app.state().resolve::<Storage>().await?;
    let versioned = storage.disk("versioned").await?;
    let plain = storage.disk("plain").await?;
    let prefix = format!("acceptance/{}", unique_suffix());
    let object_path = StoragePath::new(format!("{prefix}/versioned.bin"))?;

    let first = versioned
        .put_stream(
            object_path.clone(),
            Some("application/octet-stream".into()),
            Box::pin(stream::iter([Ok(Bytes::from_static(b"first version"))])),
            UploadOptions::default(),
            None,
        )
        .await?;
    assert!(first.metadata.version.is_some());
    assert_eq!(first.metadata.size, u64::try_from(b"first version".len())?);
    let first_version = first.metadata.version.clone().ok_or_else(|| {
        Error::custom(
            "s3_version_missing",
            "versioned bucket did not return an object version",
        )
    })?;
    assert_eq!(
        first.metadata.content_type.as_deref(),
        Some("application/octet-stream")
    );
    let old_url = versioned
        .presign_read(&object_path, Duration::from_secs(60))
        .await?;
    let expiring_url = versioned
        .presign_read(&object_path, Duration::from_secs(1))
        .await?;

    let block = Bytes::from(vec![b'x'; 1024 * 1024]);
    let large_path = StoragePath::new(format!("{prefix}/multipart.bin"))?;
    let chunks = std::iter::repeat_n(block, 12).map(Ok::<Bytes, Error>);
    let large = versioned
        .put_stream(
            large_path.clone(),
            None,
            Box::pin(stream::iter(chunks)),
            UploadOptions::default(),
            None,
        )
        .await?;
    assert_eq!(large.metadata.size, 12 * 1024 * 1024);
    let mut large_reader = versioned.open(&large_path, None).await?;
    let mut total = 0_u64;
    while let Some(chunk) = large_reader.body.next().await {
        total += chunk?.len() as u64;
    }
    assert_eq!(total, 12 * 1024 * 1024);

    let rejected_path = StoragePath::new(format!("{prefix}/rejected.bin"))?;
    let rejection_block = Bytes::from(vec![b'r'; 1024 * 1024]);
    let rejection_chunks = std::iter::repeat_n(rejection_block, 12).map(Ok::<Bytes, Error>);
    assert!(
        versioned
            .put_stream(
                rejected_path.clone(),
                None,
                Box::pin(stream::iter(rejection_chunks)),
                UploadOptions::default(),
                Some(Arc::new(RejectAfter {
                    chunks: std::sync::atomic::AtomicUsize::new(0),
                })),
            )
            .await
            .is_err()
    );
    assert!(versioned.stat(&rejected_path).await.is_err());

    let cancelled_path = StoragePath::new(format!("{prefix}/cancelled.bin"))?;
    let slow = stream::unfold((), |()| async {
        tokio::time::sleep(Duration::from_millis(35)).await;
        Some((Ok(Bytes::from(vec![b'z'; 1024 * 1024])), ()))
    });
    let upload = tokio::spawn({
        let versioned = versioned.clone();
        let cancelled_path = cancelled_path.clone();
        async move {
            versioned
                .put_stream(
                    cancelled_path,
                    None,
                    Box::pin(slow) as StorageStream,
                    UploadOptions::default(),
                    None,
                )
                .await
        }
    });
    tokio::time::sleep(Duration::from_millis(270)).await;
    upload.abort();
    let _ = upload.await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(versioned.stat(&cancelled_path).await.is_err());

    versioned
        .put_stream(
            object_path.clone(),
            Some("application/octet-stream".into()),
            Box::pin(stream::iter([Ok(Bytes::from_static(b"second version"))])),
            UploadOptions::default(),
            None,
        )
        .await?;
    let mut historical = versioned.open(&object_path, Some(&first_version)).await?;
    assert_eq!(
        historical.metadata.version.as_deref(),
        Some(first_version.as_str())
    );
    let mut historical_body = Vec::new();
    while let Some(chunk) = historical.body.next().await {
        historical_body.extend_from_slice(&chunk?);
    }
    assert_eq!(historical_body, b"first version");
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .get(old_url)
        .send()
        .await?;
    assert!(response.status().is_success());
    assert_eq!(response.bytes().await?.as_ref(), b"first version");
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let expired = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .get(expiring_url)
        .send()
        .await?;
    assert!(!expired.status().is_success());

    let no_version_path = StoragePath::new(format!("{prefix}/unversioned.bin"))?;
    plain
        .put_stream(
            no_version_path.clone(),
            None,
            Box::pin(stream::iter([Ok(Bytes::from_static(b"plain bucket"))])),
            UploadOptions::default(),
            None,
        )
        .await?;
    assert!(
        plain
            .presign_read(&no_version_path, Duration::from_secs(60))
            .await
            .is_err()
    );

    for path in [
        object_path,
        large_path,
        rejected_path,
        cancelled_path,
        no_version_path,
    ] {
        let disk = if path.as_str().ends_with("unversioned.bin") {
            &plain
        } else {
            &versioned
        };
        disk.delete(&path).await?;
    }
    Ok(())
}

struct RejectAfter {
    chunks: std::sync::atomic::AtomicUsize,
}

#[async_trait::async_trait]
impl UploadInspector for RejectAfter {
    async fn inspect_chunk(
        &self,
        _context: &openrust::storage::UploadContext,
        _chunk: &[u8],
    ) -> Result<()> {
        if self
            .chunks
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
            == 11
        {
            Err(Error::Forbidden("验收 inspector 拒绝内容".into()))
        } else {
            Ok(())
        }
    }
}

fn unique_suffix() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
        .to_string()
}
