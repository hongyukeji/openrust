# Mix

## Support status {#support-status}

**Not included in this source line.** No Mix/Webpack integration, mix helper or Mix manifest reader is shipped. Public assets can be built by application-owned tooling and served from the public directory.

This chapter describes the currently supported integration boundary; the navigation entry does not install an additional package.

## Available entry points {#available-entry-points}

See [Frontend](../getting-started/frontend.md) for the native interfaces and usage. When integrating a separate library or service, implement the application adapter and validate its lifecycle, errors and credentials against that service's contract.

## Scope {#scope}

OpenRust does not provide an installation command for this integration. Use the native interfaces and commands documented for this integration.
