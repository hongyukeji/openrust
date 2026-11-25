use futures_util::stream;
use openrust::http::{Body, HttpError, Request};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
fn request(body: Body, length: Option<&str>) -> Request {
    let mut builder = http::Request::builder();
    if let Some(value) = length {
        builder = builder.header("content-length", value);
    }
    let (parts, body) = builder.body(body).unwrap().into_parts();
    Request::from_parts(parts, body)
}
#[tokio::test]
async fn bounded_body_reader_preserves_original_bytes_and_checks_cached_body() {
    let mut request = request(Body::from(b" raw\0\xff ".to_vec()), None);
    let original = request.body_bytes().await.unwrap();
    assert_eq!(&original[..], b" raw\0\xff ");
    assert!(matches!(
        request.body_bytes_with_limit(2).await,
        Err(HttpError::PayloadTooLarge)
    ));
    assert_eq!(
        request.body_bytes_with_limit(original.len()).await.unwrap(),
        original
    );
    assert_eq!(request.body_bytes().await.unwrap(), original);
}
#[tokio::test]
async fn declared_body_budget_rejects_without_polling_the_stream() {
    let reads = Arc::new(AtomicUsize::new(0));
    let count = reads.clone();
    let mut request = request(
        Body::from_stream(
            stream::iter([Ok::<_, std::io::Error>(b"not-read".to_vec())]).inspect(move |_| {
                count.fetch_add(1, Ordering::SeqCst);
            }),
        ),
        Some("8"),
    );
    assert!(matches!(
        request.body_bytes_with_limit(7).await,
        Err(HttpError::PayloadTooLarge)
    ));
    assert_eq!(reads.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn chunked_budget_stops_at_first_excess_and_cannot_be_retried() {
    let reads = Arc::new(AtomicUsize::new(0));
    let count = reads.clone();
    let chunks =
        stream::iter((0..100).map(|_| Ok::<_, std::io::Error>(vec![b'x'; 4]))).inspect(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
        });
    let mut request = request(Body::from_stream(chunks), None);
    assert!(matches!(
        request.body_bytes_with_limit(8).await,
        Err(HttpError::PayloadTooLarge)
    ));
    assert_eq!(reads.load(Ordering::SeqCst), 3);
    assert!(matches!(
        request.body_bytes().await,
        Err(HttpError::PayloadTooLarge)
    ));
    assert_eq!(reads.load(Ordering::SeqCst), 3);
}
#[tokio::test]
async fn requested_budget_never_increases_the_existing_global_limit() {
    let mut request = request(Body::empty(), Some("8388609"));
    assert!(matches!(
        request.body_bytes_with_limit(usize::MAX).await,
        Err(HttpError::PayloadTooLarge)
    ));
}
#[tokio::test]
async fn zero_budget_accepts_only_an_empty_body() {
    assert!(
        request(Body::empty(), None)
            .body_bytes_with_limit(0)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        request(Body::from("x"), None)
            .body_bytes_with_limit(0)
            .await,
        Err(HttpError::PayloadTooLarge)
    ));
}
use futures_util::StreamExt as _;

#[tokio::test]
async fn cancelled_read_never_turns_a_consumed_stream_into_an_empty_retry() {
    let body = Body::from_stream(stream::once(std::future::pending::<
        Result<Vec<u8>, std::io::Error>,
    >()));
    let mut request = request(body, None);
    {
        let read = request.body_bytes_with_limit(4);
        tokio::pin!(read);
        tokio::select! {
            biased;
            result=&mut read=>panic!("挂起流不应完成：{result:?}"),
            ()=tokio::task::yield_now()=>{},
        }
    }
    assert!(matches!(
        request.body_bytes().await,
        Err(HttpError::InvalidRequest)
    ));
}
