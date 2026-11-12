# gRPC

## Installation

Run the gRPC installer from the application checkout:

```sh
./artisan install:grpc
```

It creates `routes/grpc.rs` and connects its registrar through `withGrpc` in `bootstrap/app.rs`. Running the installer again preserves an existing route file. Rebuild after registering your services.

## Registering services

Add generated Tonic services to the supplied `RoutesBuilder` inside `routes/grpc.rs`. The registrar receives the application so it can resolve its services. OpenRust executes this callback once after providers and HTTP routes have been assembled.

```rust
use openrust::{foundation::Application, grpc::RoutesBuilder, support::ProviderResult};

pub(crate) fn register(_app: &Application, services: &mut RoutesBuilder) -> ProviderResult {
    let (_, health) = tonic_health::server::health_reporter();
    services.add_service(health);
    Ok(())
}
```

This example requires the `tonic-health` dependency in the application.

## Running the listener

Set `GRPC_HOST` and `GRPC_PORT` before starting the application. Leave the host empty to disable the listener. The HTTP and gRPC ports are bound before either reports readiness. A bind failure stops startup, and the listeners share graceful shutdown. Artisan boots the application without binding the gRPC port.

This listener accepts plaintext gRPC over HTTP/2. Use a TLS terminator when serving clients over an untrusted network.
