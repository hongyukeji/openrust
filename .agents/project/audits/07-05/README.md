# 07-05 Images

依赖 03-01、04-02。对照固定 [Laravel Images](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/images.md)、[Image](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Image/Image.php)、[ImageManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Image/ImageManager.php)、[InterventionDriver](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Image/Drivers/InterventionDriver.php) 和 [ImageTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Image/ImageTest.php)。Provider 沿用应用编译型注册，参照固定 [Goravel Application](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/foundation/application.go)。旧版 `6d1a8192` 有图像 MIME 映射，未找到可复用的图像处理驱动。本批复用当前上传内容、随机文件名、磁盘读取和写入、HTTP 客户端。

应用级 `Image` Facade 通过 Manager 解析命名驱动。输入支持上传、磁盘、字节、Base64、本地路径、HTTP URL 及异步流。支持顺序变换、EXIF 方向、裁剪与缩放、背景色、编码质量、格式和尺寸检查、平均色以及公开与普通存储。类型化 Driver 接收有序管线，可解释自定义变换。Rust 使用拥有所有权的 builder，克隆可分支，共享同一管线的渲染结果。CPU 编解码在有界阻塞任务运行，默认每个应用至多四个任务。一次性异步流在创建时读取，其他输入延迟加载。损坏输入会在输出原始字节前经过有界解码验证。

内置 `image` 驱动读写 JPEG、PNG、GIF、WebP、BMP，写入 AVIF。JPEG、WebP、AVIF 使用 1 至 100 的质量参数，默认 70。图像输入限制 32 MiB，边长 16,384，像素总数 2,400 万，特定编码器还有格式自身限制。Rust 用 image 0.25.10 解码，使用 webp 0.3.1 的 libwebp 绑定编码 WebP。核对 libwebp-sys 0.9.6 的 Huffman 修复源文件与 [libwebp 1.3.2](https://github.com/webmproject/libwebp/blob/v1.3.2/src/utils/huffman_utils.c) 校验值相同，入站解码走 Rust。动画变换当前只处理一帧。AVIF 解码、HEIC、JXL、GD/Imagick 驱动及原样元数据保留尚未对齐。任意角度旋转采用最近邻采样。

## 验收

- Rust 1.94.0、macOS Apple Silicon。锁定离线工作区 516 项通过，4 个忽略项由集成测试作为子进程启动。格式与严格 Clippy 通过。临时 SQLite 上的 Artisan migrate、inspire、首页及 /up 通过。
- 四项定向测试覆盖六种实际编码、五种解码、WebP 质量 1 与 100、EXIF 方向、变换结果、尺寸和字节限制、损坏输入、驱动隔离及真实 multipart 上传。用伪造的文件扩展名和 MIME 验证按内容识别，经公共磁盘、HTTP URL 和本地文件重新读取实际编码结果。
- 三语三个 Rust 示例实际运行，VitePress 构建、12 项通用浏览器检查及 6 项 Images 页面导航通过。
- 在隔离副本验证本单元依赖，并对合并在途 gRPC 后的锁文件单独执行锁定离线 metadata。原有 reqwest、tonic 和 tokio-stream 与 image、webp 均保留。

下一单元 07-06 Search，依赖 05-03、06-04、07-02。
