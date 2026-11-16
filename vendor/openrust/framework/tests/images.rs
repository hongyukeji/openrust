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
        Format::Heic,
        Format::Jxl,
        Format::Bmp,
    ] {
        let image = source.clone().cover(5, 5).encode(format).quality(81);
        let encoded = image.toBytes().await.unwrap();
        assert_eq!(image.extension().await.unwrap(), format.extension());
        assert_eq!(image.dimensions().await.unwrap(), (5, 5));
        let decoded = manager.fromBytes(encoded);
        assert_eq!(decoded.mimeType().await.unwrap(), format.mime_type());
        assert_eq!(decoded.dimensions().await.unwrap(), (5, 5));
        let pixels = decoded.toFormat("png").unwrap().toBytes().await.unwrap();
        let pixels = ::image::load_from_memory(&pixels).unwrap().into_rgba8();
        let left = pixels.get_pixel(0, 2);
        let right = pixels.get_pixel(4, 2);
        assert!(left[0] > left[2] && right[2] > right[0], "{format:?}");
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
async fn recent_codecs_apply_limits_and_reject_incomplete_contents() {
    let manager = ImageManager::new(Limits::default());
    let input = manager.fromBytes(sample());
    let limited = ImageManager::new(Limits {
        max_pixels: 50,
        ..Limits::default()
    });
    for converted in [
        input.clone().toAvif(),
        input.clone().toFormat("heif").unwrap(),
        input.toJxl(),
    ] {
        let encoded = converted.toBytes().await.unwrap();
        assert!(matches!(
            limited.fromBytes(encoded.clone()).dimensions().await,
            Err(Error::DimensionsTooLarge)
        ));
        assert!(
            manager
                .fromBytes(encoded.slice(..encoded.len() / 2))
                .toBytes()
                .await
                .is_err()
        );
    }
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

fn gif_sample(repeat: Option<gif::Repeat>) -> Vec<u8> {
    let mut gif = gif::Encoder::new(Vec::new(), 4, 3, &[255, 0, 0, 0, 255, 0, 0, 0, 255]).unwrap();
    if let Some(repeat) = repeat {
        gif.set_repeat(repeat).unwrap();
    }
    for (width, height, left, delay, index, dispose) in [
        (4, 3, 0, 4, 0, gif::DisposalMethod::Keep),
        (2, 3, 0, 9, 1, gif::DisposalMethod::Previous),
        (2, 3, 2, 18, 2, gif::DisposalMethod::Keep),
    ] {
        gif.write_frame(&gif::Frame {
            width,
            height,
            left,
            delay,
            buffer: vec![index; usize::from(width * height)].into(),
            dispose,
            ..Default::default()
        })
        .unwrap();
    }
    gif.into_inner().unwrap()
}

fn frames_for(bytes: &[u8], format: Format) -> Vec<::image::Frame> {
    use ::image::AnimationDecoder as _;
    let input = Cursor::new(bytes);
    let frames = match format {
        Format::Gif => ::image::codecs::gif::GifDecoder::new(input)
            .unwrap()
            .into_frames(),
        Format::Png => ::image::codecs::png::PngDecoder::new(input)
            .unwrap()
            .apng()
            .unwrap()
            .into_frames(),
        Format::Webp => ::image::codecs::webp::WebPDecoder::new(input)
            .unwrap()
            .into_frames(),
        _ => unreachable!(),
    };
    frames.collect_frames().unwrap()
}

#[tokio::test]
async fn preserves_composited_frames_delays_and_finite_repeats_across_animation_formats() {
    let manager = ImageManager::new(Limits::default());
    let original = gif_sample(Some(gif::Repeat::Finite(2)));
    let source = manager.fromBytes(original.clone());
    assert_eq!(source.toBytes().await.unwrap(), original);
    assert_eq!(source.dominantColor().await.unwrap(), "#ff0000");
    let render = source
        .flipHorizontally()
        .resize(Some(8), Some(6))
        .quality(100);
    let mut outputs = Vec::new();
    for format in [Format::Gif, Format::Png, Format::Webp] {
        let output = render.clone().encode(format).toBytes().await.unwrap();
        let frames = frames_for(&output, format);
        assert_eq!(frames.len(), 3, "{format:?}");
        assert_eq!(
            frames
                .iter()
                .map(|f| {
                    assert_eq!(f.buffer().dimensions(), (8, 6));
                    let (numerator, denominator) = f.delay().numer_denom_ms();
                    numerator / denominator
                })
                .collect::<Vec<_>>(),
            [40, 90, 180]
        );
        let pixel = |frame: usize, x| frames[frame].buffer().get_pixel(x, 2).0;
        assert_eq!(pixel(0, 7), [255, 0, 0, 255]);
        assert!(pixel(1, 7)[1] > 235, "{format:?}");
        assert!(pixel(2, 7)[0] > 235, "{format:?}");
        assert!(pixel(2, 0)[2] > 235, "{format:?}");
        outputs.push(output);
    }
    assert_eq!(
        gif::DecodeOptions::new()
            .read_info(outputs[0].as_ref())
            .unwrap()
            .repeat(),
        gif::Repeat::Finite(2)
    );
    assert_eq!(
        png::Decoder::new(Cursor::new(&outputs[1]))
            .read_info()
            .unwrap()
            .info()
            .animation_control()
            .unwrap()
            .num_plays,
        3
    );
    assert_eq!(
        oxideav_webp::animation_params(&outputs[2])
            .unwrap()
            .unwrap()
            .0,
        3
    );

    let jpeg = manager
        .fromBytes(outputs[2].clone())
        .toJpeg()
        .toBytes()
        .await
        .unwrap();
    let image = ::image::load_from_memory(&jpeg).unwrap().into_rgb8();
    assert!(
        image
            .pixels()
            .all(|pixel| pixel[0] > pixel[1] && pixel[0] > pixel[2])
    );
}

#[tokio::test]
async fn preserves_rational_apng_delays_and_distinguishes_once_from_infinite() {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, 2, 2);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_animated(2, 0).unwrap();
    let mut writer = encoder.write_header().unwrap();
    writer.set_frame_delay(1, 60).unwrap();
    writer
        .write_image_data(&[240, 25, 50, 255].repeat(4))
        .unwrap();
    writer.set_frame_delay(7, 113).unwrap();
    writer.write_image_data(&[0, 0, 0, 0].repeat(4)).unwrap();
    writer.finish().unwrap();

    let manager = ImageManager::new(Limits::default());
    let source = manager.fromBytes(bytes);
    let apng = source
        .clone()
        .flipVertically()
        .toPng()
        .toBytes()
        .await
        .unwrap();
    let frames = frames_for(&apng, Format::Png);
    for (frame, (expected_n, expected_d)) in frames.iter().zip([(1, 60), (7, 113)]) {
        let (n, d) = frame.delay().numer_denom_ms();
        assert_eq!(n * expected_d, d * expected_n * 1000);
    }
    let lossless = source
        .clone()
        .toWebp()
        .quality(100)
        .toBytes()
        .await
        .unwrap();
    let lossy = source.toWebp().quality(10).toBytes().await.unwrap();
    assert_ne!(lossless, lossy);
    let frames = frames_for(&lossless, Format::Webp);
    assert_eq!(frames[0].delay().numer_denom_ms(), (17, 1));
    assert_eq!(frames[1].delay().numer_denom_ms(), (62, 1));
    assert!(frames[1].buffer().pixels().all(|pixel| pixel[3] == 0));
    assert_eq!(
        oxideav_webp::animation_params(&lossless)
            .unwrap()
            .unwrap()
            .0,
        0
    );
    assert_eq!(frames_for(&lossy, Format::Webp).len(), 2);

    let once = manager
        .fromBytes(gif_sample(None))
        .toWebp()
        .toBytes()
        .await
        .unwrap();
    assert_eq!(oxideav_webp::animation_params(&once).unwrap().unwrap().0, 1);
    let forever = manager
        .fromBytes(gif_sample(Some(gif::Repeat::Infinite)))
        .toWebp()
        .toBytes()
        .await
        .unwrap();
    assert_eq!(
        oxideav_webp::animation_params(&forever).unwrap().unwrap().0,
        0
    );
}

#[tokio::test]
async fn bounds_animation_work_and_rejects_damage_after_the_first_frame() {
    let sample = gif_sample(None);
    let limited = ImageManager::new(Limits {
        max_pixels: 24,
        ..Limits::default()
    });
    let mut gif = gif::Encoder::new(Vec::new(), 4, 3, &[]).unwrap();
    for _ in 0..8 {
        let frame = gif::Frame::from_rgb(4, 3, &[220, 20, 0].repeat(12));
        gif.write_frame(&frame).unwrap();
    }
    let eight_frames = gif.into_inner().unwrap();
    let source = limited.fromBytes(eight_frames.clone());
    assert!(source.toBytes().await.is_ok());
    assert!(matches!(
        source.resize(Some(8), Some(3)).toWebp().toBytes().await,
        Err(Error::AnimationTooLarge)
    ));
    let smaller = ImageManager::new(Limits {
        max_pixels: 12,
        ..Limits::default()
    });
    assert!(matches!(
        smaller.fromBytes(eight_frames).toBytes().await,
        Err(Error::AnimationTooLarge)
    ));
    let manager = ImageManager::new(Limits::default());
    assert!(
        manager
            .fromBytes(sample[..sample.len() - 7].to_vec())
            .toBytes()
            .await
            .is_err()
    );
}

#[tokio::test]
async fn jpeg_xl_quality_controls_lossy_encoding_and_preserves_alpha_at_full_quality() {
    let pixels = RgbaImage::from_fn(32, 28, |x, y| {
        Rgba([(x * 7 + y) as u8, (y * 9) as u8, (x * 5 + y * 2) as u8, 235])
    });
    let mut source = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(pixels.clone())
        .write_to(&mut source, ImageFormat::Png)
        .unwrap();
    let manager = ImageManager::new(Limits::default());
    let source = manager.fromBytes(source.into_inner()).toJxl();
    let lossless = source.clone().quality(100).toBytes().await.unwrap();
    let lossy = source.quality(30).toBytes().await.unwrap();
    assert_ne!(lossless, lossy);
    let restored = manager.fromBytes(lossless).toPng().toBytes().await.unwrap();
    assert_eq!(
        ::image::load_from_memory(&restored).unwrap().into_rgba8(),
        pixels
    );
    let restored = manager.fromBytes(lossy).toPng().toBytes().await.unwrap();
    let restored = ::image::load_from_memory(&restored).unwrap().into_rgba8();
    assert_ne!(restored, pixels);
    assert!(restored.pixels().all(|pixel| pixel[3] == 235));
}

#[tokio::test]
async fn keeps_timing_for_a_single_frame_animation() {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, 2, 2);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_animated(1, 4).unwrap();
    let mut writer = encoder.write_header().unwrap();
    writer.set_frame_delay(5, 100).unwrap();
    writer
        .write_image_data(&[128, 64, 20, 255].repeat(4))
        .unwrap();
    writer.finish().unwrap();

    let manager = ImageManager::new(Limits::default());
    let source = manager.fromBytes(bytes);
    for format in [Format::Webp, Format::Avif, Format::Heic, Format::Jxl] {
        let encoded = source.clone().encode(format).toBytes().await.unwrap();
        let webp = manager.fromBytes(encoded).toWebp().toBytes().await.unwrap();
        assert_eq!(oxideav_webp::animation_params(&webp).unwrap().unwrap().0, 4);
        let frames = frames_for(&webp, Format::Webp);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].delay().numer_denom_ms(), (50, 1));
    }
}

#[tokio::test]
async fn avif_heic_and_jpeg_xl_sequence_round_trips() {
    let manager = ImageManager::new(Limits::default());
    for (repeat, plays) in [
        (Some(gif::Repeat::Finite(2)), 3),
        (None, 1),
        (Some(gif::Repeat::Infinite), 0),
    ] {
        let source = manager
            .fromBytes(gif_sample(repeat))
            .resize(Some(16), Some(12));
        for format in [Format::Avif, Format::Heic, Format::Jxl] {
            let bytes = source
                .clone()
                .encode(format)
                .quality(100)
                .toBytes()
                .await
                .unwrap_or_else(|error| panic!("{format:?} encode: {error}"));
            let restored = manager.fromBytes(bytes);
            assert_eq!(restored.mimeType().await.unwrap(), format.mime_type());
            let webp = restored
                .flipHorizontally()
                .toWebp()
                .quality(100)
                .toBytes()
                .await
                .unwrap_or_else(|error| panic!("{format:?} decode: {error}"));
            assert_eq!(
                oxideav_webp::animation_params(&webp).unwrap().unwrap().0,
                plays
            );
            let frames = frames_for(&webp, Format::Webp);
            assert_eq!(frames.len(), 3, "{format:?}");
            assert_eq!(
                frames
                    .iter()
                    .map(|f| f.delay().numer_denom_ms().0)
                    .collect::<Vec<_>>(),
                [40, 90, 180]
            );
            assert!(frames[0].buffer().get_pixel(0, 0)[0] > 200);
            assert!(frames[1].buffer().get_pixel(15, 0)[1] > 200);
            assert!(frames[2].buffer().get_pixel(0, 0)[2] > 200);
        }
    }
}

#[tokio::test]
async fn preserves_exif_icc_and_xmp_and_resets_applied_orientation() {
    let manager = ImageManager::new(Limits::default());
    let jxl = manager.fromBytes(sample()).toJxl().toBytes().await.unwrap();
    let profile = jxl_oxide::JxlImage::read_with_defaults(Cursor::new(jxl))
        .unwrap()
        .rendered_icc();
    let exif = [
        b'I', b'I', 42, 0, 8, 0, 0, 0, // TIFF header
        1, 0, 0x12, 1, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, // Orientation
        0, 0, 0, 0,
    ];
    let xmp = "<x:xmpmeta xmlns:x='adobe:ns:meta/'>original</x:xmpmeta>";
    for animated in [false, true] {
        let mut bytes = Vec::new();
        let mut info = png::Info::with_size(16, 12);
        info.color_type = png::ColorType::Rgba;
        info.icc_profile = Some(std::borrow::Cow::Borrowed(&profile));
        info.exif_metadata = Some(std::borrow::Cow::Borrowed(&exif));
        let mut encoder = png::Encoder::with_info(&mut bytes, info).unwrap();
        encoder
            .add_itxt_chunk("XML:com.adobe.xmp".into(), xmp.into())
            .unwrap();
        if animated {
            encoder.set_animated(2, 3).unwrap();
        }
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&[220, 0, 0, 255].repeat(16 * 12))
            .unwrap();
        if animated {
            writer
                .write_image_data(&[0, 220, 0, 255].repeat(16 * 12))
                .unwrap();
        }
        writer.finish().unwrap();
        let source = manager.fromBytes(bytes).orient();
        for format in [
            Format::Jpeg,
            Format::Png,
            Format::Webp,
            Format::Avif,
            Format::Heic,
            Format::Jxl,
        ] {
            let output = source
                .clone()
                .encode(format)
                .quality(100)
                .toBytes()
                .await
                .unwrap_or_else(|error| panic!("{format:?} encode: {error}"));
            if animated && matches!(format, Format::Avif | Format::Heic) {
                let (header, _) = oxideav_heif::boxes::find_box(&output, b"meta")
                    .unwrap()
                    .unwrap();
                let mut track_only = output.to_vec();
                // Keep the media offsets while removing the cover item and its metadata.
                track_only[header.start + 4..header.start + 8].copy_from_slice(b"free");
                let track_only = manager
                    .fromBytes(track_only)
                    .toWebp()
                    .quality(100)
                    .toBytes()
                    .await
                    .unwrap();
                let track_metadata = oxideav_webp::read_metadata(&track_only).unwrap();
                assert_eq!(track_metadata.icc.as_deref(), Some(profile.as_slice()));
                assert_eq!(frames_for(&track_only, Format::Webp).len(), 2);
                if format == Format::Avif {
                    let mut oriented = exif;
                    oriented[18] = 1;
                    assert_eq!(track_metadata.exif.as_deref(), Some(oriented.as_slice()));
                    assert_eq!(track_metadata.xmp.as_deref(), Some(xmp.as_bytes()));
                }
            }
            let webp = manager
                .fromBytes(output)
                .toWebp()
                .quality(100)
                .toBytes()
                .await
                .unwrap_or_else(|error| panic!("{format:?} restore: {error}"));
            let metadata = oxideav_webp::read_metadata(&webp).unwrap();
            assert_eq!(
                metadata.icc.as_deref(),
                Some(profile.as_slice()),
                "{format:?}"
            );
            let mut oriented = exif;
            oriented[18] = 1;
            assert_eq!(
                metadata.exif.as_deref(),
                Some(oriented.as_slice()),
                "{format:?}"
            );
            assert_eq!(metadata.xmp.as_deref(), Some(xmp.as_bytes()), "{format:?}");
        }
    }
}

#[tokio::test]
async fn preserves_jpeg_iptc_and_comments_after_a_transform() {
    use ::image::{ImageDecoder, codecs::jpeg::JpegDecoder};

    let manager = ImageManager::new(Limits::default());
    let jpeg = manager
        .fromBytes(sample())
        .toJpeg()
        .toBytes()
        .await
        .unwrap();
    let photoshop = b"8BIM\x04\x04\0\0\0\0\0\x09\x1c\x02\x05\0\x04note\0";
    let mut profiles = Vec::new();
    for (marker, payload) in [
        (0xfe, b"first comment".to_vec()),
        (0xed, [b"Photoshop 3.0\0".as_slice(), photoshop].concat()),
        (0xfe, b"last comment".to_vec()),
    ] {
        profiles.extend_from_slice(&[0xff, marker]);
        profiles.extend_from_slice(&u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
        profiles.extend_from_slice(&payload);
    }
    let input = [jpeg[..2].as_ref(), profiles.as_slice(), &jpeg[2..]].concat();
    let image = manager.fromBytes(input).resize(Some(12), Some(16));
    let output = image.clone().toJpeg().toBytes().await.unwrap();
    let mut decoder = JpegDecoder::new(Cursor::new(&output)).unwrap();
    assert_eq!(decoder.dimensions(), (12, 16));
    assert_eq!(
        decoder.iptc_metadata().unwrap().as_deref(),
        Some(photoshop.as_slice())
    );
    assert_eq!(
        output
            .windows(profiles.len())
            .filter(|part| *part == profiles)
            .count(),
        1
    );
    let png = image.toPng().toBytes().await.unwrap();
    assert!(!png.windows(photoshop.len()).any(|part| part == photoshop));
}

#[tokio::test]
async fn preserves_external_heif_metadata_on_the_original_display_frames() {
    use oxideav_heif::{
        HeifFile,
        props::Colr,
        sequence::{LoopBehaviour, MetadataTrackKind, parse_movie, sample_bytes},
    };

    fn read_frames(contents: &[u8], repeat: LoopBehaviour) -> Vec<[Vec<u8>; 3]> {
        let file = HeifFile::parse_borrowed(contents).unwrap();
        let movie = parse_movie(&file).unwrap().unwrap();
        let track = movie.visual_tracks().next().unwrap();
        let linked = movie.metadata_tracks_of(track.track_id);
        assert_eq!(linked.len(), 2);
        assert_eq!(track.loop_behaviour(), repeat);
        let mut pictures: Vec<_> = track.samples.iter().collect();
        pictures.sort_by_key(|sample| sample.pts());
        assert_eq!(pictures.len(), 3);
        pictures
            .into_iter()
            .zip([40, 90, 180])
            .enumerate()
            .map(|(index, (sample, delay))| {
                assert_eq!(sample.duration * 1000, delay * track.timescale);
                let entry = &track.sample_entries[sample.description_index as usize - 1];
                let profile = entry
                    .colr
                    .iter()
                    .find_map(|color| match color {
                        Colr::Icc { profile, .. } => Some(profile),
                        _ => None,
                    })
                    .unwrap();
                assert_eq!(&profile[48..52], format!("FRM{index}").as_bytes());
                let get_timed = |kind| {
                    let metadata = linked
                        .iter()
                        .find(|metadata| metadata.metadata_kind() == Some(kind))
                        .unwrap();
                    let timed = &metadata.samples[index];
                    assert_eq!(
                        timed.pts() * u64::from(track.timescale),
                        sample.pts() * u64::from(metadata.timescale)
                    );
                    assert_eq!(timed.duration * 1000, delay * metadata.timescale);
                    sample_bytes(&file, timed).unwrap().to_vec()
                };
                let exif = get_timed(MetadataTrackKind::Exif);
                let artist = format!("frame {index} artist");
                assert!(
                    exif.windows(artist.len())
                        .any(|bytes| bytes == artist.as_bytes())
                );
                let xmp = get_timed(MetadataTrackKind::Xmp);
                assert_eq!(
                    xmp,
                    format!("<x:xmpmeta xmlns:x='adobe:ns:meta/' id='frame {index}'/>").as_bytes()
                );
                [
                    profile.clone(),
                    oxideav_heif::meta::exif_payload(&exif).unwrap(),
                    xmp,
                ]
            })
            .collect()
    }

    let manager = ImageManager::new(Limits::default());
    for (fixture, original, loops, plays) in [
        (
            include_bytes!("fixtures/heif_sequences/frame-metadata.avif").as_slice(),
            Format::Avif,
            LoopBehaviour::Times(3),
            3,
        ),
        (
            include_bytes!("fixtures/heif_sequences/frame-metadata.heic").as_slice(),
            Format::Heic,
            LoopBehaviour::Forever,
            0,
        ),
    ] {
        let input = read_frames(fixture, loops);
        for target in [Format::Avif, Format::Heic] {
            let converted = manager
                .fromBytes(fixture.to_vec())
                .resize(Some(20), Some(14))
                .flipHorizontally()
                .encode(target)
                .quality(100)
                .toBytes()
                .await
                .unwrap_or_else(|error| panic!("{original:?} to {target:?}: {error}"));
            assert_eq!(read_frames(&converted, loops), input);
            let source = manager.fromBytes(converted);
            assert_eq!(source.dimensions().await.unwrap(), (20, 14));
            let restored = source
                .clone()
                .encode(original)
                .quality(100)
                .toBytes()
                .await
                .unwrap();
            assert_eq!(read_frames(&restored, loops), input);
            let webp = source.toWebp().quality(100).toBytes().await.unwrap();
            assert_eq!(
                oxideav_webp::animation_params(&webp).unwrap().unwrap().0,
                plays
            );
            for (index, frame) in frames_for(&webp, Format::Webp).iter().enumerate() {
                let pixel = frame.buffer().get_pixel(9, 6);
                assert!(
                    pixel[index] > 190,
                    "{original:?} to {target:?}, frame {index}: {pixel:?}"
                );
                assert_eq!(frame.delay().numer_denom_ms(), ([40, 90, 180][index], 1));
            }
        }
    }
}
