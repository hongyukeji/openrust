# 图像处理

## 读取图像 {#reading-images}

`file()` 在渲染、克隆、继续变换、切换格式或驱动后仍保留原始上传文件。

在应用中从 `openrust::support::facades` 导入 `Image`。用 `request.image("avatar").await?` 从 multipart 字段创建图像，或用 `Image::fromUpload` 读取已有上传。`Image::fromStorage` 和 `Storage::disk(Some("public"))?.image(path)` 使用配置磁盘。还可以使用字节、Base64、路径、HTTP URL 和异步流。流在构造时读取，其余来源在渲染或检查图像时加载。

## 变换和编码 {#manipulating-images}

变换按追加顺序执行。`resize` 接收可选的宽和高。`scale` 保持宽高比且不会放大。`cover` 裁剪并填满目标尺寸，`contain` 保留整张图像并填充空白。还支持裁剪、EXIF 方向、旋转、模糊、灰度、锐化和翻转。用克隆分出处理链，通过 `toBytes().await?`、`toBase64().await?` 或 `toDataUri().await?` 获取结果。

`flip()` 垂直翻转，`flop()` 水平翻转。`resize(width, height)` 和 `scale(width, height)` 返回 `Result<Image>`，两个尺寸都缺失时立即报错；传入的零尺寸归一为 1。模糊和锐化强度在交给驱动处理器之前限制为最大 100。

成功加载的来源在分支之间共享。修改已渲染的图像时以处理后的字节为起点，不重复此前的变换。`hashName("").await?` 返回该图像稳定的文件名，`store` 复用此名称；`hashName("avatars").await?` 添加路径前缀。`toString().await?` 返回数据 URI。路由处理器可直接返回 `Image` 或 `Result<Image, HttpError>`，响应包含图像字节及对应 MIME 类型。

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

内置驱动在 JPEG、PNG、WebP、AVIF、HEIC 和 JPEG XL 的变换与转换中携带 EXIF、ICC 配置文件和 XMP。当 `orient()` 应用 JPEG、PNG 或 WebP 的 EXIF 方向时，输出将方向字段重置为正向，并保留其他 EXIF 字节。HEIF 动画输出将这些字段写入封面图像。读取 AVIF 或 HEIC 序列时，主轨道有 ICC 配置文件则优先使用。封面缺少 EXIF 或 XMP 时，从主轨道读取对应字段。JPEG 变换后继续输出 JPEG 时，也会保留包含 IPTC 的 Photoshop APP13 配置和 JPEG 注释。这些段保留在 JPEG 容器中。

AVIF 与 HEIC 序列的变换和相互转换保留每个显示帧各自的 ICC、EXIF 和 XMP。ICC 随图像样本描述，EXIF 和 XMP 通过关联的元数据轨道按显示时间匹配到原帧。转为动画 PNG、WebP 或 JPEG XL 时使用这些格式的全局元数据字段。

## 存储图像 {#storing-images}

`store` 按编码格式生成文件名。`storeAs` 使用指定文件名，`storePublicly` 和 `storePubliclyAs` 请求公开可见性。磁盘写入成功后返回存储路径。

`storeAs("avatar.png", None, None).await?` 将第一个参数作为完整文件名；同时指定目录和名称时，只去掉合并路径两端的斜杠。

## 检查图像 {#inspecting-images}

用 `mimeType`、`extension`、`dimensions`、`width` 和 `height` 检查处理后的结果。`dominantColor` 返回按透明度加权的 RGB 平均色，格式为十六进制。

## 图像驱动 {#image-drivers}

应用默认使用 `image` 驱动。`IMAGE_DRIVER` 可选择已注册的驱动。用 `Image::extend` 注册 `Driver` 实现，用 `using` 为图像选择驱动。默认限制为每个输入 32 MiB、每边 16,384 像素、每张图像 2,400 万像素。

内置驱动读写 JPEG、PNG、GIF、WebP、AVIF、HEIC、JPEG XL 和 BMP。可用 `toAvif()`、`toHeic()`、`toJxl()` 或 `toFormat("heif")` 转换格式。`heif` 别名输出扩展名为 `.heic`、MIME 类型为 `image/heic` 的文件。JPEG、WebP、AVIF、HEIC 和 JPEG XL 应用指定质量。WebP 和 JPEG XL 在质量为 100 时采用无损编码。编解码均在 Rust 中运行。

### 自定义转换处理器 {#custom-transformation-handlers}

用 `Image::transformUsing(driver, name, callback)?` 或 `ImageManager` 的同名方法为指定驱动注册处理器。也可先注册处理器，再通过 `extend` 注册应用驱动；管理器的 `extend` 返回 `Result`。驱动契约提供 `transformUsing` 和 `dimensions`；不支持扩展处理器的驱动返回 `UnsupportedTransformationHandler`。`Image::driver(None)?`、`getDefaultDriver()` 和 `getDrivers()` 提供当前配置驱动与已注册实例。

```rust
use std::sync::Arc;
use openrust::image::{Error, NativeImage, Transformation};
use openrust::support::facades::Image;

Image::transformUsing(
    "image",
    "app::Invert",
    Arc::new(|native, _transformation| {
        let image = native.downcast_mut::<NativeImage>()
            .ok_or(Error::InvalidTransformation)?;
        image.invert();
        Ok(())
    }),
)?;
let output = Image::fromBytes(bytes)?
    .transform(Transformation::Custom {
        name: "app::Invert".into(),
        options: serde_json::Value::Null,
    })
    .toBytes().await?;
```

内置转换使用变体原名，如 `Resize` 或 `Grayscale`，同名处理器覆盖内置操作。自定义转换使用传入名称，建议加应用命名空间以避免冲突。回调接收驱动实际对象的 `&mut dyn Any`，内置驱动使用 `NativeImage`；返回错误即停止流水线。原生动画处理器逐个处理参与转换的帧，修改后的帧仍须符合配置限额。

`usingGd()` 和 `usingImagick()` 分别选择应用已注册的 `gd` 和 `imagick` 驱动。框架内置原生 `image` 后端；转换需要的后端未注册时返回 `UnknownDriver`，原样读取字节不解析所选后端。
