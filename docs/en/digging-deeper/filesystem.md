# File Storage

## Configuration

Configure the default disk with `FILESYSTEM_DISK` and named disks in `config/filesystems.rs`. The `local` and `public` disks both use the local driver. Each disk has its own root. The manager resolves and caches named disks within the current application.

### The Local Driver

The default `local` disk stores files under `storage/app/private`. Use paths relative to the disk root. Absolute paths, traversal components and reserved temporary names are rejected. On Unix the local driver opens files relative to directory handles, rejects symbolic links during traversal and refuses to read files with multiple hard links.

### The Public Disk

The `public` disk is configured in `config/filesystems.rs`. Its default local root is `storage/app/public`, and its URL is the application URL followed by `/storage`. Set `PUBLIC_STORAGE_ROOT` to change the directory or `PUBLIC_STORAGE_URL` to change the public URL. Relative filesystem paths are resolved from the application root.

Create or remove the links configured in `filesystems.links` with Artisan. The default link is `public/storage`, pointing to the public disk directory. Use `--force` to recreate an existing symbolic link. Existing ordinary files and directories are preserved.

```sh
./artisan storage:link --relative
./artisan storage:unlink
```

#### Serving local files (OpenRust extension)

The default `routes/web.rs` contains these mounts, in this order:

```rust
Route::staticPublic()?;
Route::staticDisk("public")?;
```

`staticPublic` reads the website directory from `app.public_path`, which defaults to `public`. `PUBLIC_PATH` can override it. `staticDisk` reads the local disk root and derives the entire mount path from the disk's `url`. There is no separate `url_prefix` configuration. For example, `PUBLIC_STORAGE_URL=/storage/tmp/` exposes `storage/app/public/photo.jpg` at `/storage/tmp/photo.jpg`. An absolute URL must have the same origin as `app.url`; a different scheme, host or effective port is rejected during HTTP bootstrap. This file server mounts local disks.

The OpenRust HTTP server can also serve the public disk directly. Its disk mount requires a public file and searchable directories on Unix. Symlinks, temporary write files and files with multiple hard links are excluded.

Application routes take precedence over static files. Static mounts are searched in declaration order and fall through when a file is missing. With the default order, `public/storage/photo.jpg` wins over `storage/app/public/photo.jpg` at `/storage/photo.jpg`. The server supports GET and HEAD, MIME types, `Last-Modified` and `If-Modified-Since`. It does not list directories, and rejects traversal and symlinks escaping the mount root. Missing files use the application's normal 404 policy.

See [routing](../the-basics/routing.md#mounting-a-static-directory) and [public file configuration](../getting-started/configuration.md#public-files).

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

`get` returns file contents. Use `readStream` for a Tokio file handle when reading large files. The default `throw = false` returns `None` for a failed read or `false` for a failed write. Set `throw = true` to return the error. Set `report = true` to log failures handled by the default policy. Invalid paths return an error in either mode.

## Storing Files

`writeStream` copies from an asynchronous reader. The local driver writes a sibling temporary file and publishes it by rename after the stream completes. A source failure or cancellation before publication preserves an existing file. A cancellation after the rename has been dispatched can still publish the completed write.

### File Visibility

On Unix, the public and private file modes are `0644` and `0600`. New subdirectories use the disk’s configured directory visibility. Override the visibility of a single write with `WriteOptions`. Use `getVisibility` and `setVisibility` on an existing file. Windows writes use the filesystem ACL. Explicit visibility changes are currently supported on Unix.
