# Prompts

## Support status {#support-status}

**Partial native support.** Native console input, confirmation, choices and secret input are documented in Artisan. There is no complete the framework Prompts port or standalone Prompts package.

This chapter describes the currently supported integration boundary; the navigation entry does not install an additional package.

## Available entry points {#available-entry-points}

See [Artisan Console](../digging-deeper/artisan.md) for the native interfaces and usage. When integrating a separate library or service, implement the application adapter and validate its lifecycle, errors and credentials against that service's contract.

## Scope {#scope}

OpenRust does not provide an installation command for this integration. Use the native interfaces and commands documented for this integration.
