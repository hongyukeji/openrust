use std::collections::HashSet;

use ::http::{
    HeaderValue,
    header::{CACHE_CONTROL, CONTENT_SECURITY_POLICY},
};
use futures_util::TryStreamExt as _;
use serde_json::Value;
use tokio_util::io::StreamReader;

use crate::{
    contracts::{
        config::Repository,
        routing::{Handler, UrlGenerator as _},
    },
    http::{HeaderMap, HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{Router, RoutingError, SignatureMode, SignatureQueryFilter, UrlError},
    support::facades::{Storage, URL},
};

use super::{FilesystemError, filesystem_adapter::validate_path};

pub(crate) fn register(router: &mut Router, config: &dyn Repository) -> Result<(), RoutingError> {
    let Some(Value::Object(disks)) = config.get("filesystems.disks") else {
        return Ok(());
    };
    let production = config
        .get("app.env")
        .as_ref()
        .and_then(Value::as_str)
        .unwrap_or("production")
        == "production";
    let mut served = HashSet::new();
    for (disk, settings) in disks {
        if settings.get("driver").and_then(Value::as_str) != Some("local")
            || settings.get("serve").and_then(Value::as_bool) != Some(true)
        {
            continue;
        }
        let configured_url = settings
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or("/storage");
        let url = configured_url
            .parse::<::http::Uri>()
            .map_err(|_| RoutingError::InvalidUri {
                uri: configured_url.into(),
            })?;
        let prefix = url.path().trim_end_matches('/');
        if !prefix.is_empty() && (!prefix.starts_with('/') || prefix.starts_with("//"))
            || url.query().is_some()
            || configured_url.contains('#')
        {
            return Err(RoutingError::InvalidUri {
                uri: configured_url.into(),
            });
        }
        if !served.insert(prefix.to_owned()) {
            return Err(RoutingError::ConflictingStorageRoute { uri: prefix.into() });
        }
        let uri = format!("{prefix}/{{path}}");
        let name = format!("storage.{disk}");
        let public = settings.get("visibility").and_then(Value::as_str) == Some("public");
        let download = SignedFile {
            disk: disk.clone(),
            upload: false,
            public,
            production,
        };
        let upload = SignedFile {
            disk,
            upload: true,
            public: false,
            production,
        };
        router.storage_route(&uri, &name, false, download)?;
        router.storage_route(&uri, &format!("{name}.upload"), true, upload)?;
    }
    Ok(())
}

struct SignedFile {
    disk: String,
    upload: bool,
    public: bool,
    production: bool,
}

impl SignedFile {
    fn authorized(&self, request: &Request) -> Result<bool, HttpError> {
        let upload = request
            .query(Some("upload"))?
            .is_some_and(|value| match value {
                Value::Bool(value) => value,
                Value::String(value) => matches!(
                    value.to_ascii_lowercase().as_str(),
                    "1" | "true" | "on" | "yes"
                ),
                _ => false,
            });
        if upload != self.upload {
            return Ok(false);
        }
        if self.public {
            return Ok(true);
        }
        match URL::service()?.hasValidSignature(
            request,
            SignatureMode::Relative,
            SignatureQueryFilter::Keys(&[]),
        ) {
            Err(UrlError::MissingSigningKey) => Ok(false),
            valid => Ok(valid?),
        }
    }
}

impl Handler for SignedFile {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if !self.authorized(request)? {
                return Err(if self.production {
                    HttpError::NotFound
                } else {
                    HttpError::InvalidSignature
                });
            }
            let path = request.route("path").ok_or(HttpError::NotFound)?;
            validate_path(path).map_err(|_| HttpError::NotFound)?;
            let disk = Storage::disk(Some(&self.disk))?;
            if self.upload {
                let path = path.to_owned();
                let mut body = StreamReader::new(
                    request
                        .take_body()
                        .into_data_stream()
                        .map_err(std::io::Error::other),
                );
                if !disk.writeStream(&path, &mut body).await? {
                    return Err(FilesystemError::from(std::io::Error::other(
                        "could not store uploaded file",
                    ))
                    .into());
                }
                Ok(Response::empty(StatusCode::NO_CONTENT))
            } else {
                let mut headers = HeaderMap::new();
                headers.insert(
                    CACHE_CONTROL,
                    HeaderValue::from_static("no-store, no-cache, must-revalidate, max-age=0"),
                );
                headers.insert(
                    CONTENT_SECURITY_POLICY,
                    HeaderValue::from_static(
                        "default-src 'none'; style-src 'unsafe-inline'; sandbox",
                    ),
                );
                Ok(disk.response(path, None, headers).await?)
            }
        })
    }
}
