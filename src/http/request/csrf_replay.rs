//! CSRF 先复用唯一受管 Request；原始 body 保留共享暂存所有权并供下游重放。

use axum::{
    body::{Body, Bytes},
    extract::FromRequest,
};
use tokio::io::AsyncReadExt;

use crate::Result;

pub(crate) async fn prepare(
    request: axum::extract::Request,
) -> Result<(super::Request, axum::extract::Request)> {
    let (parts, body) = request.into_parts();
    let replay_parts = parts.clone();
    let parsed =
        super::Request::from_request(axum::extract::Request::from_parts(parts, body), &()).await?;
    let reader = parsed.content().await?;
    let stream = futures_util::stream::unfold(Some(reader), |reader| async move {
        let mut reader = reader?;
        let mut bytes = vec![0_u8; 64 * 1024];
        match reader.read(&mut bytes).await {
            Ok(0) => None,
            Ok(size) => {
                bytes.truncate(size);
                Some((
                    Ok::<Bytes, std::io::Error>(Bytes::from(bytes)),
                    Some(reader),
                ))
            }
            Err(error) => Some((Err(error), None)),
        }
    });
    let mut replay = axum::extract::Request::from_parts(replay_parts, Body::from_stream(stream));
    replay.extensions_mut().insert(parsed.clone());
    Ok((parsed, replay))
}
