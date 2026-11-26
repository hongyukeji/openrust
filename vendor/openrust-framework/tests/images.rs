use std::{io::Cursor, path::PathBuf, sync::Arc};

use ::image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use axum::body::Bytes;
use openrust::image::{
    Background, Driver, EncodedImage, Error, Format, ImageManager, ImagePipeline, Limits,
    Transformation,
};

fn image_reference() -> serde_json::Value {
    serde_json::from_str(include_str!("reference/image-lifecycle.json")).unwrap()
}

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
    let source = manager.from_bytes(png.clone());
    assert_eq!(source.to_bytes().await.unwrap(), png);
    assert_eq!(source.dominant_color().await.unwrap(), "#800080");
    assert_eq!(source.dimensions().await.unwrap(), (12, 6));
    let webp = source.clone().to_webp();
    let low = webp.clone().quality(1);
    let high = webp.quality(100);
    let (low, high) = tokio::join!(low.to_bytes(), high.to_bytes());
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
        let encoded = image.to_bytes().await.unwrap();
        assert_eq!(image.extension().await.unwrap(), format.extension());
        assert_eq!(image.dimensions().await.unwrap(), (5, 5));
        let decoded = manager.from_bytes(encoded);
        assert_eq!(decoded.mime_type().await.unwrap(), format.mime_type());
        assert_eq!(decoded.dimensions().await.unwrap(), (5, 5));
        let pixels = decoded.to_format("png").unwrap().to_bytes().await.unwrap();
        let pixels = ::image::load_from_memory(&pixels).unwrap().into_rgba8();
        let left = pixels.get_pixel(0, 2);
        let right = pixels.get_pixel(4, 2);
        assert!(left[0] > left[2] && right[2] > right[0], "{format:?}");
    }
    assert_eq!(
        source
            .clone()
            .resize(Some(6), None)
            .unwrap()
            .dimensions()
            .await
            .unwrap(),
        (6, 3)
    );
    assert_eq!(
        source
            .clone()
            .scale(Some(40), Some(40))
            .unwrap()
            .dimensions()
            .await
            .unwrap(),
        (12, 6)
    );
    assert_eq!(
        source
            .clone()
            .crop(6, 6, 6, 0)
            .dominant_color()
            .await
            .unwrap(),
        "#0000ff"
    );
    let contained = source
        .clone()
        .contain(12, 12, Background::Dominant)
        .to_png();
    let bytes = contained.to_bytes().await.unwrap();
    let pixels = ::image::load_from_memory(&bytes).unwrap().into_rgba8();
    assert_eq!(pixels.get_pixel(0, 0).0, [128, 0, 128, 255]);
    let rotated = source
        .clone()
        .rotate(90.0, Background::Transparent)
        .flip_horizontally();
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
        .to_png()
        .to_base64()
        .await
        .unwrap();
    assert_eq!(
        manager.from_base64(encoded).dimensions().await.unwrap(),
        (12, 6)
    );
    assert!(
        source
            .optimize(None, None)
            .to_data_uri()
            .await
            .unwrap()
            .starts_with("data:image/webp;base64,")
    );
}

#[tokio::test]
async fn recent_codecs_apply_limits_and_reject_incomplete_contents() {
    let manager = ImageManager::new(Limits::default());
    let input = manager.from_bytes(sample());
    let limited = ImageManager::new(Limits {
        max_pixels: 50,
        ..Limits::default()
    });
    for converted in [
        input.clone().to_avif(),
        input.clone().to_format("heif").unwrap(),
        input.to_jxl(),
    ] {
        let encoded = converted.to_bytes().await.unwrap();
        assert!(matches!(
            limited.from_bytes(encoded.clone()).dimensions().await,
            Err(Error::DimensionsTooLarge)
        ));
        assert!(
            manager
                .from_bytes(encoded.slice(..encoded.len() / 2))
                .to_bytes()
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
    let image = manager.from_bytes(jpeg.clone());
    assert_eq!(image.dimensions().await.unwrap(), (8, 3));
    assert_eq!(image.orient().dimensions().await.unwrap(), (3, 8));
    // 覆盖 Orient 后没有执行内置旋转，不能擅自将 EXIF 方向改成正向。
    manager
        .transform_using("image", "Orient", Arc::new(|_, _| Ok(())))
        .unwrap();
    let unrotated = manager.from_bytes(jpeg).orient().to_bytes().await.unwrap();
    let mut decoder = ::image::codecs::jpeg::JpegDecoder::new(Cursor::new(unrotated)).unwrap();
    use ::image::ImageDecoder as _;
    assert_eq!(
        decoder.orientation().unwrap(),
        ::image::metadata::Orientation::Rotate90
    );

    let png = sample();
    let limited = ImageManager::new(Limits {
        max_input_bytes: png.len() - 1,
        ..Limits::default()
    });
    assert!(matches!(
        limited.from_bytes(png.clone()).to_bytes().await,
        Err(Error::InputTooLarge)
    ));
    assert!(matches!(
        limited.from_stream(png.as_slice()).await,
        Err(Error::InputTooLarge)
    ));
    let limited = ImageManager::new(Limits {
        max_pixels: 50,
        ..Limits::default()
    });
    assert!(matches!(
        limited.from_bytes(png.clone()).to_bytes().await,
        Err(Error::DimensionsTooLarge)
    ));
    assert!(matches!(
        manager
            .from_bytes(png.clone())
            .cover(50_000, 50_000)
            .to_bytes()
            .await,
        Err(Error::DimensionsTooLarge)
    ));
    assert!(matches!(
        manager.from_bytes(png).crop(3, 3, 13, 0).to_bytes().await,
        Err(Error::InvalidDimensions)
    ));
    assert!(matches!(
        manager
            .from_bytes(b"not an image".as_slice())
            .to_bytes()
            .await,
        Err(Error::UnsupportedFormat)
    ));
    assert!(matches!(
        manager.from_base64("not base64").to_bytes().await,
        Err(Error::Base64(_))
    ));
}

#[tokio::test]
async fn applies_custom_driver_and_keeps_manager_registrations_isolated() {
    struct Probe;
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
        fn dominant_color(
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
    left.extend("probe", |_| Ok(Arc::new(Probe))).unwrap();
    assert!(matches!(
        left.driver(Some("probe"))
            .unwrap()
            .transform_using("Probe", Arc::new(|_, _| Ok(()))),
        Err(Error::UnsupportedTransformationHandler)
    ));
    let image = left
        .from_bytes(sample())
        .using("probe")
        .transform(Transformation::Custom {
            name: "probe".into(),
            options: serde_json::Value::Null,
        });
    assert_eq!(image.dimensions().await.unwrap(), (1, 1));
    assert_eq!(image.dominant_color().await.unwrap(), "#abcdef");
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
            assert_eq!(
                image.file().unwrap().get_client_original_name(),
                "sample.txt"
            );
            let image = image.cover(5, 5).to_webp();
            let path = image
                .store_publicly("avatars", Some("public"))
                .await
                .map_err(|_| HttpError::InvalidRequest)?
                .unwrap();
            let reference = image_reference();
            let transformed = image.clone().cover(3, 3);
            let encoded = image.clone().to_png();
            let selected = image.clone().using("image");
            for branch in [&image, &transformed, &encoded, &selected] {
                assert_eq!(
                    branch.file().map(|file| file.get_client_original_name()),
                    reference["upload_original_name"].as_str(),
                );
                assert_eq!(
                    branch.file().is_some(),
                    reference["upload_file_preserved"].as_bool().unwrap(),
                );
            }
            assert_eq!(transformed.dimensions().await.unwrap(), (3, 3));
            assert_eq!(encoded.mime_type().await.unwrap(), "image/png");
            assert_eq!(selected.dimensions().await.unwrap(), (5, 5));
            assert_eq!(image.dimensions().await.unwrap(), (5, 5));
            assert!(Image::from_bytes(sample()).unwrap().file().is_none());
            let restored = Storage::disk(Some("public"))?.image(&path).unwrap();
            assert_eq!(restored.dimensions().await.unwrap(), (5, 5));
            let restored = Image::from_storage(&path, Some("public")).unwrap();
            assert_eq!(restored.mime_type().await.unwrap(), "image/webp");
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
        kernel.handle_transport(incoming).await
    }

    let directory = Directory::new();
    let application = Application::configure(directory.0.clone())
        .with_config(|base, _| {
            Ok(std::collections::BTreeMap::from([("filesystems".into(), json!({
                "default": "public",
                "disks": {"public": {
                    "driver": "local", "root": base.join("public"), "url": "/storage", "visibility": "public", "throw": true
                }}
            }))]))
        })
        .with_routing(Routing::default().web(|| {
            Route::static_disk("public")?;
            Route::post("/upload", upload)?;
            Ok(())
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
        .pending_request()
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
        String::from_utf8_lossy(&response.body().unwrap())
    );
    let data = response.json::<Value>().unwrap();
    let path = data["path"].as_str().unwrap();
    assert!(path.starts_with("avatars/") && path.ends_with(".webp"));
    assert_eq!(data["dimensions"], json!([5, 5]));
    let manager = ImageManager::new(Limits::default());
    let restored = manager.from_url(format!("{url}/storage/{path}")).unwrap();
    assert_eq!(restored.dimensions().await.unwrap(), (5, 5));
    let restored = manager.from_path(directory.0.join("public").join(path));
    assert_eq!(restored.dimensions().await.unwrap(), (5, 5));
    let response = client
        .pending_request()
        .attach(
            "avatar",
            Part::bytes(b"broken".to_vec()).file_name("sample.txt"),
        )
        .send(Method::POST, &endpoint)
        .await
        .unwrap();
    assert!(response.bad_request());
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
    let source = manager.from_bytes(original.clone());
    assert_eq!(source.to_bytes().await.unwrap(), original);
    assert_eq!(source.dominant_color().await.unwrap(), "#ff0000");
    let render = source
        .flip_horizontally()
        .resize(Some(8), Some(6))
        .unwrap()
        .quality(100);
    let mut outputs = Vec::new();
    for format in [Format::Gif, Format::Png, Format::Webp] {
        let output = render.clone().encode(format).to_bytes().await.unwrap();
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
        .from_bytes(outputs[2].clone())
        .to_jpeg()
        .to_bytes()
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
    let source = manager.from_bytes(bytes);
    let apng = source
        .clone()
        .flip_vertically()
        .to_png()
        .to_bytes()
        .await
        .unwrap();
    let frames = frames_for(&apng, Format::Png);
    for (frame, (expected_n, expected_d)) in frames.iter().zip([(1, 60), (7, 113)]) {
        let (n, d) = frame.delay().numer_denom_ms();
        assert_eq!(n * expected_d, d * expected_n * 1000);
    }
    let lossless = source
        .clone()
        .to_webp()
        .quality(100)
        .to_bytes()
        .await
        .unwrap();
    let lossy = source.to_webp().quality(10).to_bytes().await.unwrap();
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
        .from_bytes(gif_sample(None))
        .to_webp()
        .to_bytes()
        .await
        .unwrap();
    assert_eq!(oxideav_webp::animation_params(&once).unwrap().unwrap().0, 1);
    let forever = manager
        .from_bytes(gif_sample(Some(gif::Repeat::Infinite)))
        .to_webp()
        .to_bytes()
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
    let source = limited.from_bytes(eight_frames.clone());
    assert!(source.to_bytes().await.is_ok());
    assert!(matches!(
        source
            .resize(Some(8), Some(3))
            .unwrap()
            .to_webp()
            .to_bytes()
            .await,
        Err(Error::AnimationTooLarge)
    ));
    let smaller = ImageManager::new(Limits {
        max_pixels: 12,
        ..Limits::default()
    });
    assert!(matches!(
        smaller.from_bytes(eight_frames).to_bytes().await,
        Err(Error::AnimationTooLarge)
    ));
    let manager = ImageManager::new(Limits::default());
    assert!(
        manager
            .from_bytes(sample[..sample.len() - 7].to_vec())
            .to_bytes()
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
    let source = manager.from_bytes(source.into_inner()).to_jxl();
    let lossless = source.clone().quality(100).to_bytes().await.unwrap();
    let lossy = source.quality(30).to_bytes().await.unwrap();
    assert_ne!(lossless, lossy);
    let restored = manager
        .from_bytes(lossless)
        .to_png()
        .to_bytes()
        .await
        .unwrap();
    assert_eq!(
        ::image::load_from_memory(&restored).unwrap().into_rgba8(),
        pixels
    );
    let restored = manager.from_bytes(lossy).to_png().to_bytes().await.unwrap();
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
    let source = manager.from_bytes(bytes);
    for format in [Format::Webp, Format::Avif, Format::Heic, Format::Jxl] {
        let encoded = source.clone().encode(format).to_bytes().await.unwrap();
        let webp = manager
            .from_bytes(encoded)
            .to_webp()
            .to_bytes()
            .await
            .unwrap();
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
            .from_bytes(gif_sample(repeat))
            .resize(Some(16), Some(12))
            .unwrap();
        for format in [Format::Avif, Format::Heic, Format::Jxl] {
            let bytes = source
                .clone()
                .encode(format)
                .quality(100)
                .to_bytes()
                .await
                .unwrap_or_else(|error| panic!("{format:?} encode: {error}"));
            let restored = manager.from_bytes(bytes);
            assert_eq!(restored.mime_type().await.unwrap(), format.mime_type());
            let webp = restored
                .flip_horizontally()
                .to_webp()
                .quality(100)
                .to_bytes()
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
    let jxl = manager
        .from_bytes(sample())
        .to_jxl()
        .to_bytes()
        .await
        .unwrap();
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
        let source = manager.from_bytes(bytes).orient();
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
                .to_bytes()
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
                    .from_bytes(track_only)
                    .to_webp()
                    .quality(100)
                    .to_bytes()
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
                .from_bytes(output)
                .to_webp()
                .quality(100)
                .to_bytes()
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
        .from_bytes(sample())
        .to_jpeg()
        .to_bytes()
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
    let image = manager
        .from_bytes(input)
        .resize(Some(12), Some(16))
        .unwrap();
    let output = image.clone().to_jpeg().to_bytes().await.unwrap();
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
    let png = image.to_png().to_bytes().await.unwrap();
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
                .from_bytes(fixture.to_vec())
                .resize(Some(20), Some(14))
                .unwrap()
                .flip_horizontally()
                .encode(target)
                .quality(100)
                .to_bytes()
                .await
                .unwrap_or_else(|error| panic!("{original:?} to {target:?}: {error}"));
            assert_eq!(read_frames(&converted, loops), input);
            let source = manager.from_bytes(converted);
            assert_eq!(source.dimensions().await.unwrap(), (20, 14));
            let restored = source
                .clone()
                .encode(original)
                .quality(100)
                .to_bytes()
                .await
                .unwrap();
            assert_eq!(read_frames(&restored, loops), input);
            let webp = source.to_webp().quality(100).to_bytes().await.unwrap();
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

#[tokio::test]
async fn flip_and_flop_follow_vertical_and_horizontal_pixel_directions() {
    let pixels = RgbaImage::from_fn(2, 2, |x, y| Rgba([(x + 2 * y) as u8 * 50, 0, 0, 255]));
    let mut encoded = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(pixels.clone())
        .write_to(&mut encoded, ImageFormat::Png)
        .unwrap();
    let manager = ImageManager::new(Limits::default());
    let image = manager.from_bytes(encoded.into_inner());
    let vertical = ::image::load_from_memory(&image.clone().flip().to_bytes().await.unwrap())
        .unwrap()
        .into_rgba8();
    let horizontal = ::image::load_from_memory(&image.flop().to_bytes().await.unwrap())
        .unwrap()
        .into_rgba8();
    assert_eq!(vertical, ::image::imageops::flip_vertical(&pixels));
    assert_eq!(horizontal, ::image::imageops::flip_horizontal(&pixels));
}

#[tokio::test]
async fn native_driver_handlers_modify_real_pixels_in_pipeline_order_and_override_builtins() {
    use openrust::image::NativeImage;
    use std::sync::Mutex;
    let manager = ImageManager::new(Limits::default());
    let other = ImageManager::new(Limits::default());
    let driver = manager.driver(None).unwrap();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let seen = observed.clone();
    driver
        .transform_using(
            "Probe",
            Arc::new(move |image, transformation| {
                let Transformation::Custom { options, .. } = transformation else {
                    panic!("wrong transformation")
                };
                assert_eq!(options["value"], 9);
                let image = image
                    .downcast_mut::<NativeImage>()
                    .ok_or(Error::InvalidTransformation)?;
                assert_eq!(image.width(), 12);
                *image = image.fliph();
                seen.lock().unwrap().push("Probe");
                Ok(())
            }),
        )
        .unwrap();
    let seen = observed.clone();
    driver
        .transform_using(
            "Resize",
            Arc::new(move |image, transformation| {
                assert!(matches!(
                    transformation,
                    Transformation::Resize(Some(3), Some(2))
                ));
                let image = image
                    .downcast_mut::<NativeImage>()
                    .ok_or(Error::InvalidTransformation)?;
                *image = image.crop_imm(0, 0, 3, 2);
                seen.lock().unwrap().push("Resize");
                Ok(())
            }),
        )
        .unwrap();
    let custom = || Transformation::Custom {
        name: "Probe".into(),
        options: serde_json::json!({"value":9}),
    };
    let image = manager
        .from_bytes(sample())
        .transform(custom())
        .resize(Some(3), Some(2))
        .unwrap();
    let output = image.to_bytes().await.unwrap();
    assert_eq!(image.to_bytes().await.unwrap(), output);
    assert_eq!(*observed.lock().unwrap(), ["Probe", "Resize"]);
    let pixels = ::image::load_from_memory(&output).unwrap().into_rgba8();
    assert_eq!(pixels.dimensions(), (3, 2));
    assert_eq!(pixels.get_pixel(0, 0), &Rgba([0, 0, 255, 255]));
    assert!(matches!(
        other
            .from_bytes(sample())
            .transform(custom())
            .to_bytes()
            .await,
        Err(Error::InvalidTransformation)
    ));
    assert_eq!(
        driver
            .dimensions(sample().into(), Limits::default())
            .unwrap(),
        (12, 6)
    );
}

#[tokio::test]
async fn custom_handlers_propagate_errors_enforce_limits_and_process_animation_frames() {
    use openrust::image::NativeImage;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let manager = ImageManager::new(Limits {
        max_dimension: 20,
        ..Limits::default()
    });
    let driver = manager.driver(None).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    driver
        .transform_using(
            "Probe",
            Arc::new(move |image, _| {
                let image = image
                    .downcast_mut::<NativeImage>()
                    .ok_or(Error::InvalidTransformation)?;
                *image = image.fliph();
                seen.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }),
        )
        .unwrap();
    let custom = || Transformation::Custom {
        name: "Probe".into(),
        options: serde_json::Value::Null,
    };
    let animated = manager
        .from_bytes(gif_sample(Some(gif::Repeat::Infinite)))
        .transform(custom());
    let frames = frames_for(&animated.to_bytes().await.unwrap(), Format::Gif);
    assert_eq!(frames.len(), 3);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    driver
        .transform_using(
            "Probe",
            Arc::new(|_, _| Err(Error::Io(std::io::Error::other("转换失败")))),
        )
        .unwrap();
    let error = manager
        .from_bytes(sample())
        .transform(custom())
        .to_bytes()
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Io(ref error) if error.to_string()=="转换失败"));
    driver
        .transform_using(
            "Probe",
            Arc::new(|image, _| {
                *image
                    .downcast_mut::<NativeImage>()
                    .ok_or(Error::InvalidTransformation)? = NativeImage::new_rgba8(21, 1);
                Ok(())
            }),
        )
        .unwrap();
    assert!(matches!(
        manager
            .from_bytes(sample())
            .transform(custom())
            .to_bytes()
            .await,
        Err(Error::DimensionsTooLarge)
    ));
    assert!(
        matches!(manager.from_bytes(sample()).using_gd().cover(1,1).to_bytes().await,Err(Error::UnknownDriver(name)) if name=="gd")
    );
    assert!(
        matches!(manager.from_bytes(sample()).using_imagick().cover(1,1).to_bytes().await,Err(Error::UnknownDriver(name)) if name=="imagick")
    );
}

#[tokio::test]
async fn handler_can_register_a_later_pipeline_handler_without_holding_the_registry_lock() {
    let manager = ImageManager::new(Limits::default());
    let driver = manager.driver(None).unwrap();
    let weak = Arc::downgrade(&driver);
    driver
        .transform_using(
            "First",
            Arc::new(move |_, _| {
                weak.upgrade()
                    .unwrap()
                    .transform_using("Second", Arc::new(|_, _| Err(Error::InvalidTransformation)))
            }),
        )
        .unwrap();
    let output = manager
        .from_bytes(sample())
        .transform(Transformation::Custom {
            name: "First".into(),
            options: serde_json::Value::Null,
        })
        .transform(Transformation::Custom {
            name: "Second".into(),
            options: serde_json::Value::Null,
        })
        .to_bytes()
        .await;
    assert!(matches!(output, Err(Error::InvalidTransformation)));
}

#[tokio::test]
async fn lazy_source_is_shared_and_processed_branches_do_not_repeat_prior_transformations() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let manager = ImageManager::new(Limits::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    manager
        .transform_using(
            "image",
            "Probe",
            Arc::new(move |_, _| {
                seen.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }),
        )
        .unwrap();
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), sample()).unwrap();
    let source = manager.from_path(file.path());
    let processed = source.clone().transform(Transformation::Custom {
        name: "Probe".into(),
        options: serde_json::Value::Null,
    });
    let first = processed.to_bytes().await.unwrap();
    std::fs::write(file.path(), b"changed after first successful load").unwrap();
    assert_eq!(processed.dominant_color().await.unwrap(), "#800080");
    assert_eq!(processed.dominant_color().await.unwrap(), "#800080");
    assert_eq!(source.flop().dimensions().await.unwrap(), (12, 6));
    assert_eq!(processed.clone().to_png().to_bytes().await.unwrap(), first);
    assert_eq!(
        calls.load(Ordering::SeqCst) as u64,
        image_reference()["custom_calls"].as_u64().unwrap()
    );
    let base_name = processed.hash_name("").await.unwrap();
    assert_eq!(
        base_name.len() as u64,
        image_reference()["hash_length"].as_u64().unwrap()
    );
    assert_eq!(
        processed.hash_name("avatars").await.unwrap(),
        format!("avatars/{base_name}")
    );
    assert_eq!(processed.hash_name("0").await.unwrap(), base_name);
    assert_eq!(
        processed.clone().to_png().hash_name("").await.unwrap(),
        base_name
    );
    assert_eq!(
        processed.to_string().await.unwrap(),
        processed.to_data_uri().await.unwrap()
    );
}

#[tokio::test]
async fn manager_retains_handlers_before_custom_driver_registration() {
    struct Forward(Arc<dyn Driver>);
    impl Driver for Forward {
        fn process(
            &self,
            bytes: Bytes,
            pipeline: &ImagePipeline,
            limits: Limits,
        ) -> openrust::image::Result<EncodedImage> {
            self.0.process(bytes, pipeline, limits)
        }
        fn dominant_color(
            &self,
            bytes: Bytes,
            pipeline: &ImagePipeline,
            limits: Limits,
        ) -> openrust::image::Result<String> {
            self.0.dominant_color(bytes, pipeline, limits)
        }
        fn transform_using(
            &self,
            name: &str,
            callback: openrust::image::TransformationHandler,
        ) -> openrust::image::Result<()> {
            self.0.transform_using(name, callback)
        }
    }
    let manager = ImageManager::new(Limits::default());
    assert_eq!(manager.get_default_driver(), "image");
    manager
        .transform_using(
            "custom",
            "Probe",
            Arc::new(|_, _| Err(Error::InvalidTransformation)),
        )
        .unwrap();
    manager
        .extend("custom", |_| {
            Ok(Arc::new(Forward(
                ImageManager::new(Limits::default()).driver(None)?,
            )))
        })
        .unwrap();
    assert!(!manager.get_drivers().contains_key("custom"));
    manager.driver(Some("custom")).unwrap();
    assert!(manager.get_drivers().contains_key("custom"));
    let result = manager
        .from_bytes(sample())
        .using("custom")
        .transform(Transformation::Custom {
            name: "Probe".into(),
            options: serde_json::Value::Null,
        })
        .to_bytes()
        .await;
    assert!(matches!(result, Err(Error::InvalidTransformation)));
    manager.forget_drivers();
    assert!(matches!(
        manager
            .from_bytes(sample())
            .using("custom")
            .transform(Transformation::Custom {
                name: "Probe".into(),
                options: serde_json::Value::Null,
            })
            .to_bytes()
            .await,
        Err(Error::InvalidTransformation)
    ));
    manager
        .transform_using("custom", "Probe", Arc::new(|_, _| Ok(())))
        .unwrap();
    assert!(
        manager
            .from_bytes(sample())
            .using("custom")
            .transform(Transformation::Custom {
                name: "Probe".into(),
                options: serde_json::Value::Null
            })
            .to_bytes()
            .await
            .is_ok()
    );
}

fn image_response(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::image::Image, openrust::http::HttpError>> {
    Box::pin(async {
        let facade = &openrust::support::facades::Image::get_default_driver().unwrap();
        assert_eq!(facade, "image");
        openrust::support::facades::Config::set("images.default", "missing".into()).unwrap();
        assert_eq!(
            openrust::support::facades::Image::get_default_driver().unwrap(),
            "missing"
        );
        assert!(
            matches!(openrust::support::facades::Image::driver(None), Err(Error::UnknownDriver(name)) if name=="missing")
        );
        openrust::support::facades::Config::set("images.default", "image".into()).unwrap();
        openrust::support::facades::Image::transform_using(
            "image",
            "Probe",
            Arc::new(|_, _| Ok(())),
        )
        .unwrap();
        assert!(
            openrust::support::facades::Image::get_drivers()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            openrust::support::facades::Image::driver(None)
                .unwrap()
                .dimensions(sample().into(), Limits::default())
                .unwrap(),
            (12, 6)
        );
        let owner = openrust::support::facades::Image::get_container()
            .unwrap()
            .unwrap();
        openrust::support::facades::Image::extend("facade-probe", |manager| {
            assert!(manager.get_container().is_some());
            manager.driver(Some("image"))
        })
        .unwrap();
        assert!(
            !openrust::support::facades::Image::get_drivers()
                .unwrap()
                .contains_key("facade-probe")
        );
        openrust::support::facades::Image::driver(Some("facade-probe")).unwrap();
        let other = Arc::new(openrust::container::Container::default());
        openrust::support::facades::Image::set_container(other.clone()).unwrap();
        assert!(Arc::ptr_eq(
            &other,
            &openrust::support::facades::Image::get_container()
                .unwrap()
                .unwrap()
        ));
        openrust::support::facades::Image::forget_drivers().unwrap();
        assert!(
            openrust::support::facades::Image::get_drivers()
                .unwrap()
                .is_empty()
        );
        openrust::support::facades::Image::set_container(owner).unwrap();
        let image = openrust::support::facades::Image::from_bytes(sample())
            .unwrap()
            .transform(Transformation::Custom {
                name: "Probe".into(),
                options: serde_json::Value::Null,
            })
            .cover(3, 3);
        let first = image.store("avatars", None).await.unwrap();
        assert_eq!(image.store("avatars", None).await.unwrap(), first);
        assert_eq!(first, Some(image.hash_name("avatars").await.unwrap()));
        assert_eq!(
            image.store_as("/single.png/", None, None).await.unwrap(),
            image_reference()["store_null"].as_str().map(str::to_owned)
        );
        assert_eq!(
            image
                .store_as("/avatars/", "/named.png/", None)
                .await
                .unwrap(),
            image_reference()["store_slashes"]
                .as_str()
                .map(str::to_owned)
        );
        assert_eq!(
            image
                .store_publicly_as("/public.png/", None, None)
                .await
                .unwrap(),
            image_reference()["store_public_null"]
                .as_str()
                .map(str::to_owned)
        );
        Ok(image)
    })
}
#[tokio::test]
async fn image_facade_and_responsable_return_real_image_response() {
    use openrust::{foundation::configuration::Routing, support::facades::Route, testing::TestApp};
    let mut app = TestApp::create(|builder| {
        builder.with_config(|base, _|Ok(std::collections::BTreeMap::from([("filesystems".into(),serde_json::json!({"default":"test","disks":{"test":{"driver":"local","root":base.join("images"),"throw":true}}}))]))).with_routing(Routing::default().web(|| {
            Route::get("/image", image_response)?;
            Ok(())
        }))
    })
    .unwrap();
    let response = app.get("/image").await.unwrap();
    response
        .assert_ok()
        .assert_header("content-type", "image/png");
    let pixels = ::image::load_from_memory(response.body()).unwrap();
    assert_eq!((pixels.width(), pixels.height()), (3, 3));
}

#[tokio::test]
async fn image_builders_reject_missing_dimensions_and_normalize_bounds_before_handlers() {
    let manager = ImageManager::new(Limits::default());
    assert!(matches!(
        manager.from_bytes(sample()).resize(None, None),
        Err(Error::InvalidDimensions)
    ));
    assert!(matches!(
        manager.from_bytes(sample()).scale(None, None),
        Err(Error::InvalidDimensions)
    ));
    assert_eq!(
        manager
            .from_bytes(sample())
            .resize(Some(0), Some(0))
            .unwrap()
            .dimensions()
            .await
            .unwrap(),
        (1, 1)
    );
    assert_eq!(
        manager
            .from_bytes(sample())
            .cover(0, 0)
            .dimensions()
            .await
            .unwrap(),
        (1, 1)
    );
    assert_eq!(
        manager
            .from_bytes(sample())
            .contain(0, 0, Background::Transparent)
            .dimensions()
            .await
            .unwrap(),
        (1, 1)
    );
    assert_eq!(
        manager
            .from_bytes(sample())
            .crop(0, 0, 0, 0)
            .dimensions()
            .await
            .unwrap(),
        (1, 1)
    );
    manager
        .transform_using(
            "image",
            "Blur",
            Arc::new(|_, step| {
                assert!(matches!(step, Transformation::Blur(100)));
                Ok(())
            }),
        )
        .unwrap();
    manager
        .transform_using(
            "image",
            "Sharpen",
            Arc::new(|_, step| {
                assert!(matches!(step, Transformation::Sharpen(100)));
                Ok(())
            }),
        )
        .unwrap();
    manager
        .from_bytes(sample())
        .blur(255)
        .sharpen(255)
        .to_bytes()
        .await
        .unwrap();
    // 未进行转换时不会为了读取原始字节去解析尚未安装的驱动。
    assert_eq!(
        manager
            .from_bytes(sample())
            .using_gd()
            .to_bytes()
            .await
            .unwrap(),
        sample()
    );
}

#[test]
fn image_manager_creators_are_lazy_cached_and_recreated_after_forgetting() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("reference/image-manager.json")).unwrap();
    let manager = ImageManager::new(Limits::default());
    assert_eq!(manager.get_drivers().is_empty(), reference["initial_empty"]);
    let creates = Arc::new(AtomicUsize::new(0));
    let observed = creates.clone();
    manager
        .extend("custom", move |manager| {
            observed.fetch_add(1, Ordering::SeqCst);
            manager.driver(Some("image"))
        })
        .unwrap();
    assert_eq!(
        creates.load(Ordering::SeqCst),
        reference["calls_after_extend"].as_u64().unwrap() as usize
    );
    let first = manager.driver(Some("custom")).unwrap();
    assert!(Arc::ptr_eq(
        &first,
        &manager.driver(Some("custom")).unwrap()
    ));
    assert_eq!(
        creates.load(Ordering::SeqCst),
        reference["calls_after_cached"].as_u64().unwrap() as usize
    );
    let replacement = Arc::new(AtomicUsize::new(0));
    let observed = replacement.clone();
    manager
        .extend("custom", move |manager| {
            observed.fetch_add(1, Ordering::SeqCst);
            manager.driver(Some("image"))
        })
        .unwrap();
    assert!(Arc::ptr_eq(
        &first,
        &manager.driver(Some("custom")).unwrap()
    ));
    assert_eq!(
        replacement.load(Ordering::SeqCst),
        reference["replacement_calls_before_forget"]
            .as_u64()
            .unwrap() as usize
    );
    manager.forget_drivers();
    assert!(manager.get_drivers().is_empty());
    let second = manager.driver(Some("custom")).unwrap();
    assert!(!Arc::ptr_eq(&first, &second));
    assert_eq!(
        replacement.load(Ordering::SeqCst),
        reference["replacement_calls_after_forget"]
            .as_u64()
            .unwrap() as usize
    );
    assert!(Arc::ptr_eq(
        &manager.driver(Some("")).unwrap(),
        &manager.driver(Some("0")).unwrap()
    ));
    assert!(Arc::ptr_eq(&manager.driver(None).unwrap(), &second));
}

#[test]
fn image_manager_creator_errors_and_recursive_resolution_do_not_poison_cache() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let manager = ImageManager::new(Limits::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    manager
        .extend("retry", move |manager| {
            if observed.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(Error::InvalidTransformation);
            }
            manager.driver(None)
        })
        .unwrap();
    assert!(matches!(
        manager.driver(Some("retry")),
        Err(Error::InvalidTransformation)
    ));
    assert!(manager.get_drivers().is_empty());
    assert!(manager.driver(Some("retry")).is_ok());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    manager
        .extend("recursive", |manager| manager.driver(Some("recursive")))
        .unwrap();
    assert!(
        matches!(manager.driver(Some("recursive")), Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock)
    );
    manager
        .extend("recursive", |manager| manager.driver(None))
        .unwrap();
    assert!(manager.driver(Some("recursive")).is_ok());
}

#[test]
fn image_manager_rebinding_changes_creator_container_and_preserves_resolved_drivers() {
    use openrust::container::Container;
    let manager = ImageManager::new(Limits::default());
    assert!(manager.get_container().is_none());
    let left = Arc::new(Container::default());
    let right = Arc::new(Container::default());
    manager.set_container(left.clone());
    assert!(Arc::ptr_eq(&manager.get_container().unwrap(), &left));
    let observed = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = observed.clone();
    manager
        .extend("custom", move |manager| {
            seen.lock().unwrap().push(manager.get_container().unwrap());
            manager.driver(None)
        })
        .unwrap();
    let first = manager.driver(Some("custom")).unwrap();
    manager.set_container(right.clone());
    assert!(Arc::ptr_eq(&manager.get_container().unwrap(), &right));
    assert!(Arc::ptr_eq(
        &first,
        &manager.driver(Some("custom")).unwrap()
    ));
    manager.forget_drivers();
    assert!(manager.driver(Some("custom")).is_ok());
    let observations = observed.lock().unwrap();
    assert_eq!(observations.len(), 2);
    assert!(Arc::ptr_eq(&observations[0], &left));
    assert!(Arc::ptr_eq(&observations[1], &right));
}

#[tokio::test]
async fn image_manager_rebinding_resolves_storage_and_http_when_sources_are_loaded() {
    use openrust::{
        config::Repository,
        container::Container,
        contracts::container::Container as _,
        filesystem::FilesystemManager,
        http::{
            HeaderMap, StatusCode,
            client::{Factory, Response},
        },
    };
    fn source_container(root: &std::path::Path, bytes: &[u8]) -> Arc<Container> {
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join("input.png"), bytes).unwrap();
        let config = Arc::new(Repository::new(std::collections::BTreeMap::from([(
            "filesystems".into(),
            serde_json::json!({
                "default":"local", "disks":{"local":{"driver":"local", "root":root, "throw":true}}
            }),
        )])));
        let container = Arc::new(Container::default());
        container
            .instance_named(
                "filesystem",
                Arc::new(FilesystemManager::new(config, root.to_owned())),
            )
            .unwrap();
        let http = Arc::new(Factory::new().unwrap());
        http.stub_url(
            "example.test/*",
            Response::new(StatusCode::OK, HeaderMap::new(), bytes.to_vec()),
        );
        http.prevent_stray_requests(true);
        container.instance(http).unwrap();
        container
    }
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("reference/image-manager.json")).unwrap();
    for key in [
        "storage_loaded_from_rebound",
        "url_loaded_from_rebound",
        "storage_loaded_source_cached",
        "url_loaded_source_cached",
    ] {
        assert_eq!(reference[key], true);
    }
    let root = Directory::new();
    let small = ImageManager::new(Limits::default())
        .from_bytes(sample())
        .cover(2, 2)
        .to_bytes()
        .await
        .unwrap();
    let left = source_container(&root.0.join("left"), &sample());
    let right = source_container(&root.0.join("right"), &small);
    let manager = ImageManager::new(Limits::default());
    manager.set_container(left.clone());
    let storage = manager.from_storage("input.png", None).unwrap();
    let http = manager.from_url("https://example.test/input.png").unwrap();
    manager.set_container(right.clone());
    assert_eq!(storage.dimensions().await.unwrap(), (2, 2));
    assert_eq!(http.dimensions().await.unwrap(), (2, 2));
    manager.set_container(left.clone());
    assert_eq!(storage.dimensions().await.unwrap(), (2, 2));
    assert_eq!(http.dimensions().await.unwrap(), (2, 2));
    assert_eq!(
        manager
            .from_storage("input.png", None)
            .unwrap()
            .dimensions()
            .await
            .unwrap(),
        (12, 6)
    );
    assert_eq!(
        manager
            .from_url("https://example.test/input.png")
            .unwrap()
            .dimensions()
            .await
            .unwrap(),
        (12, 6)
    );
}

#[tokio::test]
async fn image_driver_registration_in_provider_boot_uses_default_config_and_application_container()
{
    use openrust::{
        contracts::{container::Container as _, foundation::Application as _},
        foundation::{Application, configuration::Routing},
        http::{HttpError, HttpFuture, Request},
        support::{
            ProviderResult, ServiceProvider,
            facades::{Image, Route},
        },
        testing::TestApp,
    };
    struct ImageSettings(u32);
    struct ThumbnailImages(u32, Arc<std::sync::atomic::AtomicUsize>);
    impl ServiceProvider for ThumbnailImages {
        fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
            Box::pin(async move {
                app.container().instance(Arc::new(ImageSettings(self.0)))?;
                let count = self.1.clone();
                let expected = self.0;
                Image::extend("thumbnail", move |manager| {
                    let container = manager.get_container().unwrap();
                    assert_eq!(container.make::<ImageSettings>().unwrap().0, expected);
                    count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    manager.driver(Some("image"))
                })?;
                Ok(())
            })
        }
    }
    fn thumbnail(_: &mut Request) -> HttpFuture<'_, Result<openrust::image::Image, HttpError>> {
        Box::pin(async {
            let container = Image::get_container().unwrap().unwrap();
            let width = container.make::<ImageSettings>().unwrap().0;
            Ok(Image::from_bytes(sample()).unwrap().cover(width, width))
        })
    }
    fn app(width: u32, calls: Arc<std::sync::atomic::AtomicUsize>) -> TestApp {
        TestApp::create(|builder| {
            builder
                .with_config(|_, _| {
                    Ok(std::collections::BTreeMap::from([(
                        "images".into(),
                        serde_json::json!({"default":"thumbnail"}),
                    )]))
                })
                .with_providers(vec![Box::new(ThumbnailImages(width, calls))])
                .with_routing(Routing::default().web(|| {
                    Route::get("/thumbnail", thumbnail)?;
                    Ok(())
                }))
        })
        .unwrap()
    }
    let left_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let right_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut left = app(3, left_calls.clone());
    let mut right = app(4, right_calls.clone());
    assert_eq!(left_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(right_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    for _ in 0..2 {
        let (left, right) = tokio::join!(left.get("/thumbnail"), right.get("/thumbnail"));
        for (response, width) in [(left.unwrap(), 3), (right.unwrap(), 4)] {
            response
                .assert_ok()
                .assert_header("content-type", "image/png");
            let pixels = ::image::load_from_memory(response.body()).unwrap();
            assert_eq!((pixels.width(), pixels.height()), (width, width));
        }
    }
    assert_eq!(left_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(right_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn driver_requirements_run_before_handlers_and_only_cache_successful_creations() {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Checked {
        inner: Arc<dyn Driver>,
        ready: Arc<AtomicBool>,
        order: Arc<std::sync::Mutex<Vec<&'static str>>>,
    }
    impl Driver for Checked {
        fn ensure_requirements_are_met(&self) -> openrust::image::Result<()> {
            self.order.lock().unwrap().push("requirements");
            if self.ready.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(Error::Missing)
            }
        }
        fn transform_using(
            &self,
            _: &str,
            _: openrust::image::TransformationHandler,
        ) -> openrust::image::Result<()> {
            self.order.lock().unwrap().push("handler");
            Ok(())
        }
        fn process(
            &self,
            contents: Bytes,
            pipeline: &ImagePipeline,
            limits: Limits,
        ) -> openrust::image::Result<EncodedImage> {
            self.inner.process(contents, pipeline, limits)
        }
        fn dominant_color(
            &self,
            contents: Bytes,
            pipeline: &ImagePipeline,
            limits: Limits,
        ) -> openrust::image::Result<String> {
            self.inner.dominant_color(contents, pipeline, limits)
        }
    }
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("reference/image-manager.json")).unwrap();
    let manager = ImageManager::new(Limits::default());
    let ready = Arc::new(AtomicBool::new(false));
    let order = Arc::new(std::sync::Mutex::new(Vec::new()));
    let create_ready = ready.clone();
    let create_order = order.clone();
    manager
        .extend("checked", move |manager| {
            Ok(Arc::new(Checked {
                inner: manager.driver(Some("image"))?,
                ready: create_ready.clone(),
                order: create_order.clone(),
            }))
        })
        .unwrap();
    manager
        .transform_using("checked", "Probe", Arc::new(|_, _| Ok(())))
        .unwrap();
    assert!(order.lock().unwrap().is_empty());
    assert!(matches!(
        manager.driver(Some("checked")),
        Err(Error::Missing)
    ));
    assert!(!manager.get_drivers().contains_key("checked"));
    ready.store(true, Ordering::SeqCst);
    let first = manager.driver(Some("checked")).unwrap();
    assert!(Arc::ptr_eq(
        &first,
        &manager.driver(Some("checked")).unwrap()
    ));
    manager.forget_drivers();
    assert!(!Arc::ptr_eq(
        &first,
        &manager.driver(Some("checked")).unwrap()
    ));
    assert_eq!(
        serde_json::json!(*order.lock().unwrap()),
        reference["requirements_order"]
    );
}
