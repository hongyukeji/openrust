# 04-09 上传、下载及对象 URL

## 契约与实现

- 对照固定 Laravel 的 `UploadedFile`、`FilesystemAdapter`、`AwsS3V3Adapter` 和适配器测试，沿用固定 Goravel 文件系统 Provider 的应用级磁盘装配。复用旧版 `6d1a8192` 上传的 40 字符随机名、按文件对象缓存名称、按内容识别扩展名、显式命名及可见性，接到当前本地及 S3 流式写入。 [Laravel 适配器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Filesystem/FilesystemAdapter.php) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Filesystem/FilesystemAdapterTest.php) · [Goravel Provider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/filesystem/service_provider.go)
- 接通 `UploadedFile::{hashName, store, storeAs, storePublicly, storePubliclyAs}`、磁盘及 Storage 门面的 `putFile`、`putFileAs`。随机名和识别结果由同一上传及其克隆共享。基于已有的有界 multipart 缓冲，保存时共享 Bytes 并复用异步写入。
- `download` 和 `response` 从已打开的文件流生成附件或内联响应，补齐默认响应头并支持调用方覆盖。内容处置头支持 Unicode 名称、百分号和 ASCII 回退，拒绝路径分隔符和控制字符。普通磁盘下载的授权由应用处理，公开静态挂载执行已有的公开文件限制。
- `Storage::url` 和具名磁盘 `url` 读取现有 URL 配置。S3 复用 AWS SDK 的端点解析规则，编码对象键并保留磁盘前缀。本地磁盘遵循 Laravel 的 `/storage` 回退及 URL 安全文件名约定。
- Rust 适配使用 `Option` 表达 Laravel 的未保存结果，异步读取和流式 HTTP 主体共用当前内核。上传通过文件签名与纯文本检测推断扩展名，识别范围和下载文件名的 ASCII 转写表与 PHP 环境存在差异。本地下载 MIME 以磁盘文件名推断，S3 使用对象元数据。按需磁盘的现有类型化配置签名保留，显式 URL 基址通过具名磁盘配置。签名临时 URL 的端到端验收归 06-01。

## 验收

隔离基线 `21a65e98` 加已验收依赖和本批改动，macOS arm64，Rust 1.94.0。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过，共 432 项测试。`./artisan inspire` 正常退出。追加 256 KiB 下载及非法文件名断言后复跑本地磁盘专项、格式和严格 Clippy，全部通过。
- 真实 HTTP 覆盖 multipart 上传、伪造客户端类型与扩展名、随机命名、路径穿越拒绝、公开 URL、私有文件 404、应用磁盘隔离、附件名编码、非法名称 400、已知长度和 256 KiB 完整正文。
- 真实 RustFS 1.0.5 验证路径式对象 URL、配置 CDN 基址及前缀、编码空格和 Unicode、下载对象与元数据，并复跑原有并发、失败和取消测试。该轮覆盖指定 RustFS 主测试和真实 Redis 回归，未重新启动 ACL 专用 S3 后端。
- 三语的下载与上传示例共六份，按原样编译并经真实 HTTP 运行，同时请求生成的公共 URL。VitePress 构建、根路径和子路径的 12 项通用浏览器组合及 6 项文件系统章节、锚点和导航检查通过。

下一批为 05-01 Eloquent 模型读写和作用域，依赖已验收的 02-03 查询、02-04 写入和 02-08 集合，使用真实 MySQL 8.4 验收。
