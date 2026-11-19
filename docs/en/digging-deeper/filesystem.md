# File Storage

## Configuration

Configure the default disk with `FILESYSTEM_DISK` and named disks in `config/filesystems.rs`. The `local` and `public` disks both use the local driver. Each disk has its own root. The manager resolves and caches named disks within the current application.

### The Local Driver

The default `local` disk stores files under `storage/app/private`. Set `FILESYSTEM_LOCAL_ROOT` to override this directory; relative paths are resolved from the application root, and absolute paths are used directly. When unset, the default follows the storage root configured by `APP_STORAGE_PATH`. Use paths relative to the disk root. Absolute paths, traversal components and reserved temporary names are rejected. On Unix the local driver opens files relative to directory handles, rejects symbolic links during traversal and refuses to read files with multiple hard links.

### The Public Disk

The `public` disk is configured in `config/filesystems.rs`. Its default local root is `storage/app/public`, and its URL is the application URL followed by `/storage`. Set `FILESYSTEM_PUBLIC_ROOT` to change the directory or `FILESYSTEM_PUBLIC_URL` to change the public URL. Relative filesystem paths are resolved from the application root.

Create or remove the links configured in `filesystems.links` with Artisan. The default link is `public/storage`, pointing to the public disk directory. Use `--force` to recreate an existing symbolic link. Existing ordinary files and directories are preserved.

```sh
./artisan storage:link --relative
./artisan storage:unlink
```

#### Serving local files (OpenRust extension)

The default `routes/web.rs` contains these mounts, in this order:

```rust
Route::staticDisk("web")?;
Route::staticDisk("public")?;
```

The `web` disk describes the website directory, with its root defaulting to `public` and its URL to `app.url`. Set `APP_PUBLIC_PATH` and `FILESYSTEM_WEB_URL` to override those defaults in `config/filesystems.rs`. The application uses `web.root` for `publicPath()`; calling `usePublicPath()` before boot overrides the directory and updates `web.root`. Both mounts use `staticDisk`, which reads the local disk root and derives the complete mount path from its `url`. For example, `FILESYSTEM_PUBLIC_URL=/storage/tmp/` exposes `storage/app/public/photo.jpg` at `/storage/tmp/photo.jpg`. Absolute URLs must have the same origin as `app.url`. A different scheme, host or effective port fails HTTP bootstrap.

Both disk mounts require public files and traversable directories on Unix, and reject symbolic links, temporary files and files with multiple hard links. If `public/storage` is a symbolic link, the `web` disk skips it and the `public` disk serves files directly from its configured root.

Application routes take precedence over static files. Static mounts are searched in declaration order and fall through when a file is missing. With the default order, `public/storage/photo.jpg` wins over `storage/app/public/photo.jpg` at `/storage/photo.jpg`. The server supports GET and HEAD, MIME types, `Last-Modified` and `If-Modified-Since`. It does not list directories, and rejects traversal and symlinks escaping the mount root. Missing files use the application's normal 404 policy.

See [routing](../the-basics/routing.md#mounting-a-static-directory) and [public file configuration](../getting-started/configuration.md#public-files).

### Amazon S3 Compatible Filesystems

Set `AWS_BUCKET` and `AWS_DEFAULT_REGION` for the `s3` disk. Provide `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` together, with `AWS_SESSION_TOKEN` for temporary credentials. When both keys are omitted, the AWS SDK resolves credentials through its provider chain. Set `AWS_ENDPOINT` for a compatible service and enable `AWS_USE_PATH_STYLE_ENDPOINT` when the service uses path-style bucket URLs.

Select the disk by name from an application request or command. Set `FILESYSTEM_DISK=s3` to use it as the default. An optional `root` in the disk configuration prefixes object keys. Object paths remain relative to the disk.

```rust
use openrust::{filesystem::FilesystemResult, support::facades::Storage};

pub async fn store_report() -> FilesystemResult<Option<Vec<u8>>> {
    let disk = Storage::disk(Some("s3"))?;
    if !disk.put("reports/summary.txt", b"Ready").await? {
        return Ok(None);
    }
    disk.get("reports/summary.txt").await
}
```

`readStream` returns an async reader shared by local and S3 disks. S3 writes stream in 8 MiB parts and publish when multipart upload completes. The driver can upload up to 10,000 parts, about 78 GiB per object. Interrupted uploads are aborted when possible. If the server may have completed a write after the connection failed, `throw = true` exposes `PublishOutcomeUnknown`. Read the target to check its state before retrying.

## Obtaining Disk Instances

Call `Storage::disk(Some("public"))` to select a disk from the current application. Call `Storage::disk(None)` or use `Storage::put` and `Storage::get` to work with the default disk. This example runs within a request or command scope.

```rust
use openrust::{filesystem::FilesystemResult, support::facades::Storage};

pub async fn store_note() -> FilesystemResult<Option<Vec<u8>>> {
    if !Storage::put("notes/example.txt", b"Hello").await? {
        return Ok(None);
    }
    let disk = Storage::disk(Some("local"))?;
    disk.get("notes/example.txt").await
}
```

Use `Storage::build(LocalDiskConfig)` for an on-demand local disk. The named disk cache belongs to one application.

## Retrieving Files

`get` returns file contents. Use `readStream` for an asynchronous reader when reading large files. The default `throw = false` returns `None` for a failed read or `false` for a failed write. Set `throw = true` to return the error. Set `report = true` to log failures handled by the default policy. Invalid paths return an error in either mode.

### Downloading Files

Use `download` for an attachment or `response` for inline display. Both stream the file and accept a download name and response headers. The adapter supplies content type, length when known, and disposition. Authorize the request before calling these methods for private files. A missing file converts to HTTP 404 when the handler returns `HttpError`.

```rust
use openrust::{
    http::{HeaderMap, HttpError, Response},
    support::facades::Storage,
};

pub async fn download_report() -> Result<Response, HttpError> {
    Ok(Storage::disk(Some("public"))?
        .download("reports/summary.txt", Some("summary.txt"), HeaderMap::new())
        .await?)
}
```

### File URLs

`Storage::url` uses the default disk. A named disk uses its configured `url` when present. A local disk without a configured URL uses `/storage`. Use URL-safe names for local URLs. For S3, set `AWS_URL` for a custom public base. With no custom base, the AWS SDK resolves the bucket endpoint, and the adapter appends the encoded object key including its configured prefix. Generating a URL leaves access control to the disk mount or bucket.

### File Metadata {#file-metadata}

Use `size` to get the byte count and `lastModified` to get a `SystemTime` for a file on a local disk. Both methods return errors when metadata is unavailable. The same methods on `Storage` use the default disk.

```rust
use openrust::filesystem::{FilesystemAdapter, FilesystemResult};

async fn report_metadata(disk: &FilesystemAdapter) -> FilesystemResult<(u64, Option<String>)> {
    let path = "reports/today.txt";
    Ok((disk.size(path).await?, disk.mimeType(path).await?))
}
```

Use `mimeType` to check content signatures, with a filename extension or `application/octet-stream` as fallback. Use `checksum` for a streamed MD5 digest, or `checksumWith::<D>` to select a Rust digest. For these two operations, disk failures return `None` when `throw` is disabled. Invalid paths always return an error.

## Storing Files

`writeStream` copies from an asynchronous reader. The local driver writes a sibling temporary file and publishes it by rename after the stream completes. A source failure or cancellation before publication preserves an existing file. A cancellation after the rename has been dispatched can still publish the completed write.

### File Uploads

Get an upload from a multipart request with `Request::file`. Use `store` and `storeAs` on the uploaded file, or `putFile` and `putFileAs` on the disk. `storePublicly` and `storePubliclyAs` request public visibility. Generated names use 40 random characters and a content-derived extension when recognized. The client filename and media type are metadata. Apply your application’s upload validation before saving.

```rust
use openrust::{
    http::{HttpError, Request, Response, StatusCode},
    support::facades::Storage,
};

pub async fn store_attachment(request: &mut Request) -> Result<Response, HttpError> {
    let attachment = request.file("attachment").await?.ok_or(HttpError::InvalidRequest)?;
    let disk = Storage::disk(Some("public"))?;
    let path = attachment.storePublicly("attachments", Some("public")).await?
        .ok_or_else(|| HttpError::Internal {
            source: std::io::Error::other("Could not store attachment").into(),
        })?;
    Response::json(StatusCode::CREATED, disk.url(&path)?)
}
```

### File Visibility

On Unix, the public and private file modes are `0644` and `0600`. New subdirectories use the disk’s configured directory visibility. Override the visibility of a single write with `WriteOptions`. Use `getVisibility` and `setVisibility` on an existing file. Windows writes use the filesystem ACL. Explicit visibility changes are currently supported on Unix.

S3 visibility uses object ACLs. If no visibility is specified, the write leaves access control to the bucket. An explicit visibility request requires an ACL-capable bucket, and confirmation requires reading the ACL back. Bucket policies and the provider’s ACL enforcement determine whether an object is actually accessible anonymously.

#### Custom Unix Permissions {#local-permissions}

A named local disk can override the Unix mode for each visibility. Set `permissions.file.public`, `permissions.file.private`, `permissions.dir.public`, or `permissions.dir.private` in its configuration. Unspecified modes use the defaults. Use octal numbers such as `0o664` in Rust config.

```rust
use openrust::filesystem::{FilesystemResult, LocalPermissions};

fn permissions() -> FilesystemResult<LocalPermissions> {
    LocalPermissions::new(0o664, 0o640, 0o775, 0o750)
}
```

For an on-demand disk, give these permissions to `Storage::buildWithPermissions` along with its `LocalDiskConfig`. Modes apply to newly created files and directories. `setVisibility` applies the configured file mode. Public disk mounts also require Unix public-read and directory-traverse permissions and respect modes mapped to `private`. Custom Unix modes are supported on Unix.

## Temporary file URLs

Use `disk.temporaryUrl(path, expiration).await?` for a download or `disk.temporaryUploadUrl(path, expiration).await?` for an upload. Send the returned `headers` with the upload `url`. For a local disk, enable `serve` in the disk configuration. Its route uses the path in the disk's `url`, or `/storage` when no URL is set. The local upload link accepts PUT. The S3 adapter signs against the configured bucket endpoint. The client needs network access to that endpoint.

### S3 temporary URL origin {#s3-temporary-url-origin}

Set `temporary_url` on the S3 disk, or `AWS_TEMPORARY_URL` for the default S3 configuration, to replace the scheme, host and port in generated GET and PUT links. The signed path and query stay intact. Configure the proxy to forward the original signed Host, path and query to the S3 endpoint.

### S3 request options {#s3-request-parameters}

Use `temporaryUrlWith` to sign download response headers or a version ID. Use `temporaryUploadUrlWith` to sign upload content headers and metadata. Send the returned headers with the PUT request. The disk supplies the bucket and object path.

```rust
use std::time::{Duration, SystemTime};

use openrust::{
    filesystem::{FilesystemResult, TemporaryUrlOptions},
    support::facades::Storage,
};

async fn report_url() -> FilesystemResult<String> {
    Storage::disk(Some("s3"))?
        .temporaryUrlWith(
            "reports/current.csv",
            SystemTime::now() + Duration::from_secs(300),
            TemporaryUrlOptions {
                response_content_type: Some("text/csv".into()),
                response_content_disposition: Some("attachment; filename=report.csv".into()),
                ..Default::default()
            },
        )
        .await
}
```

For advanced S3 options, call `temporaryDownloadRequest` or `temporaryUploadRequest` on the named disk and configure the AWS SDK request builder. Both methods return the URL and signed request headers. Send those headers with the request. The disk sets the bucket and prefixed key after applying the builder options.

```rust
use std::time::{Duration, SystemTime};

use openrust::{
    filesystem::{FilesystemResult, TemporaryDownloadUrl},
    support::facades::Storage,
};

async fn report_preview() -> FilesystemResult<TemporaryDownloadUrl> {
    Storage::disk(Some("s3"))?
        .temporaryDownloadRequest(
            "reports/current.csv",
            SystemTime::now() + Duration::from_secs(300),
            |request| request.range("bytes=0-1023"),
        )
        .await
}
```

### Custom local disk URL builders {#customizing-temporary-urls}

Register `buildTemporaryUrlsUsing` or `buildTemporaryUploadUrlsUsing` on the local disk during provider boot. The callback receives the validated path, expiration and typed options. The upload callback returns a `TemporaryUploadUrl` with the URL and required headers.

## Directories {#directories}

Use `files` and `directories` to list direct children on a local disk. Use `allFiles` and `allDirectories` to include descendants. Pass `None` for the disk root. Results are disk-relative paths with `/` separators. File lists are sorted by path, and a missing directory returns an empty list.

```rust
use openrust::filesystem::{FilesystemAdapter, FilesystemResult};

async fn daily_reports(disk: &FilesystemAdapter) -> FilesystemResult<Vec<String>> {
    disk.makeDirectory("reports/daily").await?;
    disk.allFiles(Some("reports")).await
}
```

Use `makeDirectory` to create missing parents with the disk’s directory visibility, and `deleteDirectory` to remove a directory and its contents. Listing skips symbolic links and temporary writes. Directory creation and deletion require a named relative path. The same methods are available on `Storage` for the default disk.
