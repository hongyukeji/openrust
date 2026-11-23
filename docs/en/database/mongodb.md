# MongoDB

## Support status {#support-status}

**Not included in this source line.** The current database layer does not include a MongoDB driver, BSON query builder or MongoDB Eloquent adapter. SQL Eloquent APIs do not imply MongoDB support.

This chapter states the supported integration boundary; the navigation entry does not install an additional package.

## Available entry points {#available-entry-points}

See [Database](getting-started.md) for the native interfaces and usage. When integrating a separate library or service, implement the application adapter and validate its lifecycle, errors and credentials against that service's contract.

## Scope {#scope}

OpenRust does not provide an installation command for this integration. Use the native interfaces and commands documented for this integration.
