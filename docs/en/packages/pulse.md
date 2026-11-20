# Pulse

## Support status {#support-status}

**Not included in this source line.** No Pulse dashboard, recorders or Pulse aggregation schema is shipped. Existing logging and context APIs remain available for application diagnostics.

This chapter retains the Laravel navigation path for comparison. Its presence does not install a package or promise compatibility with the corresponding PHP API.

## Available entry points {#available-entry-points}

See [Logging](../the-basics/logging.md) for the native interfaces and their tested scope. When integrating a separate library or service, implement the application adapter and validate its lifecycle, errors and credentials against that service's contract.

## Scope {#scope}

There is no verified installation command for the missing integration. Do not copy a Laravel Composer or Artisan command into OpenRust on the assumption that this navigation entry implements it. Future ports require their own public API, configuration, tests and three-language documentation.
