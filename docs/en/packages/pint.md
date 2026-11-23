# Pint

## Support status {#support-status}

**Not included in this source line.** Rust source is formatted with rustfmt and checked with Clippy. No Pint executable, PHP formatter or pint Artisan command is shipped.

This chapter describes the currently supported integration boundary; the navigation entry does not install an additional package.

## Available entry points {#available-entry-points}

See [Contribution Guide](../prologue/contributions.md) for the native interfaces and usage. When integrating a separate library or service, implement the application adapter and validate its lifecycle, errors and credentials against that service's contract.

## Scope {#scope}

OpenRust does not provide an installation command for this integration. Use the native interfaces and commands documented for this integration.
