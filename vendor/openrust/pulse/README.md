# OpenRust Pulse

Optional application performance metrics with native package discovery, database aggregation, Redis ingestion, recorders, Artisan commands and an authorized dashboard.

Enable the application's `pulse` feature, run `./artisan migrate`, then open `/pulse`. Outside the local environment, authorize the `viewPulse` Gate. Package configuration can be published with `./artisan vendor:publish --tag=pulse-config`.

The public entry points are `Pulse`, `facades::Pulse`, `PulseServiceProvider`, `recorders::Recorder`, `contracts::{Storage, Ingest, ResolvesUsers}` and the native recorder types. The dashboard uses native HTML and JavaScript; application templates replace PHP Livewire components. See the application's Pulse guide for configuration, lifecycle behavior and supported storage.
