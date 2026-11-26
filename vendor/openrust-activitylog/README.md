# OpenRust Activitylog

Fluent activity logging, Eloquent model event recording, scoped logging status
and causer resolution, lifecycle buffering, typed morph relationships, custom
activity models and actions, transaction writes, queries and retention cleanup.
Explicit properties are retained by default; redaction is an opt-in policy.

Enable `activitylog` in the application's default features or add this package
using package discovery. Run `./artisan migrate`; publish configuration with
`./artisan vendor:publish --tag=activitylog-config`.
See the [guide](../../docs/en/packages/activitylog.md) for configuration,
model registration, execution scopes and storage limitations.
