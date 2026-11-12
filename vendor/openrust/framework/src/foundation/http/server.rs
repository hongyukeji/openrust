use std::{error::Error, io, sync::Arc};

use axum::{
    extract::{Request as TransportRequest, State},
    handler::Handler as _,
    response::Response as TransportResponse,
};
use tokio::net::TcpListener;

use super::Kernel;
use crate::{
    config::{ConfigError, Value},
    contracts::{config::Repository, foundation::Application as _, http::Kernel as _},
    http::Request,
};

impl Kernel {
    /// Boots the application and binds its configured HTTP address.
    pub async fn listen(&mut self) -> Result<TcpListener, Box<dyn Error + Send + Sync>> {
        self.bootstrap().await?;
        let (host, port) = listener_address(self.get_application().config()?.as_ref())?;
        let listener = TcpListener::bind((host.as_str(), port))
            .await
            .map_err(|error| {
                io::Error::new(error.kind(), format!("cannot bind HTTP listener: {error}"))
            })?;
        Ok(listener)
    }

    pub(in crate::foundation) async fn serve(mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let listener = self.listen().await?;

        let services = self.get_application().take_runtime_services();
        let termination_tasks = self.termination_tasks.clone();
        let service = handle
            .with_state(Arc::new(self))
            .into_make_service_with_connect_info::<std::net::SocketAddr>();
        super::super::runtime_service::run(services, move |shutdown| {
            Box::pin(async move {
                println!("OpenRust listening on http://{}", listener.local_addr()?);
                let result = axum::serve(listener, service)
                    .with_graceful_shutdown(shutdown.cancelled_owned())
                    .await;
                termination_tasks.close();
                termination_tasks.wait().await;
                result?;
                Ok(())
            })
        })
        .await
    }
}

async fn handle(State(kernel): State<Arc<Kernel>>, request: TransportRequest) -> TransportResponse {
    kernel.handleTransport(request).await
}

impl Kernel {
    /// Dispatches an HTTP transport request and schedules any post-response middleware.
    #[allow(
        non_snake_case,
        reason = "Matches the framework HTTP entry point naming"
    )]
    pub async fn handleTransport(self: Arc<Self>, incoming: TransportRequest) -> TransportResponse {
        let (parts, body) = incoming.into_parts();
        let mut request = Request::from_parts(parts, body);
        let (parts, body) = self.handle(&mut request).await.into_parts();
        if request
            .parts()
            .extensions
            .get::<super::Termination>()
            .is_none()
            && request
                .parts()
                .extensions
                .get::<crate::concurrency::DeferredCallbacks>()
                .is_none()
        {
            return TransportResponse::from_parts(parts, body);
        }
        let (request, _) = request.into_parts();
        let body = super::termination::ResponseBody::wrap(body, self, request, parts.clone());
        TransportResponse::from_parts(parts, body)
    }
}

fn listener_address(config: &dyn Repository) -> Result<(String, u16), ConfigError> {
    let host = match config.get("app.host") {
        Some(Value::String(host)) if !host.trim().is_empty() => host,
        _ => {
            return Err(ConfigError::InvalidType {
                key: "app.host".to_owned(),
                expected: "a non-empty host",
            });
        }
    };
    let port = config
        .get("app.port")
        .and_then(|port| port.as_u64())
        .and_then(|port| u16::try_from(port).ok())
        .ok_or_else(|| ConfigError::InvalidType {
            key: "app.port".to_owned(),
            expected: "an integer from 0 to 65535",
        })?;
    Ok((host, port))
}
