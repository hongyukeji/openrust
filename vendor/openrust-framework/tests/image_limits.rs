use ::image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use openrust::image::{Driver, EncodedImage, Error, Format, ImageManager, ImagePipeline, Limits};
use std::{
    io::Cursor,
    sync::{Arc, Mutex},
};
fn sample() -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(RgbaImage::from_pixel(12, 6, Rgba([20, 40, 60, 255])))
        .write_to(&mut bytes, ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}
#[tokio::test]
async fn image_limits_are_per_image_and_checked_before_a_resize() {
    let manager = ImageManager::new(Limits::default());
    let original = manager.from_bytes(sample());
    let limits = Limits {
        max_dimension: 10,
        ..Default::default()
    };
    assert!(matches!(
        original.clone().with_limits(limits).dimensions().await,
        Err(Error::DimensionsTooLarge)
    ));
    assert_eq!(original.dimensions().await.unwrap(), (12, 6));
    assert!(matches!(
        manager
            .from_bytes(sample())
            .resize(Some(2), Some(2))
            .unwrap()
            .with_limits(Limits {
                max_dimension: 4,
                ..Default::default()
            })
            .dimensions()
            .await,
        Err(Error::DimensionsTooLarge)
    ));
}
#[tokio::test]
async fn image_limits_revalidate_cached_bytes_render_and_dominant_color() {
    let source = ImageManager::new(Limits::default()).from_bytes(sample());
    assert_eq!(source.dimensions().await.unwrap(), (12, 6));
    source.dominant_color().await.unwrap();
    assert!(matches!(
        source
            .clone()
            .with_limits(Limits {
                max_input_bytes: 1,
                ..Default::default()
            })
            .to_bytes()
            .await,
        Err(Error::InputTooLarge)
    ));
    assert!(matches!(
        source
            .clone()
            .with_limits(Limits {
                max_dimension: 1,
                ..Default::default()
            })
            .dominant_color()
            .await,
        Err(Error::DimensionsTooLarge)
    ));
    assert_eq!(source.dimensions().await.unwrap(), (12, 6));
}
#[tokio::test]
async fn image_limits_cannot_relax_global_or_previously_tightened_bounds() {
    let tight = ImageManager::new(Limits {
        max_dimension: 10,
        ..Default::default()
    });
    assert!(matches!(
        tight
            .from_bytes(sample())
            .with_limits(Limits::default())
            .dimensions()
            .await,
        Err(Error::DimensionsTooLarge)
    ));
    let source = ImageManager::new(Limits::default())
        .from_bytes(sample())
        .with_limits(Limits {
            max_pixels: 10,
            ..Default::default()
        });
    assert!(matches!(
        source.with_limits(Limits::default()).dimensions().await,
        Err(Error::DimensionsTooLarge)
    ));
}
struct Probe(Arc<Mutex<Vec<Limits>>>);
impl Driver for Probe {
    fn dominant_color(
        &self,
        _contents: axum::body::Bytes,
        _pipeline: &ImagePipeline,
        _limits: Limits,
    ) -> openrust::image::Result<String> {
        Ok("#14283c".into())
    }

    fn process(
        &self,
        _contents: axum::body::Bytes,
        _pipeline: &ImagePipeline,
        limits: Limits,
    ) -> openrust::image::Result<EncodedImage> {
        self.0.lock().unwrap().push(limits);
        Ok(EncodedImage {
            bytes: sample().into(),
            format: Format::Png,
            dimensions: (12, 6),
        })
    }
}
#[tokio::test]
async fn image_limits_keep_registered_drivers_and_pipeline_selection() {
    let manager = ImageManager::new(Limits::default());
    let calls = Arc::new(Mutex::new(Vec::new()));
    let captured = calls.clone();
    manager
        .extend("probe", move |_| Ok(Arc::new(Probe(captured.clone()))))
        .unwrap();
    let image = manager
        .from_bytes(sample())
        .using("probe")
        .quality(80)
        .with_limits(Limits {
            max_dimension: 100,
            ..Default::default()
        });
    assert_eq!(image.dimensions().await.unwrap(), (12, 6));
    assert_eq!(calls.lock().unwrap().len(), 1);
    assert_eq!(calls.lock().unwrap()[0].max_dimension, 100);
}
