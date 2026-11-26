use std::{error::Error, time::Duration};

use futures_util::StreamExt;
use openrust::http::client::{Error as HttpError, Factory};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    time::timeout,
};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

async fn accept(listener: TcpListener) -> TcpStream {
    let (mut socket, _) = listener.accept().await.unwrap();
    let mut request = Vec::new();
    while !request.ends_with(b"\r\n\r\n") {
        request.push(socket.read_u8().await.unwrap());
    }
    socket
        .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    socket
}

#[tokio::test]
async fn first_chunk_arrives_before_server_finishes() -> TestResult {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let (client_received_first, server_permission) = oneshot::channel();
    let server = tokio::spawn(async move {
        let mut socket = accept(listener).await;
        socket.write_all(b"3\r\none\r\n").await.unwrap();
        server_permission.await.unwrap();
        socket.write_all(b"3\r\ntwo\r\n0\r\n\r\n").await.unwrap();
    });
    let response = Factory::new()?
        .pending_request()
        .stream()
        .timeout(Duration::from_secs(3))
        .get(&url)
        .await?;
    assert!(response.body().is_err());
    let other = response.clone();
    let mut stream = response.into_stream()?;
    assert!(other.into_stream().is_err());
    assert_eq!(stream.next().await.transpose()?.unwrap(), "one");
    client_received_first.send(()).unwrap();
    assert_eq!(stream.next().await.transpose()?.unwrap(), "two");
    assert!(stream.next().await.is_none());
    server.await?;
    Ok(())
}

#[tokio::test]
async fn unread_network_body_is_cancelled_when_stream_is_dropped() -> TestResult {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move {
        let mut socket = accept(listener).await;
        socket.write_all(b"3\r\none\r\n").await.unwrap();
        let mut trailing = [0; 1];
        timeout(Duration::from_secs(3), socket.read(&mut trailing))
            .await
            .unwrap()
            .unwrap()
    });
    let response = Factory::new()?.pending_request().stream().get(&url).await?;
    let mut stream = response.into_stream()?;
    assert_eq!(stream.next().await.transpose()?.unwrap(), "one");
    drop(stream);
    assert_eq!(server.await?, 0);
    Ok(())
}

#[tokio::test]
async fn streaming_body_obeys_byte_limit_and_deadline() -> TestResult {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move {
        accept(listener)
            .await
            .write_all(b"3\r\none\r\n3\r\ntwo\r\n0\r\n\r\n")
            .await
            .unwrap();
    });
    let factory = Factory::new()?;
    let response = factory
        .pending_request()
        .stream()
        .max_response_bytes(4)
        .get(&url)
        .await?;
    let mut stream = response.into_stream()?;
    assert_eq!(stream.next().await.transpose()?.unwrap(), "one");
    assert!(matches!(
        stream.next().await,
        Some(Err(HttpError::BodyTooLarge))
    ));
    assert!(stream.next().await.is_none());
    server.await?;

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let (keep_open, pending) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let _socket = accept(listener).await;
        let _ = pending.await;
    });
    let response = factory
        .pending_request()
        .stream()
        .timeout(Duration::from_millis(350))
        .get(&url)
        .await?;
    let mut stream = response.into_stream()?;
    assert!(matches!(stream.next().await, Some(Err(HttpError::Timeout))));
    keep_open.send(()).unwrap();
    server.await?;
    Ok(())
}
