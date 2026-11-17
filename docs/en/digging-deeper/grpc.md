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

## Named clients {#named-clients}

Define outgoing connections in the `clients` object returned by `config/grpc.rs`. Each name resolves to a shared lazy Tonic connection within one application. The first RPC opens the connection.

```rust
"clients": {
    "health": {
        "host": "127.0.0.1",
        "port": 50051,
        "connect_timeout_ms": 1000,
        "timeout_ms": 2000,
    },
    "billing": {"endpoint": "http://billing.example.test:50051"},
},
```

Use `Grpc::connect("health")` in HTTP handlers and Artisan commands. You can also resolve `GrpcClients` from the application container. Pass the returned service to a generated Tonic client, such as `HealthClient::new(Grpc::connect("health")?)`. A missing client or malformed address fails during resolution. Network failures surface from the RPC.
