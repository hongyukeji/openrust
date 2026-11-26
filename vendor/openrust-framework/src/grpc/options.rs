use std::sync::{Arc, Mutex};

use tonic::{Request, Status, service::Interceptor, transport::ServerTlsConfig};

pub(super) type Callback = Arc<dyn Fn(Request<()>) -> Result<Request<()>, Status> + Send + Sync>;

/// A chain of request interceptors shared by Tonic clients and servers.
#[derive(Clone, Default)]
pub struct GrpcInterceptor(pub(super) Vec<Callback>);

impl Interceptor for GrpcInterceptor {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        for intercept in &self.0 {
            request = intercept(request)?;
        }
        Ok(request)
    }
}

/// Application-wide options for the installed gRPC listener.
#[derive(Default)]
pub struct GrpcServerOptions {
    options: Mutex<(Option<ServerTlsConfig>, GrpcInterceptor)>,
}

impl GrpcServerOptions {
    /// Sets the transport credentials to use when the listener starts.
    pub fn set_credentials(&self, credentials: ServerTlsConfig) {
        self.options.lock().unwrap().0 = Some(credentials);
    }

    /// Adds a request interceptor, in registration order.
    pub fn add_interceptor(
        &self,
        interceptor: impl Fn(Request<()>) -> Result<Request<()>, Status> + Send + Sync + 'static,
    ) {
        self.options.lock().unwrap().1.0.push(Arc::new(interceptor));
    }

    pub(super) fn snapshot(&self) -> (Option<ServerTlsConfig>, GrpcInterceptor) {
        self.options.lock().unwrap().clone()
    }
}
