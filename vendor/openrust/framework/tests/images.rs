use std::{io::Cursor, path::PathBuf, sync::Arc};

use ::image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use axum::body::Bytes;
use openrust::image::{
    Background, Driver, EncodedImage, Error, Format, ImageManager, ImagePipeline, Limits,
    Transformation,
};

fn sample() -> Vec<u8> {
    let colors = RgbaImage::from_fn(12, 6, |x, _| {
        if x < 6 {
            Rgba([255, 0, 0, 255])
        } else {
            Rgba([0, 0, 255, 255])
        }
    });
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(colors)
        .write_to(&mut bytes, ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

#[tokio::test]
async fn decodes_real_images_and_encodes_each_supported_output() {
    let manager = ImageManager::new(Limits::default());
    let png = sample();
    let source = manager.fromBytes(png.clone());
    assert_eq!(source.toBytes().await.unwrap(), png);
    assert_eq!(source.dominantColor().await.unwrap(), "#800080");
    assert_eq!(source.dimensions().await.unwrap(), (12, 6));
    let webp = source.clone().toWebp();
    let low = webp.clone().quality(1);
    let high = webp.quality(100);
    let (low, high) = tokio::join!(low.toBytes(), high.toBytes());
    assert_ne!(low.unwrap(), high.unwrap());

    for format in [
        Format::Jpeg,
        Format::Png,
        Format::Gif,
        Format::Webp,
        Format::Avif,
        Format::Bmp,
    ] {
        let image = source.clone().cover(5, 5).encode(format).quality(81);
        let encoded = image.toBytes().await.unwrap();
        assert_eq!(
            Format::try_from(::image::guess_format(&encoded).unwrap()).unwrap(),
            format
        );
        assert_eq!(image.extension().await.unwrap(), format.extension());
        assert_eq!(image.dimensions().await.unwrap(), (5, 5));
        if format != Format::Avif {
            let decoded = manager.fromBytes(encoded);
            assert_eq!(decoded.dimensions().await.unwrap(), (5, 5));
        }
    }
    assert_eq!(
        source
            .clone()
            .resize(Some(6), None)
            .dimensions()
            .await
            .unwrap(),
        (6, 3)
    );
    assert_eq!(
        source
            .clone()
            .scale(Some(40), Some(40))
            .dimensions()
            .await
            .unwrap(),
        (12, 6)
    );
    assert_eq!(
        source
            .clone()
            .crop(6, 6, 6, 0)
            .dominantColor()
            .await
            .unwrap(),
        "#0000ff"
    );
    let contained = source.clone().contain(12, 12, Background::Dominant).toPng();
    let bytes = contained.toBytes().await.unwrap();
    let pixels = ::image::load_from_memory(&bytes).unwrap().into_rgba8();
    assert_eq!(pixels.get_pixel(0, 0).0, [128, 0, 128, 255]);
    let rotated = source
        .clone()
        .rotate(90.0, Background::Transparent)
        .flipHorizontally();
    assert_eq!(rotated.dimensions().await.unwrap(), (6, 12));
    let rotated = source
        .clone()
        .rotate(45.0, Background::Color([255, 255, 255, 255]));
    assert_eq!(rotated.dimensions().await.unwrap(), (13, 13));
    let encoded = source
        .clone()
        .grayscale()
        .blur(5)
        .sharpen(3)
        .toPng()
        .toBase64()
        .await
        .unwrap();
    assert_eq!(
        manager.fromBase64(encoded).dimensions().await.unwrap(),
        (12, 6)
    );
    assert!(
        source
            .optimize(None, None)
            .toDataUri()
            .await
            .unwrap()
            .starts_with("data:image/webp;base64,")
    );
}

#[tokio::test]
async fn reads_exif_orientation_and_checks_inputs_and_limits() {
    use ::image::ImageEncoder as _;
    let mut jpeg = Vec::new();
    let mut encoder = ::image::codecs::jpeg::JpegEncoder::new(&mut jpeg);
    encoder
        .set_exif_metadata(vec![
            b'I', b'I', 42, 0, 8, 0, 0, 0, // little-endian TIFF header
            1, 0, 0x12, 1, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, // orientation 6
            0, 0, 0, 0,
        ])
        .unwrap();
    DynamicImage::new_rgb8(8, 3)
        .write_with_encoder(encoder)
        .unwrap();
    let manager = ImageManager::new(Limits::default());
    let image = manager.fromBytes(jpeg);
    assert_eq!(image.dimensions().await.unwrap(), (8, 3));
    assert_eq!(image.orient().dimensions().await.unwrap(), (3, 8));

    let png = sample();
    let limited = ImageManager::new(Limits {
        max_input_bytes: png.len() - 1,
        ..Limits::default()
    });
    assert!(matches!(
        limited.fromBytes(png.clone()).toBytes().await,
        Err(Error::InputTooLarge)
    ));
    assert!(matches!(
        limited.fromStream(png.as_slice()).await,
        Err(Error::InputTooLarge)
    ));
    let limited = ImageManager::new(Limits {
        max_pixels: 50,
        ..Limits::default()
    });
    assert!(matches!(
        limited.fromBytes(png.clone()).toBytes().await,
        Err(Error::DimensionsTooLarge)
    ));
    assert!(matches!(
        manager
            .fromBytes(png.clone())
            .cover(50_000, 50_000)
            .toBytes()
            .await,
        Err(Error::DimensionsTooLarge)
    ));
    assert!(matches!(
        manager.fromBytes(png).crop(3, 3, 13, 0).toBytes().await,
        Err(Error::InvalidDimensions)
    ));
    assert!(matches!(
        manager
            .fromBytes(b"not an image".as_slice())
            .toBytes()
            .await,
        Err(Error::UnsupportedFormat)
    ));
    assert!(matches!(
        manager.fromBase64("not base64").toBytes().await,
        Err(Error::Base64(_))
    ));
}

#[tokio::test]
async fn applies_custom_driver_and_keeps_manager_registrations_isolated() {
    struct Probe;
    #[allow(non_snake_case)]
    impl Driver for Probe {
        fn process(
            &self,
            contents: Bytes,
            pipeline: &ImagePipeline,
            _: Limits,
        ) -> openrust::image::Result<EncodedImage> {
            assert!(
                matches!(pipeline.transformations.as_slice(), [Transformation::Custom { name, .. }] if name == "probe")
            );
            Ok(EncodedImage {
                bytes: contents,
                format: Format::Png,
                dimensions: (1, 1),
            })
        }
        fn dominantColor(
            &self,
            _: Bytes,
            _: &ImagePipeline,
            _: Limits,
        ) -> openrust::image::Result<String> {
            Ok("#abcdef".into())
        }
    }
    let left = ImageManager::new(Limits::default());
    let right = ImageManager::new(Limits::default());
    left.extend("probe", Probe);
    let image = left
        .fromBytes(sample())
        .using("probe")
        .transform(Transformation::Custom {
            name: "probe".into(),
            options: serde_json::Value::Null,
        });
    assert_eq!(image.dimensions().await.unwrap(), (1, 1));
    assert_eq!(image.dominantColor().await.unwrap(), "#abcdef");
    assert!(matches!(
        right.driver(Some("probe")),
        Err(Error::UnknownDriver(_))
    ));
}

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "openrust-image-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn transforms_a_real_http_upload_and_reads_it_from_storage_and_url() {
    use axum::{Router, extract::State};
    use openrust::{
        contracts::http::Kernel as _,
        foundation::{Application, configuration::Routing, http::Kernel},
        http::{
            HttpError, HttpFuture, Method, Request, Response, StatusCode,
            client::{Factory, Part},
        },
        support::facades::{Image, Route, Storage},
    };
    use serde_json::{Value, json};

    fn upload(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let image = request
                .image("avatar")
                .await?
                .ok_or(HttpError::InvalidRequest)?;
            assert_eq!(image.file().unwrap().getClientOriginalName(), "sample.txt");
            let image = image.cover(5, 5).toWebp();
            let path = image
                .storePublicly("avatars", Some("public"))
                .await
                .map_err(|_| HttpError::InvalidRequest)?
                .unwrap();
            let restored = Storage::disk(Some("public"))?.image(&path).unwrap();
            assert_eq!(restored.dimensions().await.unwrap(), (5, 5));
            let restored = Image::fromStorage(&path, Some("public")).unwrap();
            assert_eq!(restored.mimeType().await.unwrap(), "image/webp");
            Response::json(
                StatusCode::CREATED,
                json!({"path": path, "dimensions": [5, 5]}),
            )
        })
    }

    async fn transport(
        State(kernel): State<Arc<Kernel>>,
        incoming: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handleTransport(incoming).await
    }

    let directory = Directory::new();
    let application = Application::configure(directory.0.clone())
        .withConfig(|base, _| {
            Ok(std::collections::BTreeMap::from([("filesystems".into(), json!({
                "default": "public",
                "disks": {"public": {
                    "driver": "local", "root": base.join("public"), "url": "/storage", "visibility": "public", "throw": true
                }}
            }))]))
        })
        .withRouting(Routing::default().web(|| {
            Route::staticDisk("public")?;
            Route::r#match(&[Method::POST], "/upload", upload)
        }))
        .create().unwrap();
    let mut kernel = Kernel::new(application);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new()
        .fallback(transport)
        .with_state(Arc::new(kernel));
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let client = Factory::new().unwrap();
    let endpoint = format!("{url}/upload");
    let response = client
        .pendingRequest()
        .attach(
            "avatar",
            Part::bytes(sample())
                .file_name("sample.txt")
                .mime_str("text/plain")
                .unwrap(),
        )
        .send(Method::POST, &endpoint)
        .await
        .unwrap();
    assert!(
        response.created(),
        "{}",
        String::from_utf8_lossy(response.body())
    );
    let data = response.json::<Value>().unwrap();
    let path = data["path"].as_str().unwrap();
    assert!(path.starts_with("avatars/") && path.ends_with(".webp"));
    assert_eq!(data["dimensions"], json!([5, 5]));
    let manager = ImageManager::new(Limits::default());
    let restored = manager.fromUrl(format!("{url}/storage/{path}")).unwrap();
    assert_eq!(restored.dimensions().await.unwrap(), (5, 5));
    let restored = manager.fromPath(directory.0.join("public").join(path));
    assert_eq!(restored.dimensions().await.unwrap(), (5, 5));
    let response = client
        .pendingRequest()
        .attach(
            "avatar",
            Part::bytes(b"broken".to_vec()).file_name("sample.txt"),
        )
        .send(Method::POST, &endpoint)
        .await
        .unwrap();
    assert!(response.badRequest());
    server.abort();
}
