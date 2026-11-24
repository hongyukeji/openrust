# Starter Kits

## Support status {#support-status}

**The complete installer and UI are not included.** No ready-made starter-kit UI, dashboard or React/Vue/Livewire kit is shipped. The native init wizard and Fortify provide application setup and headless authentication; applications supply their own pages.

This chapter states the supported integration boundary; the navigation entry does not install an additional package.

## Available entry points {#available-entry-points}

See [Fortify](../packages/fortify.md) for the native interfaces and usage. When integrating a separate library or service, implement the application adapter and validate its lifecycle, errors and credentials against that service's contract.

## Scope {#scope}

OpenRust does not provide an installation command for this integration. Use the native interfaces and commands documented for this integration.

## Optional JSON responses {#json-responses}

The complete starter-kit installer and UI are not yet available. An opt-in response adapter is available for applications that already configure Fortify actions and routes.

Register `openrust_fortify::starter::StarterKitServiceProvider` after your Fortify providers. HTML requests keep native redirects and flash messages. JSON login, registration and completed two-factor login return the authenticated user through `ApiResponse`; registration uses 201. Updates omit empty data, logout and verification retain empty 204 responses. Messages follow the current application locale.

```rust
Box::new(openrust_fortify::starter::StarterKitServiceProvider),
```

The default `UserResource` reads only `id`, `name`, `email` and `email_verified_at`, preserving string identifiers. It never enumerates model attributes or reads credentials. Replace the existing Fortify response binding to use your own resource. Successful authentication responses use `Cache-Control: no-store`. This adapter does not install pages, change authentication guards or issue personal access tokens.
