# File Storage

## Configuration

Configure the default disk with `FILESYSTEM_DISK` and named disks in `config/filesystems.rs`. The `local` and `public` disks both use the local driver. Each disk has its own root. The manager resolves and caches named disks within the current application.

### The Local Driver

The default `local` disk stores files under `storage/app/private`. Set `FILESYSTEM_LOCAL_ROOT` to override this directory; relative paths are resolved from the application root, and absolute paths are used directly. When unset, the default follows the storage root configured by `APP_STORAGE_PATH`. Use paths relative to the disk root. Absolute paths, traversal components and reserved temporary names are rejected. On Unix the local driver opens files relative to directory handles, rejects symbolic links during traversal and refuses to read files with multiple hard links. Read and write operations collapse repeated interior `/` separators: `avatars///photo.png` and `avatars/photo.png` address the same file or object key. The `path`, `url`, and temporary URL builders preserve the supplied path text when adding prefixes or signatures.

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
Route::static_disk("web")?;
Route::static_disk("public")?;
```

The `web` disk describes the website directory, with its root defaulting to `public` and its URL to `app.url`. Set `FILESYSTEM_WEB_ROOT` and `FILESYSTEM_WEB_URL` to override those defaults in `config/filesystems.rs`. `FILESYSTEM_WEB_ROOT` overrides the website disk root; when unset, it uses `APP_PUBLIC_PATH` (default `public`). Relative paths are resolved from the application root; absolute paths are used directly. Empty or whitespace-only values fail configuration loading. The application uses `web.root` for `public_path()`; calling `use_public_path()` before boot overrides the directory and updates `web.root`. Both mounts use `static_disk`, which reads the local disk root and derives the complete mount path from its `url`. For example, `FILESYSTEM_PUBLIC_URL=/storage/tmp/` exposes `storage/app/public/photo.jpg` at `/storage/tmp/photo.jpg`. Absolute URLs must have the same origin as `app.url`. A different scheme, host or effective port fails HTTP bootstrap.

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

`read_stream` returns an async reader shared by local and S3 disks. S3 writes stream in 8 MiB parts and publish when multipart upload completes. The driver can upload up to 10,000 parts, about 78 GiB per object. Interrupted uploads are aborted when possible. If the server may have completed a write after the connection failed, `throw = true` exposes `PublishOutcomeUnknown`. Read the target to check its state before retrying.

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

`get` returns file contents. Use `read_stream` for an asynchronous reader when reading large files. The default `throw = false` returns `None` for a failed read or `false` for a failed write. Set `throw = true` to return the error. Set `report = true` to log failures handled by the default policy. Invalid paths return an error in either mode.

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

Use `size` to get the byte count and `last_modified` to get a `SystemTime` for a file on a local disk. Both methods return errors when metadata is unavailable. The same methods on `Storage` use the default disk.

```rust
use openrust::filesystem::{FilesystemAdapter, FilesystemResult};

async fn report_metadata(disk: &FilesystemAdapter) -> FilesystemResult<(u64, Option<String>)> {
    let path = "reports/today.txt";
    Ok((disk.size(path).await?, disk.mime_type(path).await?))
}
```

Use `mime_type` to check content signatures, with a filename extension or `application/octet-stream` as fallback. Use `checksum` for a streamed MD5 digest, or `checksum_with::<D>` to select a Rust digest. For these two operations, disk failures return `None` when `throw` is disabled. Invalid paths always return an error.

## Storing Files

`write_stream` copies from an asynchronous reader. The local driver writes a sibling temporary file and publishes it by rename after the stream completes. A source failure or cancellation before publication preserves an existing file. A cancellation after the rename has been dispatched can still publish the completed write.

### File Uploads

Get an upload from a multipart request with `Request::file`. Use `store` and `store_as` on the uploaded file, or `put_file` and `put_file_as` on the disk. `store_publicly` and `store_publicly_as` request public visibility. Generated names use 40 random characters and a content-derived extension when recognized. The client filename and media type are metadata. Apply your application’s upload validation before saving.

```rust
use openrust::{
    http::{HttpError, Request, Response, StatusCode},
    support::facades::Storage,
};

pub async fn store_attachment(request: &mut Request) -> Result<Response, HttpError> {
    let attachment = request.file("attachment").await?.ok_or(HttpError::InvalidRequest)?;
    let disk = Storage::disk(Some("public"))?;
    let path = attachment.store_publicly("attachments", Some("public")).await?
        .ok_or_else(|| HttpError::Internal {
            source: std::io::Error::other("Could not store attachment").into(),
        })?;
    Response::json(StatusCode::CREATED, disk.url(&path)?)
}
```

### File Visibility

On Unix, the public and private file modes are `0644` and `0600`. New subdirectories use the disk’s configured directory visibility. Override the visibility of a single write with `WriteOptions`. Use `get_visibility` and `set_visibility` on an existing file. Windows writes use the filesystem ACL. Explicit visibility changes are currently supported on Unix.

S3 visibility uses object ACLs. If no visibility is specified, the write leaves access control to the bucket. An explicit visibility request requires an ACL-capable bucket, and confirmation requires reading the ACL back. Bucket policies and the provider’s ACL enforcement determine whether an object is actually accessible anonymously.

#### Custom Unix Permissions {#local-permissions}

A named local disk can override the Unix mode for each visibility. Set `permissions.file.public`, `permissions.file.private`, `permissions.dir.public`, or `permissions.dir.private` in its configuration. Unspecified modes use the defaults. Use octal numbers such as `0o664` in Rust config.

```rust
use openrust::filesystem::{FilesystemResult, LocalPermissions};

fn permissions() -> FilesystemResult<LocalPermissions> {
    LocalPermissions::new(0o664, 0o640, 0o775, 0o750)
}
```

For an on-demand disk, give these permissions to `Storage::build_with_permissions` along with its `LocalDiskConfig`. Modes apply to newly created files and directories. `set_visibility` applies the configured file mode. Public disk mounts also require Unix public-read and directory-traverse permissions and respect modes mapped to `private`. Custom Unix modes are supported on Unix.

## Temporary file URLs

Use `disk.temporary_url(path, expiration).await?` for a download or `disk.temporary_upload_url(path, expiration).await?` for an upload. Send the returned `headers` with the upload `url`. For a local disk, enable `serve` in the disk configuration. Its route uses the path in the disk's `url`, or `/storage` when no URL is set. The local upload link accepts PUT. The S3 adapter signs against the configured bucket endpoint. The client needs network access to that endpoint.

### S3 temporary URL origin {#s3-temporary-url-origin}

Set `temporary_url` on the S3 disk, or `AWS_TEMPORARY_URL` for the default S3 configuration, to replace the scheme, host and port in generated GET and PUT links. The signed path and query stay intact. Configure the proxy to forward the original signed Host, path and query to the S3 endpoint.

### S3 request options {#s3-request-parameters}

Use `temporary_url_with` to sign download response headers or a version ID. Use `temporary_upload_url_with` to sign upload content headers and metadata. Send the returned headers with the PUT request. The disk supplies the bucket and object path.

```rust
use std::time::{Duration, SystemTime};

use openrust::{
    filesystem::{FilesystemResult, TemporaryUrlOptions},
    support::facades::Storage,
};

async fn report_url() -> FilesystemResult<String> {
    Storage::disk(Some("s3"))?
        .temporary_url_with(
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

For advanced S3 options, call `temporary_download_request` or `temporary_upload_request` on the named disk and configure the AWS SDK request builder. Both methods return the URL and signed request headers. Send those headers with the request. The disk sets the bucket and prefixed key after applying the builder options.

```rust
use std::time::{Duration, SystemTime};

use openrust::{
    filesystem::{FilesystemResult, TemporaryDownloadUrl},
    support::facades::Storage,
};

async fn report_preview() -> FilesystemResult<TemporaryDownloadUrl> {
    Storage::disk(Some("s3"))?
        .temporary_download_request(
            "reports/current.csv",
            SystemTime::now() + Duration::from_secs(300),
            |request| request.range("bytes=0-1023"),
        )
        .await
}
```

### Custom local disk URL builders {#customizing-temporary-urls}

Register `build_temporary_urls_using` or `build_temporary_upload_urls_using` on the local disk during provider boot. The callback receives the validated path, expiration and typed options. The upload callback returns a `TemporaryUploadUrl` with the URL and required headers.

## Directories {#directories}

Use `files` and `directories` to list direct children on a local disk. Use `all_files` and `all_directories` to include descendants. Pass `None` for the disk root. Results are disk-relative paths with `/` separators. File lists are sorted by path, and a missing directory returns an empty list.

```rust
use openrust::filesystem::{FilesystemAdapter, FilesystemResult};

async fn daily_reports(disk: &FilesystemAdapter) -> FilesystemResult<Vec<String>> {
    disk.make_directory("reports/daily").await?;
    disk.all_files(Some("reports")).await
}
```

Use `make_directory` to create missing parents with the disk’s directory visibility, and `delete_directory` to remove a directory and its contents. Listing skips symbolic links and temporary writes. Directory creation and deletion require a named relative path. The same methods are available on `Storage` for the default disk.

## Contracts and common operations

The container resolves `dyn contracts::filesystem::Factory`, the default `Filesystem`, and the configured `Cloud` through the existing manager and adapters. Contract stream methods are `read_stream` / `write_stream`; adapters and the Storage facade use `read_stream` / `write_stream`. Use `copy`, `r#move`, `append`, `prepend`, and `json` on the selected disk. Missing files and invalid JSON return empty results; path traversal remains rejected. Resolving `Cloud` depends on `filesystems.cloud` and does not give a local disk S3 presigned upload capabilities.

Rust requires an explicit separator for `append(path, text, separator)` and `prepend(path, text, separator)`. Pass `"\n"` for a line break, or `"\r\n"` when Windows line endings are required. `r#move` uses Rust’s raw identifier syntax because `move` is reserved.

## Local files with File {#local-file-facade}

`openrust::support::facades::File` operates on local paths directly. It uses the application’s `files` singleton (`openrust::filesystem::Filesystem`), independently of the named `Storage` disks. Relative paths follow the process working directory; use absolute application paths when the working directory may vary. These operations are synchronous: put large file operations in a blocking task rather than blocking an async request worker.

```rust
use openrust::{filesystem::FileResult, support::facades::File};
use std::path::Path;

fn write_report(directory: &Path) -> FileResult<()> {
    File::ensure_directory_exists(directory)?;
    let path = directory.join("report.txt");
    File::put_with_lock(&path, b"ready\n", true)?;
    File::append_with_lock(&path, b"complete\n", true)?;
    let bytes = File::shared_get(&path)?;
    File::replace(directory.join("copy.txt"), bytes)?;
    for file in File::all_files(directory)? {
        println!("{}", file.relative_pathname().display());
    }
    Ok(())
}
```

`get` returns bytes; `get_text` requires UTF-8. `json` returns JSON null for invalid JSON, while `json_strict` reports a decoding error. File access errors propagate from both. `lines` returns a lazy byte collection, reopening the file on each traversal; a trailing newline produces a final empty item. `hash` defaults to MD5; `hash_with_algorithm` supports MD5, SHA-1, SHA-256/384/512 and xxh128. `has_same_hash` uses xxh128 and returns false if either file cannot be read.

`files` and `directories` return direct children, sorted by relative pathname. `all_files` and `all_directories` recurse without descending through directory symlinks. Hidden entries are excluded by default, and VCS directories remain excluded when hidden files are enabled. Use `files_with_options(path, hidden, DirectoryDepth::parse([">= 1", "< 3"])?)` or `directories_with_depth` for depth filters. Directory children start at depth zero. `glob` supports shell wildcards and excludes dot files unless explicitly matched.

`replace` uses a temporary file in the same directory and atomically renames it. On Unix it follows an existing target symlink and defaults to `0777` masked by umask; use `replace_with_mode` for explicit permissions. `chmod(path, None)` and `chmod(path, Some(0))` return `FileMode::Current` containing the octal mode; `Some(nonzero)` changes permissions. `delete_many` attempts every path and returns false if any deletion fails. `clean_directory` preserves the root; recursive deletion unlinks child symlinks without removing their targets. Directory copying rejects self-nesting and symlink cycles.

Typed `r#macro`, `mixin`, `macro_call`, `has_macro` and `flush_macros` share the current application’s local service and stay isolated from other applications. `File` requires an active application scope, including HTTP, Artisan and Worker scopes. Read errors passed through `?` in HTTP handlers enter normal exception reporting; a missing local file is not automatically an HTTP 404.

MIME detection reuses OpenRust’s content detector, reading at most 8 KiB; it does not claim the full libmagic database. This initial File API does not execute dynamic PHP/Rust source, expose every PHP glob/JSON flag, or implement every PHP hashing algorithm. Windows directory links currently require symlink support; native junction equivalence and Windows ACL behavior remain outside the verified Unix behavior.

Synchronous macro arguments and results can also be non-`Send` values such as `Rc<Cell<_>>`; the stored callback remains `Send + Sync`. Each returned value belongs to that call. Compiled `get_require` and `require_once` callbacks follow the same rule; `require_once` retains only path-inclusion state.

### Compiled file modules {#compiled-file-modules}

Register an existing source path with `File::register_module(path, callback)` during application setup. The callback is compiled Rust code; the file is not interpreted or compiled at runtime. Its typed argument replaces dynamically extracted local variables; use `()` for no arguments. Reuse actual application functions rather than adding a second runtime script engine. Changing Rust code requires rebuilding the application.

```rust
use openrust::{filesystem::{FileResult, RequiredFile}, support::facades::File};
use std::path::Path;

fn register_and_load(path: &Path) -> FileResult<()> {
    File::register_module(path, |files, name: String| {
        // 中文参数说明：回调是编译进应用的 Rust 函数。
        Ok(format!("Hello {name}: {}", files.exists("Cargo.toml")))
    })?;
    let value: String = File::get_require(path, "OpenRust".to_owned())?;
    let once: RequiredFile<String> = File::require_once(path, "ignored".to_owned())?;
    assert_eq!(once, RequiredFile::AlreadyIncluded);
    assert!(value.starts_with("Hello OpenRust"));
    Ok(())
}
```

`get_require` executes the callback on every call and returns its typed value. `require_once` returns `RequiredFile::Value(value)` on the first inclusion and `AlreadyIncluded` later, distinct from a module returning `true`. Any `get_require` also marks the file as included. Inclusion is recorded before execution: recursion and later calls skip a module even after its callback fails or panics. Replacing registration does not reset inclusion; explicit `get_require` still executes the replacement.

Paths are canonicalized, so symlink aliases share inclusion. A missing, deleted or non-file path produces a file error even if previously included. An existing unregistered file returns `UnregisteredModule`; wrong input/output types return a typed callback error, never a fabricated module value. Registrations and inclusion belong to the application’s `files` singleton, shared by its HTTP/Artisan/Worker calls and isolated from other applications. Unlike a global interpreter include table, they do not leak between applications. Concurrent `require_once` claims one execution; another caller may see `AlreadyIncluded` before the first callback completes. Callbacks and retired registrations are not invoked or dropped while holding registry locks.

### Conditional file calls {#conditional-file-calls}

Use `File::when(value, callback)` and `unless` for conditional calls. `ConditionValue` retains the original input type: booleans, numbers, strings (`""` and `"0"` are false), optional values and collections are supported. A callback receives the actual local service and the original value. Define `ConditionValue` for a business object when needed. JSON objects are truthy even when empty; JSON arrays follow collection emptiness.

```rust
use openrust::{filesystem::FileResult, support::facades::File};
use std::path::Path;

fn write_if_missing(path: &Path) -> FileResult<()> {
    File::unless_test(
        |files| files.exists(path),
        |files, _| files.put(path, b"created").map(Some),
    )?;
    let bytes = File::when_condition(true)?
        .call(|files| files.get(path))?
        .into_value();
    assert!(bytes.is_some());
    Ok(())
}
```

The selected callback returns `FileResult<Option<T>>`. `Some(value)` becomes `ConditionalResult::Value(value)`, including `false`, `0`, or an empty string. `None` returns `Target`, retaining the exact original service; use `None::<()>` when no result type is otherwise inferable. A skipped branch also returns `Target`. `into_value` and `into_target` unpack these alternatives. Errors propagate unchanged. `when_else` / `unless_else` add a fallback; only one branch runs.

Use `when_test` / `unless_test` when the condition is computed from the service: the resolver runs once and its original result is passed to the selected callback. `when_condition(value)` / `unless_condition(value)` produce a typed higher-order proxy: `.call(|files| ...)` executes the actual public operation only when selected. To capture a query first, use `when_capture()?.capture(|files| ...)` or `unless_capture`; `try_capture` accepts a fallible query. A missing captured condition returns `MissingCondition`. A proxy operation returns its actual value, so an operation returning `None` remains `Value(None)`; the callback-style null fallback does not apply to this form.

A facade proxy owns the original application service, so moving it to another application scope does not resolve a different service. Direct `Filesystem` calls provide the same methods with a borrowed target. Rust uses these explicit suffixes and `ConditionalResult` instead of overloaded argument counts, dynamic method interception or a target/scalar union; actual conditions, selected side effects and error propagation remain observable.
