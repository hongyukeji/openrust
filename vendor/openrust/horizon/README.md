# OpenRust Horizon

Redis queue process supervision, bounded automatic balancing, job records and tags,
metric snapshots and an explicitly authorized operations dashboard. Enable the `horizon` feature,
configure existing Redis queue connections, then run `./artisan horizon`.

The package Provider registers configuration-driven HTTP routes. The application supplies authorization; access is denied by default. Set `horizon.routes` to `false` for explicit route registration.
See the [guide](../../../docs/en/packages/horizon.md).
