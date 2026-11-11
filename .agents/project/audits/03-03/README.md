# 03-03 S3 磁盘

核对固定 Laravel 13.x 的 [S3 兼容文件系统](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/filesystem.md#amazon-s3-compatible-filesystems)、[文件可见性](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/filesystem.md#file-visibility)、[FilesystemManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Filesystem/FilesystemManager.php)、[AwsS3V3Adapter](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Filesystem/AwsS3V3Adapter.php)和[通用适配器回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Filesystem/FilesystemAdapterTest.php)。沿用 03-01 对固定 [Goravel Provider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/filesystem/service_provider.go) 的应用级装配。核对旧版 `6d1a8192` 的 `src/filesystem.rs` 和 `tests/server/storage/s3.rs`，提取对象前缀、ACL 解析、取消清理与发布状态不确定的处理。旧版 AWS SDK 要求 Rust 1.94.1，主线锁定经 Rust 1.94.0 验证的版本。

- 接通默认及具名 S3 磁盘、`Storage::buildS3`、区域、自定义 endpoint、路径式桶地址、键前缀、显式会话凭据及 AWS SDK 默认凭据链。对象可流式读取、写入、检查存在性、删除及读取和设置 ACL。共享的 `FilesystemReader` 是可异步读取的类型擦除接口，供本地和 S3 共用。
- 沿用旧版完成后发布的可观察契约。S3 直接使用目标对象的原生 multipart，`CompleteMultipartUpload` 前旧对象保持可读，取消或失败会尝试 Abort。进程强杀或服务持续不可用时，未完成上传需由存储桶生命周期回收。发布后的响应不确定时，`throw = true` 返回 `PublishOutcomeUnknown`，默认失败值仍为 `false`。写入以 8 MiB 分段，10,000 段上限约 78 GiB。
- 默认省略 ACL，沿用存储桶访问控制。显式 ACL 先在权限允许时核对 Object Ownership，拒绝 BucketOwnerEnforced 桶，并读回对象 ACL 确认设置。对象 ACL 与桶策略各自影响匿名访问，兼容服务也可能只保存 ACL 元数据。ACL 读取与设置按 [S3 接口契约](https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutObjectAcl.html) 实现。
- AWS 公网、真实角色及 STS 凭据、S3 兼容厂商完整矩阵和最大对象边界未实测。公开对象 URL、下载响应、签名临时 URL 与上传接口随 04-09、06-01 衔接。

## 验收

macOS arm64、Rust 1.94.0。隔离副本 `/private/tmp/openrust-03-03.3cVUnR/app-under-test` 以 03-01 候选为底。RustFS 1.0.5 使用 Docker 镜像摘要 `sha256:b81d16a90ee25bd9511280127ca1471788c6e874cf0f0da759645f46b2dc1d74`。SeaweedFS 4.53 使用发布版二进制。两个服务均仅绑定回环地址，测试创建独立桶。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。377 项通过，其中 3 项使用真实 S3 兼容服务。真实 MySQL 未在本批复跑。
- RustFS 验证空对象、Unicode 键、前缀隔离、9 MiB 多段传输、同键并发写入的完整结果、源失败保留旧对象、取消后无遗留 multipart、错误凭据和删除。在转发至真实 RustFS 的代理中注入一次 503 SlowDown，同一读取请求重试成功。
- SeaweedFS 验证启用 ACL 时公开和私有的真实匿名 GET 分别为 200 和 403。单独的 BucketOwnerEnforced 桶验证普通写入成功，显式公开写入及更改可见性返回失败。直接从兼容服务读回 ACL 与字节。
- 三语 Rust 示例在应用请求作用域接真实 RustFS 执行，磁盘配置未填显式密钥，使用进程环境凭据链。`./artisan inspire` 正常退出。`npm run build`、12 项常规浏览器组合和 6 项文件系统页章节及锚点检查通过。工作区锁定依赖解析及 `git diff --check` 通过。

下一批为 03-04 应用加密与密钥轮换，前置为 01-01 的配置和应用基础。
