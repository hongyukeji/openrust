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

## Storing images {#storing-images}

`store` generates a filename using the encoded extension. Use `storeAs` to supply the filename, or `storePublicly` and `storePubliclyAs` to request public visibility. These methods return the stored path when the disk reports success.

## Inspecting images {#inspecting-images}

Use `mimeType`, `extension`, `dimensions`, `width`, and `height` to inspect the processed result. `dominantColor` returns the alpha-weighted average RGB color as a hexadecimal string.

## Image drivers {#image-drivers}

The application uses the `image` driver by default. `IMAGE_DRIVER` selects a registered driver. Use `Image::extend` to register an implementation of `Driver`, and `using` to choose a driver for an image. The default limits are 32 MiB per input, 16,384 pixels per side, and 24 million pixels per image.

JPEG, PNG, GIF, WebP, and BMP support reading and writing. AVIF supports writing. JPEG, WebP, and AVIF use the requested quality. Transformations on animated inputs operate on a single frame.
