use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use axum::{
    extract::{Request as TransportRequest, State},
    handler::Handler as _,
    response::Response as TransportResponse,
};
use serde_json::Value;
use tokio::sync::{oneshot, watch};
use tokio_util::sync::CancellationToken;

use super::{Kernel, middleware, server::listen_for};
use crate::{
    contracts::foundation::Application as _,
    foundation::{BootstrapError, RuntimeResult, maintenance, runtime_service},
    http::{Body, Request, StatusCode},
};

type BootState = Option<Result<Arc<Kernel>, ()>>;

#[derive(Clone)]
struct EarlyHttp {
    marker: PathBuf,
    kernel: watch::Receiver<BootState>,
}

pub(super) async fn has_template(marker: &Path) -> bool {
    maintenance::read_file(marker)
        .await
        .ok()
        .flatten()
        .is_some_and(|data| data.get("template").is_some_and(Value::is_string))
}

pub(super) async fn serve(kernel: Kernel, marker: PathBuf) -> RuntimeResult<()> {
    let app = kernel.application.share();
    let (configured, mut configuration) = oneshot::channel();
    let mut boot = Box::pin(async move {
        let mut kernel = kernel;
        kernel.bootstrap_http(Some(configured)).await?;
        Ok::<_, BootstrapError>(kernel)
    });
    let mut completed = None;
    let configured = tokio::select! {
        biased;
        ready = &mut configuration => ready.is_ok(),
        result = &mut boot => {
            completed = Some(result);
            configuration.try_recv().is_ok()
        }
    };
    let complete = completed.as_ref().is_some_and(Result::is_ok);
    let boot = async move {
        match completed {
            Some(result) => result,
            None => boot.await,
        }
    };
    let file_driver = configured
        && match app.config()?.get("app.maintenance.driver") {
            None | Some(Value::Null) => true,
            Some(Value::String(driver)) => driver == "file",
            _ => false,
        };
    if !file_driver || complete {
        let kernel = boot.await?;
        return kernel.serve_ready(listen_for(&app).await?).await;
    }

    let listener = listen_for(&app).await?;
    let (ready, receiver) = watch::channel(None);
    let state = EarlyHttp {
        marker: marker.clone(),
        kernel: receiver,
    };
    let termination = state.kernel.clone();
    let service = handle
        .with_state(state)
        .into_make_service_with_connect_info::<std::net::SocketAddr>();
    runtime_service::run(Vec::new(), move |shutdown| {
        Box::pin(async move {
            println!("OpenRust listening on http://{}", listener.local_addr()?);
            let accept = async {
                axum::serve(listener, service)
                    .with_graceful_shutdown(shutdown.clone().cancelled_owned())
                    .await?;
                let kernel = termination
                    .borrow()
                    .as_ref()
                    .and_then(|state| state.as_ref().ok())
                    .cloned();
                if let Some(kernel) = kernel {
                    kernel.termination_tasks.close();
                    kernel.termination_tasks.wait().await;
                }
                RuntimeResult::Ok(())
            };
            let bootstrap = async {
                let result = tokio::select! {
                    _ = shutdown.cancelled() => return Ok(()),
                    result = boot => result,
                };
                match result {
                    Ok(kernel) => {
                        let mut running = Vec::new();
                        for service in kernel.application.take_runtime_services() {
                            running.push(service.prepare(shutdown.clone()).await?);
                        }
                        ready.send_replace(Some(Ok(Arc::new(kernel))));
                        futures_util::future::try_join_all(running).await?;
                        Ok(())
                    }
                    Err(error) => {
                        eprintln!("Application bootstrap failed during maintenance: {error}");
                        ready.send_replace(Some(Err(())));
                        wait_for_up(&marker, shutdown.clone()).await?;
                        if shutdown.is_cancelled() {
                            Ok(())
                        } else {
                            Err(error.into())
                        }
                    }
                }
            };
            tokio::try_join!(accept, bootstrap)?;
            Ok(())
        })
    })
    .await
}

async fn wait_for_up(marker: &Path, shutdown: CancellationToken) -> RuntimeResult<()> {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return Ok(()),
            _ = interval.tick() => {
                if !has_template(marker).await {
                    return Ok(());
                }
            }
        }
    }
}

async fn handle(
    State(mut state): State<EarlyHttp>,
    incoming: TransportRequest,
) -> TransportResponse {
    let kernel = state
        .kernel
        .borrow()
        .as_ref()
        .and_then(|state| state.as_ref().ok())
        .cloned();
    if let Some(kernel) = kernel {
        return kernel.handleTransport(incoming).await;
    }
    let (parts, body) = incoming.into_parts();
    let request = Request::from_parts(parts, body);
    if let Ok(Some(data)) = maintenance::read_file(&state.marker).await
        && let Some(response) = middleware::prerender(&request, &data)
    {
        let (parts, body) = response.into_parts();
        return TransportResponse::from_parts(parts, body);
    }
    let _ = state.kernel.wait_for(Option::is_some).await;
    let kernel = state
        .kernel
        .borrow()
        .as_ref()
        .and_then(|state| state.as_ref().ok())
        .cloned();
    if let Some(kernel) = kernel {
        let (parts, body) = request.into_parts();
        return kernel
            .handleTransport(TransportRequest::from_parts(parts, body))
            .await;
    }
    TransportResponse::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(Body::empty())
        .expect("valid bootstrap error response")
}
