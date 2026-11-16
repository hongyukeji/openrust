# Image Manipulation

## Reading images {#reading-images}

In an application, import `Image` from `openrust::support::facades`. Use `request.image("avatar").await?` to create an image from a multipart field, or call `Image::fromUpload` with an existing upload. `Image::fromStorage` and `Storage::disk(Some("public"))?.image(path)` use configured disks. Other inputs include bytes, Base64, paths, HTTP URLs, and asynchronous streams. Streams are read at construction. Other sources are loaded when rendering or inspecting the image.

## Manipulating and encoding images {#manipulating-images}

Transformations run in the order they are added. `resize` takes optional width and height. `scale` preserves aspect ratio without enlarging the image. `cover` crops to fill, while `contain` preserves the whole image and fills empty space. Other operations include crop, EXIF orientation, rotation, blur, grayscale, sharpen, and flips. Clone the pipeline to branch it. Use `toBytes().await?`, `toBase64().await?`, or `toDataUri().await?` to retrieve the result.

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

## Storing images {#storing-images}

`store` generates a filename using the encoded extension. Use `storeAs` to supply the filename, or `storePublicly` and `storePubliclyAs` to request public visibility. These methods return the stored path when the disk reports success.

## Inspecting images {#inspecting-images}

Use `mimeType`, `extension`, `dimensions`, `width`, and `height` to inspect the processed result. `dominantColor` returns the alpha-weighted average RGB color as a hexadecimal string.

## Image drivers {#image-drivers}

The application uses the `image` driver by default. `IMAGE_DRIVER` selects a registered driver. Use `Image::extend` to register an implementation of `Driver`, and `using` to choose a driver for an image. The default limits are 32 MiB per input, 16,384 pixels per side, and 24 million pixels per image.

The built-in driver reads and writes JPEG, PNG, GIF, WebP, AVIF, HEIC, JPEG XL, and BMP. Use `toAvif()`, `toHeic()`, `toJxl()`, or `toFormat("heif")` to convert a source. The `heif` alias produces a `.heic` file with MIME type `image/heic`. JPEG, WebP, AVIF, HEIC, and JPEG XL use the requested quality. WebP and JPEG XL use lossless encoding at quality 100. These codecs run in Rust.
