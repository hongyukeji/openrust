# Contracts

A contract defines a service interface independently of its implementation. In Rust, contracts are traits under `openrust::contracts`, and container bindings use `Arc<dyn Trait>` when an interface is required.

| Responsibility | Import |
| --- | --- |
| Service bindings and resolution | `openrust::contracts::container::Container` |
| Container errors | `openrust::contracts::container::BindingResolutionError` |
| Application services and bootstrap state | `openrust::contracts::foundation::Application` |
| Configuration repository | `openrust::contracts::config::Repository` |

Import a contract as `use …::Container as _;` to call its methods without shadowing `openrust::container::Container`. Register an implementation through a provider and resolve the trait rather than coupling callers to a concrete type. See [container binding](../architecture-concepts/container.md#binding) and [providers](../architecture-concepts/providers.md).

Rust trait implementations replace PHP interface declarations. There is no automatic runtime reflection: declare the trait binding and constructor dependencies in a factory. This page covers the contracts used by the startup and container baseline. Other contract files may still be skeletons and are accepted with their respective modules.
