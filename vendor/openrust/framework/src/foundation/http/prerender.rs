use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    extract::{Request as TransportRequest, State},
    handler::Handler as _,
    response::Response as TransportResponse,
};
use serde_json::Value;
use tokio::{
    net::TcpListener,
    sync::{oneshot, watch},
};

use super::{
    Kernel, middleware,
    server::{http_eligible, listen_for},
};
use crate::{
    contracts::foundation::Application as _,
    foundation::{
        BootstrapError, Runner, RunnerContext, RuntimeResult, maintenance, runner,
        runner_pattern::RunnerFilter,
    },
    http::{Body, Request, StatusCode},
};

type BootState = Option<Result<Arc<Kernel>, ()>>;

#[derive(Clone)]
struct EarlyHttp {
    marker: PathBuf,
    kernel: watch::Receiver<BootState>,
    except: Arc<[String]>,
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
    let except = kernel.middleware.maintenance_exceptions.clone().into();
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
        return kernel.serve_ready().await;
    }
    let config = app.config()?;
    let filter = RunnerFilter::from_config(config.as_ref())?;
    if filter.disables(runner::HTTP) || !http_eligible(config.as_ref())? {
        let kernel = boot.await?;
        return kernel.serve_ready().await;
    }

    let listener = listen_for(&app).await?;
    let (ready, receiver) = watch::channel(None);
    let http = Arc::new(PrerenderRunner {
        listener: Mutex::new(Some(listener)),
        state: EarlyHttp {
            marker: marker.clone(),
            kernel: receiver,
            except,
        },
    });
    let mut runners = runner::RunnerSet::default();
    runners.start(vec![http]).await?;
    let failed = runners.failure_signal();
    let signal = runner::wait_for_shutdown();
    tokio::pin!(signal);
    let finish_boot = async {
        match boot.await {
            Ok(kernel) => {
                let services = runner::configured(&kernel.application, None, &filter).await?;
                runners.start(services).await?;
                ready.send_replace(Some(Ok(Arc::new(kernel))));
                Ok(())
            }
            Err(error) => {
                eprintln!("Application bootstrap failed during maintenance: {error}");
                ready.send_replace(Some(Err(())));
                wait_for_up(&marker).await;
                Err(error.into())
            }
        }
    };
    let result: Option<RuntimeResult<()>> = tokio::select! {
        biased;
        result = &mut signal => Some(result.map_err(Into::into)),
        () = failed.cancelled() => Some(Ok(())),
        result = finish_boot => if result.is_ok() { None } else { Some(result) },
    };
    if let Some(result) = result {
        let stopped = runners.stop().await;
        return result.and(stopped);
    }
    runners.wait(signal).await
}

struct PrerenderRunner {
    listener: Mutex<Option<TcpListener>>,
    state: EarlyHttp,
}

impl Runner for PrerenderRunner {
    fn signature(&self) -> &str {
        runner::HTTP
    }

    fn run(&self, context: RunnerContext) -> crate::http::HttpFuture<'_, RuntimeResult<()>> {
        Box::pin(async move {
            let listener = self
                .listener
                .lock()
                .unwrap()
                .take()
                .expect("HTTP runner starts once");
            let termination = self.state.kernel.clone();
            let service = handle
                .with_state(self.state.clone())
                .into_make_service_with_connect_info::<std::net::SocketAddr>();
            println!("OpenRust listening on http://{}", listener.local_addr()?);
            let result = axum::serve(listener, service)
                .with_graceful_shutdown(context.cancelled_owned())
                .await;
            let kernel = termination
                .borrow()
                .as_ref()
                .and_then(|state| state.as_ref().ok())
                .cloned();
            if let Some(kernel) = kernel {
                kernel.termination_tasks.close();
                kernel.termination_tasks.wait().await;
            }
            result?;
            Ok(())
        })
    }
}

async fn wait_for_up(marker: &Path) {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    loop {
        interval.tick().await;
        if !has_template(marker).await {
            return;
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
        && let Some(response) = middleware::prerender(&request, &data, &state.except)
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
