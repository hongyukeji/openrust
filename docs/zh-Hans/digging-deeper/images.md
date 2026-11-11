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

## 存储图像 {#storing-images}

`store` 按编码格式生成文件名。`storeAs` 使用指定文件名，`storePublicly` 和 `storePubliclyAs` 请求公开可见性。磁盘写入成功后返回存储路径。

## 检查图像 {#inspecting-images}

用 `mimeType`、`extension`、`dimensions`、`width` 和 `height` 检查处理后的结果。`dominantColor` 返回按透明度加权的 RGB 平均色，格式为十六进制。

## 图像驱动 {#image-drivers}

应用默认使用 `image` 驱动。`IMAGE_DRIVER` 可选择已注册的驱动。用 `Image::extend` 注册 `Driver` 实现，用 `using` 为图像选择驱动。默认限制为每个输入 32 MiB、每边 16,384 像素、每张图像 2,400 万像素。

JPEG、PNG、GIF、WebP 和 BMP 支持读写。AVIF 支持写入。JPEG、WebP 和 AVIF 使用设置的质量值。对动画输入的变换只处理单帧。
