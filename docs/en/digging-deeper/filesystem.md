# File Storage

## The Public Disk

The `public` disk is configured in `config/filesystems.rs`. Its default local root is `storage/app/public`, and its URL is the application URL followed by `/storage`. Set `PUBLIC_STORAGE_ROOT` to change the directory or `PUBLIC_STORAGE_URL` to change the public URL. Relative filesystem paths are resolved from the application root.

### Serving local files (OpenRust extension)

The default `routes/web.rs` contains these mounts, in this order:

```rust
Route::staticPublic()?;
Route::staticDisk("public")?;
```

`staticPublic` reads the website directory from `app.public_path`, which defaults to `public`. `APP_PUBLIC_PATH` can override it. `staticDisk` reads the local disk root and derives the entire mount path from the disk's `url`. There is no separate `url_prefix` configuration. For example, `PUBLIC_STORAGE_URL=/storage/tmp/` exposes `storage/app/public/photo.jpg` at `/storage/tmp/photo.jpg`. An absolute URL must have the same origin as `app.url`; a different scheme, host or effective port is rejected during HTTP bootstrap. A remote disk such as S3 cannot be mounted by this local file server.

Laravel normally exposes the public disk through a symbolic link created by `storage:link`. OpenRust's mounts serve local files directly without that link. The `storage:link` command and the general filesystem API are outside this delivered subset.

Application routes take precedence over static files. Static mounts are searched in declaration order and fall through when a file is missing. With the default order, `public/storage/photo.jpg` wins over `storage/app/public/photo.jpg` at `/storage/photo.jpg`. The server supports GET and HEAD, MIME types, `Last-Modified` and `If-Modified-Since`. It does not list directories, and rejects traversal and symlinks escaping the mount root. Missing files use the application's normal 404 policy.

See [routing](../the-basics/routing.md#mounting-a-static-directory) and [public file configuration](../getting-started/configuration.md#public-files).
