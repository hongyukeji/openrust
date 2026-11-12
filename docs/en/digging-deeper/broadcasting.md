# Broadcasting

## Broadcast connections

Set `BROADCAST_CONNECTION` to the connection name defined in `config/broadcasting.rs`. The default `null` connection discards deliveries. The `log` driver writes the channel names, event name, payload, and originating socket to the application log.

A provider can resolve `BroadcastManager` and call `extend` with a driver name and constructor. The manager passes the selected connection's configuration to that constructor. Drivers implement `Broadcaster` to deliver events and produce authorization responses.

## Authorizing channels

Register channel rules with `Broadcast::channel` and load them through `Routing::channels` or `withBroadcasting`. This enables GET and POST `/broadcasting/auth` in the web middleware group. The route excludes request forgery middleware and still requires the driver's authorization checks.

Rules such as `orders.{id}` receive the authenticated user and named parameters. Return `ChannelAuthorization::Allowed` for an authorized private subscription or `ChannelAuthorization::Presence(data)` to expose the specified member data. Use `channelWithOptions` to select the guard lookup order. Run `artisan channel:list` to inspect the registered patterns.

## Defining broadcast events

Implement `ShouldBroadcast` and register the compiled type with `Dispatcher::registerBroadcast::<E>()` in a provider. Normal event dispatch then queues the delivery. For an event implementing `ShouldBroadcastNow`, use `registerBroadcastNow::<E>()` to deliver it synchronously.

`broadcastOn` selects public, private, or presence channels. `broadcastAs` and `broadcastWith` customize the name and payload. Defaults are the event's name and the fields explicitly returned from `broadcastData`. A false `broadcastWhen` skips delivery. The queue stores the serialized delivery data.

Mark an event implementing `ShouldDispatchAfterCommit` with `Dispatcher::markAfterCommit::<E>()` to defer its dispatch when a transaction is active. Queued broadcasts also honor `broadcastAfterCommit` and the queue connection's `after_commit` setting. Use `EventContext::toOthers(&request)` to forward the request's `X-Socket-ID` to the driver for exclusion.

The built-in connections are `null` and `log`. Enable the [Reverb package](../packages/reverb.md) for a Pusher-compatible transport.
