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

The listener uses plaintext HTTP/2 by default. Configure server credentials to serve TLS directly, or terminate TLS at a reverse proxy.

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

## Credentials and interceptors {#credentials-and-interceptors}

Register Tonic credentials and interceptor groups during application boot, before resolving a client or starting the listener. Assign their names in the client configuration.

```rust
use openrust::grpc::{Grpc, tonic::transport::ClientTlsConfig};

Grpc::clients()?.register_credentials(
    "internal-tls",
    ClientTlsConfig::new().ca_certificate(internal_ca).domain_name("health.example.test"),
);
Grpc::clients()?.register_interceptor("request-id", |mut request| {
    request.metadata_mut().insert("x-request-id", "grpc-probe".parse().unwrap());
    Ok(request)
});
```

```json
{"host":"health.example.test","port":50051,"credentials":"internal-tls","interceptors":["request-id"]}
```

A TLS group makes a host-and-port client use HTTPS. An explicit HTTPS endpoint requires a registered group. An unknown group or a scheme that disagrees with the credentials is a configuration error.

Client interceptor groups run in the order listed in configuration, with each group running in registration order. Unregistered interceptor groups are skipped. Use `Grpc::server()?.add_interceptor(...)` to add a server interceptor across all registered services. Returning a Tonic status stops the chain and returns that status to the caller.

Use `Grpc::server()?.set_credentials(ServerTlsConfig::new().identity(server_identity))` for server TLS. To require mutual TLS, add `client_ca_root(client_ca)` on the server and `identity(client_identity)` in the matching client credentials.
