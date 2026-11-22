# Image Manipulation

## Reading images {#reading-images}

`file()` retains the original uploaded file after rendering, cloning, further transformations, format changes, and driver selection.

In an application, import `Image` from `openrust::support::facades`. Use `request.image("avatar").await?` to create an image from a multipart field, or call `Image::fromUpload` with an existing upload. `Image::fromStorage` and `Storage::disk(Some("public"))?.image(path)` use configured disks. Other inputs include bytes, Base64, paths, HTTP URLs, and asynchronous streams. Streams are read at construction. Other sources are loaded when rendering or inspecting the image.

## Manipulating and encoding images {#manipulating-images}

Transformations run in the order they are added. `resize` takes optional width and height. `scale` preserves aspect ratio without enlarging the image. `cover` crops to fill, while `contain` preserves the whole image and fills empty space. Other operations include crop, EXIF orientation, rotation, blur, grayscale, sharpen, and flips. Clone the pipeline to branch it. Use `toBytes().await?`, `toBase64().await?`, or `toDataUri().await?` to retrieve the result.

`flip()` flips vertically; `flop()` flips horizontally. `resize(width, height)` and `scale(width, height)` return `Result<Image>` and reject two missing dimensions immediately. Supplied zero dimensions become 1. Blur and sharpen amounts are capped at 100 before reaching a driver handler.

A successfully loaded source is shared across branches. Modifying a rendered image starts from its processed bytes, so earlier transformations are not repeated. `hashName("").await?` returns a stable filename for that image, and `store` reuses it. `hashName("avatars").await?` adds the path prefix. `toString().await?` returns the data URI. A route handler may return `Image` or `Result<Image, HttpError>` directly; its response contains the image bytes and MIME type.

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

## Animated images {#animated-images}

GIF, animated PNG (APNG), WebP, AVIF, HEIC, and JPEG XL can preserve displayed frames, frame order, and play count through transformations and conversions. GIF stores delays in 10 ms units. WebP, AVIF, HEIC, and JPEG XL output uses 1 ms units. APNG can store fractional delays. JPEG XL rounds a zero delay up to 1 ms so the frame remains visible.

An unchanged animation returns its original bytes after frame validation. JPEG and BMP output and `dominantColor()` use the first displayed frame. HEIF inputs use a single primary timed track and can include an auxiliary alpha track. HEIC can carry coded padding with a clean aperture that describes the displayed size.

The driver bounds animated inputs and outputs to 1,024 frames and 96 million total pixels. A custom pixel limit can lower the total to four times the per-image limit. Each frame must also fit within the per-image limits.

## Image metadata {#image-metadata}

The built-in driver carries EXIF, ICC profiles, and XMP through transformations and conversions among JPEG, PNG, WebP, AVIF, HEIC, and JPEG XL. When `orient()` applies the EXIF direction from JPEG, PNG, or WebP, the output resets the direction tag to upright and retains the other EXIF bytes. Animated HEIF output stores these fields on the cover image. When reading an AVIF or HEIC sequence, the driver uses the main track's ICC profile when present and falls back to its track-level EXIF and XMP if the cover lacks those fields. JPEG-to-JPEG transformations also retain Photoshop APP13 profiles, including IPTC, and JPEG comments. These JPEG segments stay in the JPEG container.

Transforms and conversions between AVIF and HEIC sequences keep each displayed frame’s ICC profile, EXIF and XMP. ICC profiles follow the visual sample descriptions. EXIF and XMP follow linked metadata tracks on the presentation timeline. Conversions to animated PNG, WebP or JPEG XL use their shared metadata fields.

## Storing images {#storing-images}

`store` generates a filename using the encoded extension. Use `storeAs` to supply the filename, or `storePublicly` and `storePubliclyAs` to request public visibility. These methods return the stored path when the disk reports success.

`storeAs("avatar.png", None, None).await?` treats the first argument as the complete filename. Names with a supplied directory and name are trimmed only at the outside of the combined path.

## Inspecting images {#inspecting-images}

Use `mimeType`, `extension`, `dimensions`, `width`, and `height` to inspect the processed result. `dominantColor` returns the alpha-weighted average RGB color as a hexadecimal string.

## Image drivers {#image-drivers}

The application uses the `image` driver by default. `IMAGE_DRIVER` selects a registered driver. Use `Image::extend` to register an implementation of `Driver`, and `using` to choose a driver for an image. The default limits are 32 MiB per input, 16,384 pixels per side, and 24 million pixels per image.

The built-in driver reads and writes JPEG, PNG, GIF, WebP, AVIF, HEIC, JPEG XL, and BMP. Use `toAvif()`, `toHeic()`, `toJxl()`, or `toFormat("heif")` to convert a source. The `heif` alias produces a `.heic` file with MIME type `image/heic`. JPEG, WebP, AVIF, HEIC, and JPEG XL use the requested quality. WebP and JPEG XL use lossless encoding at quality 100. These codecs run in Rust.

### Custom transformation handlers {#custom-transformation-handlers}

Use `Image::transformUsing(driver, name, callback)?` or the same method on `ImageManager` to register a handler for one driver. Registration also works before an application registers that driver with `extend`; manager `extend` returns a `Result`. The driver contract exposes `transformUsing` and `dimensions`. Drivers without handler support return `UnsupportedTransformationHandler`. `Image::driver(None)?`, `getDefaultDriver()` and `getDrivers()` expose the configured driver and registered instances.

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

Built-in handlers use the transformation variant's exact name, such as `Resize` or `Grayscale`, and override that built-in operation. Custom transformations use their supplied name; use an application namespace to avoid collisions. The callback receives the actual driver object as `&mut dyn Any`; the built-in driver uses `NativeImage`. An error stops the pipeline. Native animation callbacks run on each processed frame, and modified frames must still satisfy the configured limits.

`usingGd()` and `usingImagick()` select application-registered `gd` and `imagick` drivers. The framework ships the native `image` backend; selecting an unavailable backend returns `UnknownDriver` when a transformation needs it. Reading unchanged bytes does not resolve the selected backend.
