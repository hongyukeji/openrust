# Reverb

## Installation

Build the application with the `reverb` Cargo feature. The feature registers the Reverb service provider, the `reverb` broadcast driver, and the Artisan commands.

```sh
cargo build --features reverb
```

Set `REVERB_APP_ID`, `REVERB_APP_KEY`, and `REVERB_APP_SECRET` to the credentials shared by the listener and publisher. Give clients the public key and WebSocket address. Keep the secret on the server.

## Running the server

Set `REVERB_ENABLED=true` for the default embedded mode. Starting the application binds both the HTTP and Reverb listeners. `REVERB_SERVER_HOST`, `REVERB_SERVER_PORT`, and `REVERB_SERVER_PATH` configure the Reverb listener. Use `REVERB_HOST`, `REVERB_PORT`, and `REVERB_SCHEME` for the destination used by the broadcaster. For a local listener without a TLS proxy, use `REVERB_SCHEME=http`.

For a separate process, set `REVERB_MODE=standalone` and start Reverb through Artisan:

```sh
./target/debug/main artisan reverb:start --host=127.0.0.1 --port=8080
./target/debug/main artisan reverb:restart
```

The restart command writes a signal to the application's cache. Use a shared cache store such as `CACHE_STORE=file` on one host. The standalone process closes its listeners and connections on receipt of the signal. A process manager can start it again. The embedded process follows the application's shutdown and restart lifecycle.

## Broadcasting and scaling

Set `BROADCAST_CONNECTION=reverb` and follow the [broadcasting guide](../digging-deeper/broadcasting.md) to enable private and presence authorization. Connect a Pusher protocol client to `/app/{key}` with the configured server path prefix. Set the application's `allowed_origins` to trusted host names for production and terminate TLS at the reverse proxy.

Set `REVERB_SCALING_ENABLED=true` on each instance to deliver signed HTTP publications over Redis. Instances share `REVERB_SCALING_CHANNEL` and Redis settings. Use `REDIS_URL` or the configured Redis host, credentials, port, and database. Channel membership and client-originated events are local to each listener.
