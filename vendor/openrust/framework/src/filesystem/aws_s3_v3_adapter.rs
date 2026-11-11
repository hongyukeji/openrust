use std::time::Duration;

use aws_sdk_s3::{
    Client,
    config::{
        BehaviorVersion, Credentials, Region, RequestChecksumCalculation,
        StalledStreamProtectionConfig,
        endpoint::{DefaultResolver, Params, ResolveEndpoint as _},
        timeout::TimeoutConfig,
    },
    error::{ProvideErrorMetadata, SdkError},
    primitives::ByteStream,
    types::{
        CompletedMultipartUpload, CompletedPart, Grant, ObjectCannedAcl, ObjectOwnership, Owner,
    },
};
use futures_util::FutureExt as _;
use serde::Deserialize;
use tokio::{
    io::{AsyncRead, AsyncReadExt as _},
    sync::OnceCell,
};
use url::Url;

use super::{
    FilesystemError, FilesystemReader, FilesystemResult, Visibility, download::OpenedFile,
};

const PART_SIZE: usize = 8 * 1024 * 1024;
const MAX_PARTS: i32 = 10_000;

/// Configuration for an on-demand S3 disk.
#[derive(Clone, Deserialize)]
pub struct S3DiskConfig {
    pub bucket: String,
    pub region: String,
    pub key: Option<String>,
    pub secret: Option<String>,
    pub token: Option<String>,
    pub endpoint: Option<String>,
    #[serde(default)]
    pub root: String,
    #[serde(default)]
    pub use_path_style_endpoint: bool,
    pub visibility: Option<Visibility>,
    #[serde(default)]
    pub throw: bool,
    #[serde(default)]
    pub report: bool,
}

/// Provides Amazon S3 and S3-compatible filesystem operations.
pub struct AwsS3V3Adapter {
    config: S3DiskConfig,
    client: OnceCell<Client>,
}

impl AwsS3V3Adapter {
    pub(super) fn new(mut config: S3DiskConfig) -> FilesystemResult<Self> {
        let endpoint = config
            .endpoint
            .as_deref()
            .map(Url::parse)
            .transpose()
            .map_err(|_| FilesystemError::InvalidConfiguration)?;
        if config.bucket.trim().is_empty()
            || config.region.trim().is_empty()
            || config.key.is_some() != config.secret.is_some()
            || config.token.is_some() && config.key.is_none()
            || [&config.key, &config.secret, &config.token]
                .into_iter()
                .flatten()
                .any(|value| value.trim().is_empty())
            || endpoint.as_ref().is_some_and(|url| {
                !matches!(url.scheme(), "http" | "https")
                    || url.host_str().is_none()
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || url.query().is_some()
                    || url.fragment().is_some()
            })
        {
            return Err(FilesystemError::InvalidConfiguration);
        }

        config.root = config.root.trim_matches('/').to_owned();
        if !config.root.is_empty() {
            super::filesystem_adapter::validate_path(&config.root)
                .map_err(|_| FilesystemError::InvalidConfiguration)?;
        }
        Ok(Self {
            config,
            client: OnceCell::new(),
        })
    }

    async fn client(&self) -> &Client {
        self.client
            .get_or_init(|| async {
                let region = Region::new(self.config.region.clone());
                let version = BehaviorVersion::latest();
                let mut builder = match (&self.config.key, &self.config.secret) {
                    (Some(key), Some(secret)) => aws_sdk_s3::config::Builder::new()
                        .behavior_version(version)
                        .region(region)
                        .credentials_provider(Credentials::new(
                            key,
                            secret,
                            self.config.token.clone(),
                            None,
                            "filesystem",
                        )),
                    _ => {
                        let shared = aws_config::defaults(version).region(region).load().await;
                        aws_sdk_s3::config::Builder::from(&shared)
                    }
                };
                if let Some(endpoint) = &self.config.endpoint {
                    builder = builder.endpoint_url(endpoint.as_str());
                }
                Client::from_conf(
                    builder
                        .force_path_style(self.config.use_path_style_endpoint)
                        .request_checksum_calculation(RequestChecksumCalculation::WhenRequired)
                        .stalled_stream_protection(StalledStreamProtectionConfig::enabled().build())
                        .timeout_config(
                            TimeoutConfig::builder()
                                .connect_timeout(Duration::from_secs(5))
                                .operation_attempt_timeout(Duration::from_secs(45))
                                .operation_timeout(Duration::from_secs(90))
                                .build(),
                        )
                        .build(),
                )
            })
            .await
    }

    pub(super) fn key(&self, path: &str) -> String {
        if self.config.root.is_empty() {
            path.to_owned()
        } else {
            format!("{}/{path}", self.config.root)
        }
    }

    pub(super) async fn read(&self, path: &str) -> FilesystemResult<FilesystemReader> {
        Ok(self.open(path).await?.reader)
    }

    pub(super) async fn open(&self, path: &str) -> FilesystemResult<OpenedFile> {
        let result = self
            .client()
            .await
            .get_object()
            .bucket(&self.config.bucket)
            .key(self.key(path))
            .send()
            .await
            .map_err(|error| classify_error(&error))?;
        Ok(OpenedFile {
            length: result
                .content_length
                .and_then(|size| u64::try_from(size).ok()),
            content_type: result.content_type,
            reader: Box::new(result.body.into_async_read()),
        })
    }

    pub(super) fn url(&self, path: &str, base: Option<&str>) -> FilesystemResult<String> {
        use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};

        const ENCODE: &percent_encoding::AsciiSet = &NON_ALPHANUMERIC
            .remove(b'-')
            .remove(b'_')
            .remove(b'.')
            .remove(b'~')
            .remove(b'/');
        let key = self.key(path);
        let key = utf8_percent_encode(&key, ENCODE);
        if let Some(base) = base {
            return Ok(format!("{}/{key}", base.trim_end_matches('/')));
        }
        let params = Params::builder()
            .bucket(&self.config.bucket)
            .region(&self.config.region)
            .force_path_style(self.config.use_path_style_endpoint)
            .set_endpoint(self.config.endpoint.clone())
            .build()
            .map_err(|_| FilesystemError::InvalidConfiguration)?;
        let endpoint = DefaultResolver::new()
            .resolve_endpoint(&params)
            .now_or_never()
            .ok_or(FilesystemError::InvalidConfiguration)?
            .map_err(|source| FilesystemError::Backend { source })?;
        Ok(format!("{}/{key}", endpoint.url().trim_end_matches('/')))
    }

    pub(super) async fn file_exists(&self, path: &str) -> FilesystemResult<bool> {
        match self
            .client()
            .await
            .head_object()
            .bucket(&self.config.bucket)
            .key(self.key(path))
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(error) => match classify_error(&error) {
                FilesystemError::NotFound => Ok(false),
                error => Err(error),
            },
        }
    }

    pub(super) async fn directory_exists(&self, path: &str) -> FilesystemResult<bool> {
        let prefix = format!("{}/", self.key(path));
        let result = self
            .client()
            .await
            .list_objects_v2()
            .bucket(&self.config.bucket)
            .prefix(&prefix)
            .max_keys(1)
            .send()
            .await
            .map_err(|error| classify_error(&error))?;
        Ok(!result.contents().is_empty())
    }

    pub(super) async fn delete(&self, path: &str) -> FilesystemResult<()> {
        self.client()
            .await
            .delete_object()
            .bucket(&self.config.bucket)
            .key(self.key(path))
            .send()
            .await
            .map_err(|error| classify_error(&error))?;
        Ok(())
    }

    pub(super) async fn get_visibility(&self, path: &str) -> FilesystemResult<Visibility> {
        let result = self
            .client()
            .await
            .get_object_acl()
            .bucket(&self.config.bucket)
            .key(self.key(path))
            .send()
            .await
            .map_err(|error| classify_error(&error))?;
        classify_acl(result.owner(), result.grants())
    }

    async fn ensure_acl_writable(&self, client: &Client) -> FilesystemResult<()> {
        // Some compatible servers return success for PutObjectAcl on enforced buckets.
        // Use the ownership policy when available, while allowing least-privilege clients
        // that can write an object ACL but cannot read the bucket's ownership controls.
        match client
            .get_bucket_ownership_controls()
            .bucket(&self.config.bucket)
            .send()
            .await
        {
            Ok(response)
                if response.ownership_controls().is_some_and(|controls| {
                    controls.rules().iter().any(|rule| {
                        rule.object_ownership() == &ObjectOwnership::BucketOwnerEnforced
                    })
                }) =>
            {
                Err(FilesystemError::UnsupportedOperation)
            }
            Ok(_) => Ok(()),
            Err(error) => match classify_error(&error) {
                FilesystemError::NotFound | FilesystemError::UnsupportedOperation => Ok(()),
                _ if error
                    .raw_response()
                    .is_some_and(|response| response.status().as_u16() == 403) =>
                {
                    Ok(())
                }
                error => Err(error),
            },
        }
    }

    pub(super) async fn set_visibility(
        &self,
        path: &str,
        visibility: Visibility,
    ) -> FilesystemResult<()> {
        let client = self.client().await;
        self.ensure_acl_writable(client).await?;
        client
            .put_object_acl()
            .bucket(&self.config.bucket)
            .key(self.key(path))
            .acl(canned_acl(visibility))
            .send()
            .await
            .map_err(|error| classify_error(&error))?;
        if self.get_visibility(path).await? != visibility {
            return Err(FilesystemError::UnsupportedOperation);
        }
        Ok(())
    }

    pub(super) async fn write(
        &self,
        path: &str,
        reader: &mut (impl AsyncRead + Unpin),
        visibility: Option<Visibility>,
    ) -> FilesystemResult<()> {
        let visibility = visibility.or(self.config.visibility);
        let key = self.key(path);
        let client = self.client().await;
        if visibility.is_some() {
            self.ensure_acl_writable(client).await?;
        }
        let first = read_part(reader).await?;
        if first.len() < PART_SIZE {
            client
                .put_object()
                .bucket(&self.config.bucket)
                .key(&key)
                .set_acl(visibility.map(canned_acl))
                .body(ByteStream::from(first))
                .send()
                .await
                .map_err(|error| publish_error(&error))?;
        } else {
            let upload = client
                .create_multipart_upload()
                .bucket(&self.config.bucket)
                .key(&key)
                .set_acl(visibility.map(canned_acl))
                .send()
                .await
                .map_err(|error| classify_error(&error))?;
            let id = upload
                .upload_id()
                .filter(|id| !id.is_empty())
                .ok_or_else(invalid_response)?;
            let mut upload = Multipart::new(
                client.clone(),
                self.config.bucket.clone(),
                key,
                id.to_owned(),
            );
            if let Err(error) = upload.write(first, reader).await {
                upload.abort().await;
                return Err(error);
            }
            upload.id = None;
        }

        if let Some(expected) = visibility {
            // Some compatible servers accept canned ACL headers without applying them.
            if !matches!(self.get_visibility(path).await, Ok(actual) if actual == expected) {
                return Err(FilesystemError::PublishOutcomeUnknown);
            }
        }
        Ok(())
    }
}

async fn read_part(reader: &mut (impl AsyncRead + Unpin)) -> FilesystemResult<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(PART_SIZE as u64)
        .read_to_end(&mut bytes)
        .await?;
    Ok(bytes)
}

struct Multipart {
    client: Client,
    bucket: String,
    key: String,
    id: Option<String>,
}

impl Multipart {
    fn new(client: Client, bucket: String, key: String, id: String) -> Self {
        Self {
            client,
            bucket,
            key,
            id: Some(id),
        }
    }

    async fn write(
        &self,
        mut part: Vec<u8>,
        reader: &mut (impl AsyncRead + Unpin),
    ) -> FilesystemResult<()> {
        let id = self.id.as_deref().expect("active upload");
        let mut completed = Vec::new();
        for part_number in 1..=MAX_PARTS {
            if part.is_empty() {
                break;
            }
            let uploaded = self
                .client
                .upload_part()
                .bucket(&self.bucket)
                .key(&self.key)
                .upload_id(id)
                .part_number(part_number)
                .body(ByteStream::from(part))
                .send()
                .await
                .map_err(|error| classify_error(&error))?;
            let etag = uploaded
                .e_tag()
                .filter(|etag| !etag.is_empty())
                .ok_or_else(invalid_response)?;
            completed.push(
                CompletedPart::builder()
                    .part_number(part_number)
                    .e_tag(etag)
                    .build(),
            );
            part = read_part(reader).await?;
        }
        if !part.is_empty() {
            return Err(FilesystemError::UnsupportedOperation);
        }
        let parts = CompletedMultipartUpload::builder()
            .set_parts(Some(completed))
            .build();
        self.client
            .complete_multipart_upload()
            .bucket(&self.bucket)
            .key(&self.key)
            .upload_id(id)
            .multipart_upload(parts)
            .send()
            .await
            .map_err(|error| publish_error(&error))?;
        Ok(())
    }

    async fn abort(&mut self) {
        if let Some(id) = &self.id {
            abort_request(
                self.client.clone(),
                self.bucket.clone(),
                self.key.clone(),
                id.clone(),
            )
            .await;
            self.id = None;
        }
    }
}

impl Drop for Multipart {
    fn drop(&mut self) {
        if let Some(id) = self.id.take()
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            runtime.spawn(abort_request(
                self.client.clone(),
                self.bucket.clone(),
                self.key.clone(),
                id,
            ));
        }
    }
}

async fn abort_request(client: Client, bucket: String, key: String, id: String) {
    let result = client
        .abort_multipart_upload()
        .bucket(bucket)
        .key(key)
        .upload_id(id)
        .send()
        .await;
    if let Err(error) = result
        && !matches!(classify_error(&error), FilesystemError::NotFound)
    {
        let _ = crate::log::Log::warning("S3 multipart upload cleanup failed", &Default::default());
    }
}

fn canned_acl(visibility: Visibility) -> ObjectCannedAcl {
    match visibility {
        Visibility::Public => ObjectCannedAcl::PublicRead,
        Visibility::Private => ObjectCannedAcl::Private,
    }
}

fn classify_acl(owner: Option<&Owner>, grants: &[Grant]) -> FilesystemResult<Visibility> {
    if owner.and_then(Owner::id).is_none_or(str::is_empty) || grants.is_empty() {
        return Err(invalid_response());
    }
    let mut public = false;
    for grant in grants {
        let grantee = grant.grantee().ok_or_else(invalid_response)?;
        let permission = grant.permission().ok_or_else(invalid_response)?.as_str();
        if !matches!(
            permission,
            "READ" | "WRITE" | "READ_ACP" | "WRITE_ACP" | "FULL_CONTROL"
        ) {
            return Err(invalid_response());
        }
        match grantee.r#type().as_str() {
            "Group" => match grantee.uri() {
                Some("http://acs.amazonaws.com/groups/global/AllUsers") => {
                    public |= matches!(permission, "READ" | "FULL_CONTROL");
                }
                Some(
                    "http://acs.amazonaws.com/groups/global/AuthenticatedUsers"
                    | "http://acs.amazonaws.com/groups/s3/LogDelivery",
                ) => {}
                _ => return Err(invalid_response()),
            },
            "CanonicalUser" if grantee.id().is_some_and(|id| !id.is_empty()) => {}
            "AmazonCustomerByEmail"
                if grantee
                    .email_address()
                    .is_some_and(|email| !email.is_empty()) => {}
            _ => return Err(invalid_response()),
        }
    }
    Ok(if public {
        Visibility::Public
    } else {
        Visibility::Private
    })
}

fn classify_error<E: ProvideErrorMetadata>(error: &SdkError<E>) -> FilesystemError {
    let code = error
        .as_service_error()
        .and_then(ProvideErrorMetadata::code);
    match code {
        Some("NoSuchKey" | "NotFound" | "NoSuchUpload") => FilesystemError::NotFound,
        Some("AccessControlListNotSupported" | "NotImplemented" | "MethodNotAllowed") => {
            FilesystemError::UnsupportedOperation
        }
        Some("NoSuchBucket") => FilesystemError::InvalidConfiguration,
        _ if error
            .raw_response()
            .is_some_and(|response| response.status().as_u16() == 404) =>
        {
            FilesystemError::NotFound
        }
        _ => {
            let status = error
                .raw_response()
                .map(|response| response.status().as_u16());
            let message = status.map_or_else(
                || "S3 request failed before a response was available".to_owned(),
                |status| format!("S3 request returned HTTP {status}"),
            );
            FilesystemError::Backend {
                source: Box::new(std::io::Error::other(message)),
            }
        }
    }
}

fn publish_error<E: ProvideErrorMetadata>(error: &SdkError<E>) -> FilesystemError {
    if matches!(
        error,
        SdkError::TimeoutError(_) | SdkError::DispatchFailure(_) | SdkError::ResponseError(_)
    ) || error
        .raw_response()
        .is_some_and(|response| response.status().as_u16() >= 500)
        || error
            .as_service_error()
            .and_then(ProvideErrorMetadata::code)
            == Some("NoSuchUpload")
    {
        FilesystemError::PublishOutcomeUnknown
    } else {
        classify_error(error)
    }
}

fn invalid_response() -> FilesystemError {
    FilesystemError::Backend {
        source: Box::new(std::io::Error::other("invalid S3 response")),
    }
}
