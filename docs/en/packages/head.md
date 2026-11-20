# Head

## Support status {#support-status}

**Not included in this source line.** No standalone Head package or Head service API is shipped. Page-head markup can be authored in Blade layouts and components.

This chapter retains the Laravel navigation path for comparison. Its presence does not install a package or promise compatibility with the corresponding PHP API.

## Available entry points {#available-entry-points}

See [Blade Templates](../the-basics/blade.md) for the native interfaces and usage. When integrating a separate library or service, implement the application adapter and validate its lifecycle, errors and credentials against that service's contract.

## Scope {#scope}

OpenRust does not provide an installation command for this integration. Do not copy a Laravel Composer or Artisan command into OpenRust on the assumption that this navigation entry implements it.
