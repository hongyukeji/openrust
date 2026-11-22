use http::{HeaderMap, StatusCode};
use openrust::http::client::{Error, Factory, Response};
use std::{
    io::{Read as _, Seek as _, SeekFrom},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
    sync::Notify,
};

async fn server() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    (listener, url)
}
async fn connection(listener: &TcpListener) -> tokio::net::TcpStream {
    let (mut socket, _) = listener.accept().await.unwrap();
    let mut request = Vec::new();
    while !request.ends_with(b"\r\n\r\n") {
        request.push(socket.read_u8().await.unwrap());
    }
    socket
}
async fn wait_file(path: &std::path::Path, bytes: &[u8]) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if tokio::fs::read(path)
                .await
                .is_ok_and(|actual| actual == bytes)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn sink_writes_before_completion_and_response_resources_share_cursor_and_close() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("download");
    std::fs::write(&path, b"old contents").unwrap();
    let (listener, url) = server().await;
    let finish = Arc::new(Notify::new());
    let signal = finish.clone();
    let task = tokio::spawn(async move {
        let mut socket = connection(&listener).await;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nabc")
            .await
            .unwrap();
        signal.notified().await;
        socket.write_all(b"def").await.unwrap();
    });
    let output = path.clone();
    let download =
        tokio::spawn(async move { Factory::new().unwrap().sink(output).get(&url).await });
    wait_file(&path, b"abc").await;
    assert!(!download.is_finished());
    finish.notify_one();
    let response = download.await.unwrap().unwrap();
    task.await.unwrap();
    let mut resource = response.resource().unwrap();
    let mut other = response.clone().resource().unwrap();
    let mut first = [0; 2];
    resource.read_exact(&mut first).unwrap();
    assert_eq!(&first, b"ab");
    assert_eq!(other.stream_position().unwrap(), 2);
    other.seek(SeekFrom::End(-2)).unwrap();
    let mut tail = String::new();
    resource.read_to_string(&mut tail).unwrap();
    assert_eq!(tail, "ef");
    assert_eq!(response.body().unwrap(), b"abcdef"[..]);
    assert_eq!(other.stream_position().unwrap(), 6);
    assert_eq!(response.body().unwrap(), b"abcdef"[..]);
    response.clone().close().unwrap();
    assert!(matches!(response.body(), Err(Error::ResponseBody(_))));
    assert!(resource.read(&mut first).is_err());
    assert!(response.resource().is_err());
}

#[tokio::test]
async fn connection_failure_keeps_old_file_but_successful_empty_response_truncates_it() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("download");
    std::fs::write(&path, b"old").unwrap();
    let (listener, url) = server().await;
    drop(listener);
    let factory = Factory::new().unwrap();
    assert!(matches!(
        factory.sink(&path).get(&url).await,
        Err(Error::Connection)
    ));
    assert_eq!(std::fs::read(&path).unwrap(), b"old");
    let (listener, url) = server().await;
    let task = tokio::spawn(async move {
        connection(&listener)
            .await
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
    });
    assert!(
        factory
            .sink(&path)
            .get(&url)
            .await
            .unwrap()
            .body()
            .unwrap()
            .is_empty()
    );
    task.await.unwrap();
    assert!(std::fs::read(path).unwrap().is_empty());
}

#[tokio::test]
async fn disconnected_and_timed_out_downloads_keep_partial_file() {
    let temp = tempfile::tempdir().unwrap();
    for timeout in [false, true] {
        let path = temp
            .path()
            .join(if timeout { "timeout" } else { "disconnected" });
        let (listener, url) = server().await;
        let task = tokio::spawn(async move {
            let mut socket = connection(&listener).await;
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nabc",
                )
                .await
                .unwrap();
            if timeout {
                std::future::pending::<()>().await;
            }
        });
        let result = Factory::new()
            .unwrap()
            .sink(&path)
            .timeout(Duration::from_millis(500))
            .get(&url)
            .await;
        assert!(matches!(result, Err(Error::Timeout)) == timeout);
        assert!(result.is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"abc");
        task.abort();
    }
}

#[tokio::test]
async fn cancelled_download_keeps_written_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("cancelled");
    let (listener, url) = server().await;
    let task = tokio::spawn(async move {
        let mut socket = connection(&listener).await;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nabc")
            .await
            .unwrap();
        std::future::pending::<()>().await;
    });
    let output = path.clone();
    let download =
        tokio::spawn(async move { Factory::new().unwrap().sink(output).get(&url).await });
    wait_file(&path, b"abc").await;
    download.abort();
    assert!(download.await.unwrap_err().is_cancelled());
    assert_eq!(std::fs::read(path).unwrap(), b"abc");
    task.abort();
}

#[tokio::test]
async fn response_limit_retains_prior_chunks_and_missing_parent_prevents_dispatch() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("limit");
    let (listener, url) = server().await;
    let finish = Arc::new(Notify::new());
    let signal = finish.clone();
    let task = tokio::spawn(async move {
        let mut socket = connection(&listener).await;
        socket.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\nabc\r\n").await.unwrap();
        signal.notified().await;
        socket.write_all(b"3\r\ndef\r\n0\r\n\r\n").await.unwrap();
    });
    let output = path.clone();
    let download = tokio::spawn(async move {
        Factory::new()
            .unwrap()
            .sink(output)
            .maxResponseBytes(4)
            .get(&url)
            .await
    });
    wait_file(&path, b"abc").await;
    finish.notify_one();
    assert!(matches!(download.await.unwrap(), Err(Error::BodyTooLarge)));
    task.await.unwrap();
    assert_eq!(std::fs::read(path).unwrap(), b"abc");
    let factory = Factory::new().unwrap();
    factory.fake(|_| panic!("父目录错误须在请求分发前返回"));
    assert!(matches!(
        factory
            .sink(temp.path().join("missing/file"))
            .get("http://example.test")
            .await,
        Err(Error::Sink(_))
    ));
}

#[tokio::test]
async fn file_sink_uses_current_offset_without_truncation_and_reports_read_errors() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("resource");
    std::fs::write(&path, b"prefix--tail").unwrap();
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    file.seek(SeekFrom::Start(6)).unwrap();
    let (listener, url) = server().await;
    let task = tokio::spawn(async move {
        connection(&listener)
            .await
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
            .await
            .unwrap();
    });
    let response = Factory::new().unwrap().sink(file).get(&url).await.unwrap();
    task.await.unwrap();
    assert_eq!(response.body().unwrap(), b"prefixOKtail"[..]);
    let (listener, url) = server().await;
    let task = tokio::spawn(async move {
        connection(&listener)
            .await
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
            .await
            .unwrap();
    });
    let write_only = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    let response = Factory::new()
        .unwrap()
        .sink(write_only)
        .get(&url)
        .await
        .unwrap();
    task.await.unwrap();
    let error = response.body().unwrap_err();
    assert!(matches!(error, Error::ResponseBody(_)));
    assert!(std::error::Error::source(&error).is_some());
    assert!(matches!(
        response.json::<serde_json::Value>(),
        Err(Error::ResponseBody(_))
    ));
}

#[tokio::test]
async fn path_sink_truncates_each_redirect_and_retry_attempt() {
    let temp = tempfile::tempdir().unwrap();
    for retry in [false, true] {
        let path = temp.path().join(if retry { "retry" } else { "redirect" });
        let (listener, url) = server().await;
        let task = tokio::spawn(async move {
            let mut socket = connection(&listener).await;
            let status = if retry {
                "500 Internal Server Error"
            } else {
                "302 Found"
            };
            socket.write_all(format!("HTTP/1.1 {status}\r\nLocation: /next\r\nContent-Length: 9\r\nConnection: close\r\n\r\nfirst-hop").as_bytes()).await.unwrap();
            drop(socket);
            connection(&listener)
                .await
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
                .await
                .unwrap();
        });
        let mut request = Factory::new().unwrap().sink(&path);
        if retry {
            request = request.retry(2, Duration::ZERO);
        }
        let response = request.get(&url).await.unwrap();
        task.await.unwrap();
        assert_eq!(response.body().unwrap(), b"OK"[..]);
        assert_eq!(std::fs::read(path).unwrap(), b"OK");
    }
}

#[tokio::test]
async fn fake_sink_preserves_fake_body_and_memory_resources_follow_stream_rules() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("fake");
    let factory = Factory::new().unwrap();
    factory.fake(|_| Some(Ok(Response::new(StatusCode::OK, HeaderMap::new(), b"fake"))));
    let response = factory
        .sink(&path)
        .get("http://example.test")
        .await
        .unwrap();
    assert_eq!(response.body().unwrap(), b"fake"[..]);
    assert_eq!(std::fs::read(&path).unwrap(), b"fake");
    let mut resource = response.resource().unwrap();
    assert_eq!(resource.stream_position().unwrap(), 4);
    resource.rewind().unwrap();
    let mut bytes = Vec::new();
    resource.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"fake");
    resource.seek(SeekFrom::End(3)).unwrap();
    assert_eq!(resource.read(&mut [0]).unwrap(), 0);
    assert!(resource.seek(SeekFrom::Start(0)).is_ok());
    assert!(resource.seek(SeekFrom::Current(-1)).is_err());
    response.close().unwrap();
    assert!(resource.rewind().is_err());
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    assert_eq!(
        factory
            .sink(file)
            .get("http://example.test")
            .await
            .unwrap()
            .body()
            .unwrap(),
        b"fake"[..]
    );
}

#[tokio::test]
async fn large_response_streams_without_materializing_body() {
    const SIZE: usize = 128 * 1024 * 1024;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("large");
    let (listener, url) = server().await;
    let task = tokio::spawn(async move {
        let mut socket = connection(&listener).await;
        socket
            .write_all(
                format!("HTTP/1.1 200 OK\r\nContent-Length: {SIZE}\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .unwrap();
        let chunk = [b'x'; 65536];
        for _ in 0..SIZE / chunk.len() {
            socket.write_all(&chunk).await.unwrap();
        }
    });
    let factory = Factory::new().unwrap();
    let mut request = factory.pendingRequest().maxResponseBytes(SIZE);
    let buffered = std::env::var_os("OPENRUST_ACCEPTANCE_BUFFERED").is_some();
    if !buffered {
        request = request.sink(&path);
    }
    let response = request.get(&url).await.unwrap();
    task.await.unwrap();
    if !buffered {
        assert_eq!(std::fs::metadata(path).unwrap().len(), SIZE as u64);
    }
    let mut resource = response.resource().unwrap();
    resource.seek(SeekFrom::End(-4)).unwrap();
    let mut tail = [0; 4];
    resource.read_exact(&mut tail).unwrap();
    assert_eq!(tail, [b'x'; 4]);
}

#[tokio::test]
async fn facade_sink_and_resource_writes_match_saved_php_reference() {
    use openrust::{foundation::Application, support::facades::Http};
    use std::{io::Write as _, sync::Mutex};
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("reference/http-sink.json")).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("facade");
    let request = Arc::new(Mutex::new(None));
    let captured = request.clone();
    let output = path.clone();
    let mut app = Application::configure(temp.path().to_path_buf())
        .booted(move |_| {
            *captured.lock().unwrap() = Some(Http::sink(output.clone())?);
            Ok(())
        })
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let (listener, url) = server().await;
    let task = tokio::spawn(async move {
        connection(&listener)
            .await
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
            .await
            .unwrap();
    });
    let pending = request.lock().unwrap().take().unwrap();
    let response = pending.get(&url).await.unwrap();
    task.await.unwrap();
    assert_eq!(
        std::str::from_utf8(&response.body().unwrap()).unwrap(),
        reference["path_body"].as_str().unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        reference["path_file"]
    );
    let mut resource = response.resource().unwrap();
    resource.rewind().unwrap();
    resource.write_all(b"NEW").unwrap();
    resource.flush().unwrap();
    assert_eq!(response.body().unwrap(), b"NEW"[..]);
    response.close().unwrap();
    assert_eq!(reference["close_body"], "error");
    assert!(matches!(response.body(), Err(Error::ResponseBody(_))));
    let memory = Response::new(StatusCode::OK, HeaderMap::new(), b"OK");
    let mut resource = memory.resource().unwrap();
    resource.seek(SeekFrom::Start(3)).unwrap();
    resource.write_all(b"Z").unwrap();
    assert_eq!(memory.body().unwrap(), b"OK\0Z"[..]);
    memory.close().unwrap();
    assert!(resource.write_all(b"x").is_err());
}

#[tokio::test]
async fn sink_write_errors_preserve_source_and_owned_file_contents() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("readonly");
    std::fs::write(&path, b"old").unwrap();
    let (listener, url) = server().await;
    let task = tokio::spawn(async move {
        connection(&listener)
            .await
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
            .await
            .unwrap();
    });
    let result = Factory::new()
        .unwrap()
        .sink(std::fs::File::open(&path).unwrap())
        .get(&url)
        .await;
    task.await.unwrap();
    let error = result.unwrap_err();
    assert!(matches!(error, Error::Sink(_)));
    assert!(std::error::Error::source(&error).is_some());
    assert_eq!(std::fs::read(&path).unwrap(), b"old");
}
