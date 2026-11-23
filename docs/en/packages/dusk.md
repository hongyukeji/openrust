# Browser Tests / Dusk

## Support status {#support-status}

**Not provided.** OpenRust does not include a Dusk package, WebDriver manager or `dusk` Artisan command. Use an external browser testing tool against your running application.

This chapter describes the currently supported integration boundary; the navigation entry does not install an additional package.

## Available entry points {#available-entry-points}

See [HTTP Tests](../testing/http-tests.md) for the native interfaces and usage. When integrating a separate library or service, implement the application adapter and validate its lifecycle, errors and credentials against that service's contract.

## Scope {#scope}

OpenRust does not provide an installation command for this integration. Use the native interfaces and commands documented for this integration.
