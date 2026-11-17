# Broadcasting

## Broadcast connections

Set `BROADCAST_CONNECTION` to the connection name defined in `config/broadcasting.rs`. The default `null` connection discards deliveries. The `log` driver writes the channel names, event name, payload, and originating socket to the application log.

A provider can resolve `BroadcastManager` and call `extend` with a driver name and constructor. The manager passes the selected connection's configuration to that constructor. Drivers implement `Broadcaster` to deliver events and produce authorization responses.

## Authorizing channels

Register channel rules with `Broadcast::channel` and load them through `Routing::channels` or `withBroadcasting`. This enables GET and POST `/broadcasting/auth` in the web middleware group. The route excludes request forgery middleware and still requires the driver's authorization checks.

Rules such as `orders.{id}` receive the authenticated user and named parameters. Return `ChannelAuthorization::Allowed` for an authorized private subscription or `ChannelAuthorization::Presence(data)` to expose the specified member data. Use `channelWithOptions` to select the guard lookup order. Run `artisan channel:list` to inspect the registered patterns.

Use `channelWithBindings` with `ChannelBindings::default().model::<Order>("order")` for `orders.{order}`. Implement `ChannelHandler::joinBound` and read the model with `models.get::<Order>("order")`. This uses the model’s route key, including a custom key such as a slug. A missing implicit model denies authorization with HTTP 403. Explicit router bindings registered with `Router::bind` or `Router::model` take precedence for the same parameter name. A missing explicit binding returns HTTP 404. Channel models are resolved independently of one another.

Overlapping rules run in registration order using the first matching pattern's guards. Return `ChannelAuthorization::Abstain` to try the next rule, or `Denied` to deny the request immediately.

## Defining broadcast events

Implement `ShouldBroadcast` and register the compiled type with `Dispatcher::registerBroadcast::<E>()` in a provider. Normal event dispatch then queues the delivery. For an event implementing `ShouldBroadcastNow`, use `registerBroadcastNow::<E>()` to deliver it synchronously.

`broadcastOn` selects public, private, or presence channels. `broadcastAs` and `broadcastWith` customize the name and payload. Defaults are the event's name and the fields explicitly returned from `broadcastData`. A false `broadcastWhen` skips delivery. The queue stores the serialized delivery data.

Mark an event implementing `ShouldDispatchAfterCommit` with `Dispatcher::markAfterCommit::<E>()` to defer its dispatch when a transaction is active. Queued broadcasts also honor `broadcastAfterCommit` and the queue connection's `after_commit` setting. Use `EventContext::toOthers(&request)` to forward the request's `X-Socket-ID` to the driver for exclusion.

## Sending anonymous broadcasts {#anonymous-broadcasting}

Use `Broadcast::on("news")`, `Broadcast::private("orders.42")`, or `Broadcast::presence("rooms.42")` to build an anonymous broadcast. `Broadcast::on` also accepts a list of channels. Set the wire name with `r#as`, the JSON payload with `with`, and the broadcast connection with `via`. Without a custom name, the wire name is `AnonymousEvent`.

Await `send()` for normal queue delivery or `sendNow()` for synchronous delivery. `toOthers(&request)` excludes the source socket when the request includes `X-Socket-ID`. Other sends include the source socket in the audience.

## Unique queued broadcasts {#unique-broadcasts}

Implement `contracts::broadcasting::ShouldBeUnique` and register the type with `Dispatcher::registerUniqueBroadcast::<E>()`. Uniqueness is scoped to the Rust event type and its `uniqueId()`. Use `uniqueFor()` to set a lease in seconds and `uniqueVia()` to select a named cache store. The defaults are an empty ID, a lock with no expiration, and the default cache store.

The queue emits `UniqueJobSkipped` when a lock already exists. The lock remains held across retries and is released when the worker succeeds or the job fails permanently. Synchronous broadcasts do not take a uniqueness lock. Within an explicit Rust transaction, unique queue deliveries acquire the lock at commit. A rollback drops the pending delivery. An active transaction rejects an explicit `broadcastAfterCommit() == Some(false)` on a unique delivery.

The built-in connections are `null` and `log`. Enable the [Reverb package](../packages/reverb.md) for a Pusher-compatible transport.
