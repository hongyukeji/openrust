# Asset Bundling / Vite

## Support status {#support-status}

**Partial native support.** Blade supports the documented @fonts consumer for version 1 Vite font manifests. A full Vite integration, JavaScript/CSS entrypoint manifest resolver and frontend dev-server lifecycle are not shipped.

This chapter states the supported integration boundary; the navigation entry does not install an additional package.

## Available entry points {#available-entry-points}

See [Blade Templates](blade.md) for the native interfaces and usage. When integrating a separate library or service, implement the application adapter and validate its lifecycle, errors and credentials against that service's contract.

## Scope {#scope}

OpenRust does not provide an installation command for this integration. Use the native interfaces and commands documented for this integration.
