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

## Broadcasting and scaling {#broadcasting-and-scaling}

Set `BROADCAST_CONNECTION=reverb` and follow the [broadcasting guide](../digging-deeper/broadcasting.md) to enable private and presence authorization. Connect a Pusher protocol client to `/app/{key}` with the configured server path prefix. Set the application's `allowed_origins` to trusted host names for production and terminate TLS at the reverse proxy.

Set `REVERB_SCALING_ENABLED=true` on each instance. Nodes for one app use the same `REVERB_APP_ID`, `REVERB_APP_KEY`, `REVERB_APP_SECRET`, `REVERB_SCALING_CHANNEL`, and Redis connection settings. Use `REDIS_URL` or the separate Redis settings. Give different apps distinct IDs on a shared scaling channel. All nodes on a scaling channel should run the same OpenRust Reverb version.

New presence subscriptions receive the cluster-wide member list and count. A user with multiple sockets counts once, and other subscribers see that user's first join and last leave. Graceful exits remove the sockets when Redis is reachable. After an abrupt exit, a surviving node with subscribers prunes expired sockets using a 10-second lease and a roughly 2-second check. A new subscription also removes expired entries before returning the roster.

Redis Pub/Sub delivers signed HTTP publications and authorized client events across nodes. Client events go to other subscribers in the same app and channel, excluding the sending socket. When a node loses its Redis subscription, it closes its presence sockets so clients reconnect and subscribe again after recovery. Delivery is best effort, with no replay of events missed during an outage.
