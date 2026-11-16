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


## 静态格式补齐

- 固定 Laravel 的 `InterventionDriver` 格式白名单包含 AVIF、HEIC／HEIF 和 JPEG XL。`Image::toFormat` 接受 `heif` 并输出 `heic`。新增 `toFormat`、`toHeic`、`toJxl`，保留原有类型化接口。旧版仅有相关 MIME 映射。 [固定 Image API](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Image/Image.php#L280-L323) [固定格式白名单](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Image/Drivers/InterventionDriver.php#L74-L81)
- AVIF 读取使用 imgalifier-avif，写入继续使用 ravif。HEIC 使用 OxideAV 的纯 Rust HEIF／HEVC 实现。JPEG XL 由 jxl-oxide 解码、zune-jpegxl 无损编码。WebP 写入改用 oxideav-webp，移除 libwebp 绑定并复验质量。各解码路径应用尺寸与像素限制，继续执行原有输入字节限制。
- Apple Silicon macOS、Rust 1.94.0。5 项 `images` 回归及全工作区严格 Clippy 通过。变更文件格式通过，工作区格式仍报告已有 Sanctum 测试排版。实际 Artisan 双向转换八种格式，并读取由 macOS 系统编解码器独立生成的 AVIF 和 HEIC。系统读取器独立核对全部输出尺寸。损坏输入返回退出码 1。实际 HTTP 使用外部 HEIC 并输出 AVIF、HEIF 别名、JPEG XL、WebP，检查 MIME 和尺寸。三语文档构建和 12 项通用浏览器检查通过。
- 后续范围包括动画帧、循环次数和时序，以及 JPEG XL 有损质量与平台特定编码差异。


## 帧变换与时序

- 对照固定 Laravel 通过 Intervention 4.2 的 GIF 与 WebP 多帧输出。内置驱动对 GIF、APNG、WebP 使用合成后的显示帧，逐帧应用变换，保留顺序与播放次数。GIF 的有限重复包含首轮之外的次数，APNG 与 WebP 存储总轮次。转换保留可表示的延迟，跨格式以 GIF 的 10 毫秒、WebP 的 1 毫秒或 APNG 的分数精度编码。单帧动画同样保留延迟和循环信息。 [Intervention 4.2 GIF 编码器](https://github.com/Intervention/image/blob/4.2.0/src/Drivers/Gd/Encoders/GifEncoder.php) [Intervention 4.2 WebP 编码器](https://github.com/Intervention/image/blob/4.2.0/src/Drivers/Imagick/Encoders/WebpEncoder.php)
- 原样读取验证所有 GIF、APNG 或 WebP 帧，支持静态输出只取第一个显示帧。输入和变换后分别限制 1,024 帧以及最多 9,600 万总像素。JPEG XL 改用 jixel 0.5.10，在 Rust 1.94.0 上支持有损质量和质量为 100 的无损路径。复验 RGBA 像素及透明度。
- Apple Silicon macOS 上通过 10 项定向回归，覆盖 GIF 局部帧及恢复规则、APNG 分数延迟和透明帧、WebP 有损及无损编码、单帧动画、有限及无限播放、输入和输出预算、后续帧损坏、静态输出及 JPEG XL 质量。实际 Artisan 双向转换 GIF、APNG、WebP。真实 HTTP 从同一输入写出三种动画，独立 Pillow 读取器检查帧数、40／90／180 毫秒时序、三轮播放及画面颜色。真实 Artisan 和 HTTP 的 JPEG XL 输出由系统读取器复核。全目标严格 Clippy 和变更文件格式通过。工作区格式仍报告已有 Sanctum 测试排版。三语 VitePress 构建、12 项通用检查及 6 项 Images 路径和锚点检查通过。

- 两批隔离应用、样本、文档构建及独占验收可执行文件已清理。末批项目卷可用空间从 62,371,472 KiB 增至 62,714,712 KiB。共享 Cargo 缓存保留。

## 剩余验收

AVIF、HEIC、JPEG XL 的序列往返和时序仍待验收。固定 Laravel 的 Imagick 驱动将这些格式交给系统编解码扩展，序列写入依赖其部署能力。当前纯 Rust 驱动对这些格式提供静态图像读写。HEIF 后端的序列接口提供帧和毫秒延迟，仍需核对循环元数据与总量限制。JPEG XL 的 jixel 公开编码接口接收单张图像，仍需选择符合项目最低工具链的序列编码路径。变换后原样保留专有元数据也仍待核对。07-05 保持开放。 [Intervention 4.2 AVIF 编码器](https://github.com/Intervention/image/blob/4.2.0/src/Drivers/Imagick/Encoders/AvifEncoder.php) [Intervention 4.2 HEIC 编码器](https://github.com/Intervention/image/blob/4.2.0/src/Drivers/Imagick/Encoders/HeicEncoder.php) [Intervention 4.2 JPEG XL 编码器](https://github.com/Intervention/image/blob/4.2.0/src/Drivers/Imagick/Encoders/JxlEncoder.php)
