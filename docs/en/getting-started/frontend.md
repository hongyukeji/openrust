# Frontend

## Support status {#support-status}

**Partial native support.** Server-rendered Blade views, public files and JSON responses are available. This source line does not ship a Vue/React application scaffold, an Inertia adapter or a frontend package manager workflow.

This chapter retains the Laravel navigation path for comparison. Its presence does not install a package or promise compatibility with the corresponding PHP API.

## Available entry points {#available-entry-points}

See [Views](../the-basics/views.md) for the native interfaces and their tested scope. When integrating a separate library or service, implement the application adapter and validate its lifecycle, errors and credentials against that service's contract.

## Scope {#scope}

There is no verified installation command for the missing integration. Do not copy a Laravel Composer or Artisan command into OpenRust on the assumption that this navigation entry implements it. Future ports require their own public API, configuration, tests and three-language documentation.
