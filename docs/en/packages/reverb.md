# Reverb

## Installation

Add `reverb` to the application’s `[features].default` list in `Cargo.toml`, preserving existing features. The feature registers the Reverb service provider, the `reverb` broadcast driver, and the Artisan commands. Use `./artisan` to run source commands.

The package supplies its own default server configuration; enabling the feature does not create `config/reverb.rs`. For application overrides, run `./artisan vendor:publish --tag=reverb-config`, add `mod reverb;` and `("reverb".to_owned(), reverb::load(env)?),` to the declarations and configuration map in `config/mod.rs`, then rebuild. Explicit `init --extensions=reverb` publishes and registers this configuration. Repeated initialization preserves existing configuration unless `--force` is supplied.

Set `REVERB_APP_ID`, `REVERB_APP_KEY`, and `REVERB_APP_SECRET` to the credentials shared by the listener and publisher. Give clients the public key and WebSocket address. Keep the secret on the server.

## Running the server

Set `REVERB_ENABLED=true` for the default embedded mode. Starting the application binds both the HTTP and Reverb listeners. `REVERB_SERVER_HOST`, `REVERB_SERVER_PORT`, and `REVERB_SERVER_PATH` configure the Reverb listener. Use `REVERB_HOST`, `REVERB_PORT`, and `REVERB_SCHEME` for the destination used by the broadcaster. For a local listener without a TLS proxy, use `REVERB_SCHEME=http`.

For a separate process, set `REVERB_MODE=standalone` and start Reverb through Artisan:

```sh
./artisan reverb:start --host=127.0.0.1 --port=8080
./artisan reverb:restart
```

`reverb:start --hostname=edge.example.com` displays the public hostname alongside the bound address. It defaults to `REVERB_HOST`. The listener still binds to `--host` and `--port`. Use `--debug` for connection, protocol-event, and publication diagnostics in the terminal.

The restart command writes a signal to the application's cache. Use a shared cache store such as `CACHE_STORE=file` on one host. The standalone process closes its listeners and connections on receipt of the signal. A process manager can start it again. The embedded process follows the application's shutdown and restart lifecycle.

## Broadcasting and scaling {#broadcasting-and-scaling}

Set `BROADCAST_CONNECTION=reverb` and follow the [broadcasting guide](../digging-deeper/broadcasting.md) to enable private and presence authorization. Connect a Pusher protocol client to `/app/{key}` with the configured server path prefix. Set the application's `allowed_origins` to trusted host names for production and terminate TLS at the reverse proxy.

For outbound HTTPS publishing through the `reverb` or `pusher` connection, certificates and host names are verified by default. For a private CA, set `client_options.verify` in that connection in `config/broadcasting.rs` to an absolute PEM CA bundle path:

```rust
serde_json::json!({
    "client_options": { "verify": "/etc/openrust/reverb-ca.pem" }
})
```

The bundle replaces the default trust roots for that connection; the server certificate chain and host name still have to match. An unreadable, invalid, or empty bundle fails when the connection is resolved. Omit `verify` or use `true` for platform trust. `false` disables certificate verification and should only be used for isolated local debugging. Other Guzzle client options are not exposed through this setting.

Set `REVERB_SCALING_ENABLED=true` on each instance. Nodes for one app use the same `REVERB_APP_ID`, `REVERB_APP_KEY`, `REVERB_APP_SECRET`, `REVERB_SCALING_CHANNEL`, and Redis connection settings. Use `REDIS_URL` or the separate Redis settings. Give different apps distinct IDs on a shared scaling channel. All nodes on a scaling channel should run the same OpenRust Reverb version.

New presence subscriptions receive the cluster-wide member list and count. A user with multiple sockets counts once, and other subscribers see that user's first join and last leave. Graceful exits remove the sockets when Redis is reachable. After an abrupt exit, a surviving node with subscribers prunes expired sockets using a 10-second lease and a roughly 2-second check. A new subscription also removes expired entries before returning the roster.

Redis Pub/Sub delivers signed HTTP publications and authorized client events across nodes. Client events go to other subscribers in the same app and channel, excluding the sending socket. When a node loses its Redis subscription, it closes its presence sockets so clients reconnect and subscribe again after recovery. Delivery is best effort, with no replay of events missed during an outage.

## Pusher HTTP API and cache channels {#pusher-http-api}

Send signed requests to `POST /apps/{id}/events` or `POST /apps/{id}/batch_events`. A batch accepts an ordered `batch` array with an event name, channel, JSON-encoded `data`, and optional `socket_id` and `info` per item. The server validates the whole batch before dispatching it. Both endpoints can return the requested channel statistics.

Use signed GET requests to `/apps/{id}/connections`, `/apps/{id}/channels`, `/apps/{id}/channels/{channel}`, or `/apps/{id}/channels/{channel}/users`. Use `filter_by_prefix` to restrict a channel list and `info` to select fields such as `subscription_count`, `user_count`, or `cache`. With Redis scaling, the listener combines responding nodes' snapshots and deduplicates presence user IDs.

Channels beginning with `cache-`, `private-cache-`, or `presence-cache-` replay the last event after subscription succeeds, or emit `pusher:cache_miss` if there is no cached event. Channel authorization still applies. As in Reverb, each listener holds that cache while the channel has local subscribers and discards it when the last local subscriber leaves.

## Laravel Echo {#laravel-echo}

Install `laravel-echo` and `pusher-js`. This example connects to a local non-TLS Reverb listener. Set `wsPath` to the configured server path when using a path prefix.

```js
import Echo from "laravel-echo"
import Pusher from "pusher-js"

const echo = new Echo({
  broadcaster: "reverb",
  Pusher,
  key: "your-public-app-key",
  wsHost: window.location.hostname,
  wsPort: 8080,
  forceTLS: false,
  enabledTransports: ["ws"],
})

echo.channel("news").listen(".article.updated", event => console.log(event))
```

Use `echo.private("orders.42")` for private channels and `echo.join("rooms.42")` for presence channels. Both use the application’s `/broadcasting/auth` endpoint and registered channel guards. Echo subscribes again when its connection is restored.

## Pusher Channels {#pusher-channels}

The `reverb` Cargo feature also registers the `pusher` broadcast driver. Set `BROADCAST_CONNECTION=pusher`, `PUSHER_APP_ID`, `PUSHER_APP_KEY`, `PUSHER_APP_SECRET`, and `PUSHER_APP_CLUSTER`. The default publishing endpoint is `https://api-{cluster}.pusher.com:443`. A Pusher-compatible test endpoint can override `PUSHER_HOST`, `PUSHER_PORT`, and `PUSHER_SCHEME`.

For hosted Pusher Channels, configure Echo with `broadcaster: "pusher"`, the public key, and the cluster. Use the application's `/broadcasting/auth` and channel rules for private and presence subscriptions.
