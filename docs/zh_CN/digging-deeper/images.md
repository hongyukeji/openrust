# 图像处理

## 读取图像 {#reading-images}

在应用中从 `openrust::support::facades` 导入 `Image`。用 `request.image("avatar").await?` 从 multipart 字段创建图像，或用 `Image::fromUpload` 读取已有上传。`Image::fromStorage` 和 `Storage::disk(Some("public"))?.image(path)` 使用配置磁盘。还可以使用字节、Base64、路径、HTTP URL 和异步流。流在构造时读取，其余来源在渲染或检查图像时加载。

## 变换和编码 {#manipulating-images}

变换按追加顺序执行。`resize` 接收可选的宽和高。`scale` 保持宽高比且不会放大。`cover` 裁剪并填满目标尺寸，`contain` 保留整张图像并填充空白。还支持裁剪、EXIF 方向、旋转、模糊、灰度、锐化和翻转。用克隆分出处理链，通过 `toBytes().await?`、`toBase64().await?` 或 `toDataUri().await?` 获取结果。

```rust
use openrust::image::{ImageManager, Limits};

let manager = ImageManager::new(Limits::default());
let source = manager.fromBase64(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC",
);
let thumbnail = source.cover(4, 4).toWebp();
assert_eq!(thumbnail.dimensions().await?, (4, 4));
assert_eq!(thumbnail.mimeType().await?, "image/webp");
```

## 动画图像 {#animated-images}

GIF、动画 PNG（APNG）、WebP、AVIF、HEIC 和 JPEG XL 在变换和转换时可保留显示帧、帧顺序及播放次数。GIF 的延迟单位为 10 毫秒。WebP、AVIF、HEIC、JPEG XL 的输出延迟单位为 1 毫秒。APNG 可存储分数延迟。JPEG XL 将零延迟取整为 1 毫秒，让该帧仍可显示。

未经修改的动画在通过帧检查后返回原始字节。JPEG、BMP 输出和 `dominantColor()` 使用第一个显示帧。HEIF 输入采用一条主时序轨道，可包含辅助透明度轨道。HEIC 可以带有编码填充，并通过 clean aperture 指定显示尺寸。

驱动对动画输入和输出均设置 1,024 帧和 9,600 万总像素的上限。自定义像素限额可将总上限降低至单图限额的四倍。每一帧同时受单图限额约束。

## 图像元数据 {#image-metadata}

内置驱动在 JPEG、PNG、WebP、AVIF、HEIC 和 JPEG XL 的变换与转换中携带 EXIF、ICC 配置文件和 XMP。当 `orient()` 应用 JPEG、PNG 或 WebP 的 EXIF 方向时，输出将方向字段重置为正向，并保留其他 EXIF 字节。HEIF 动画输出将这些字段写入封面图像。JPEG 变换后继续输出 JPEG 时，也会保留包含 IPTC 的 Photoshop APP13 配置和 JPEG 注释。这些段保留在 JPEG 容器中。

## 存储图像 {#storing-images}

`store` 按编码格式生成文件名。`storeAs` 使用指定文件名，`storePublicly` 和 `storePubliclyAs` 请求公开可见性。磁盘写入成功后返回存储路径。

## 检查图像 {#inspecting-images}

用 `mimeType`、`extension`、`dimensions`、`width` 和 `height` 检查处理后的结果。`dominantColor` 返回按透明度加权的 RGB 平均色，格式为十六进制。

## 图像驱动 {#image-drivers}

应用默认使用 `image` 驱动。`IMAGE_DRIVER` 可选择已注册的驱动。用 `Image::extend` 注册 `Driver` 实现，用 `using` 为图像选择驱动。默认限制为每个输入 32 MiB、每边 16,384 像素、每张图像 2,400 万像素。

内置驱动读写 JPEG、PNG、GIF、WebP、AVIF、HEIC、JPEG XL 和 BMP。可用 `toAvif()`、`toHeic()`、`toJxl()` 或 `toFormat("heif")` 转换格式。`heif` 别名输出扩展名为 `.heic`、MIME 类型为 `image/heic` 的文件。JPEG、WebP、AVIF、HEIC 和 JPEG XL 应用指定质量。WebP 和 JPEG XL 在质量为 100 时采用无损编码。编解码均在 Rust 中运行。
