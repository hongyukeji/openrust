# Envoy

## Support status {#support-status}

**Not included in this source line.** No Envoy task-file parser or remote deployment command is shipped. Native process and concurrency services can run application-owned commands; they are not an Envoy task runtime.

This chapter describes the currently supported integration boundary; the navigation entry does not install an additional package.

## Available entry points {#available-entry-points}

See [Processes](../digging-deeper/processes.md) for the native interfaces and usage. When integrating a separate library or service, implement the application adapter and validate its lifecycle, errors and credentials against that service's contract.

## Scope {#scope}

OpenRust does not provide an installation command for this integration. Use the native interfaces and commands documented for this integration.
