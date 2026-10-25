# OpenRust

> An opinionated, batteries-included Rust application framework. Create a project, run one command, and start writing business code.

**Status: pre-alpha, design phase.** Nothing is published to crates.io yet. The first generation delivers the framework foundation: project generator, configuration, HTTP conventions, database (PostgreSQL and MySQL), authentication and authorization, queue, scheduler, events, cache, storage, mail, CLI, testing tools and deployment path.

## Goals

- **Convention over configuration, without magic.** Directory layout, naming, routing, migrations and configuration keys follow visible conventions. Dependencies are explicit Rust types; there is no runtime container, no reflection, no facade.
- **One obvious way.** Each capability has one official entry point: `Validated<T>` for input, `Policy` for authorization, `Job` for background work.
- **Batteries included, drivers swappable.** Database queue, in-process cache, local storage and SMTP mail work out of the box; Redis and S3-compatible drivers are Cargo features.
- **Generated code is ordinary code.** Everything `openrust generate` produces is readable, editable and testable Rust.
- **Testable by default.** Every capability ships a fake driver and assertions, so business tests never need a live Redis, SMTP server or object store.

OpenRust is built on `tokio`, `axum`, `tower` and `SeaORM`, with explicit Rust types and conventional project structure.

## Run in this repository

The non-published [integration server](apps/server/README.md) runs the currently implemented framework capabilities without external services:

```bash
cargo run
curl --noproxy '*' http://127.0.0.1:3000/health
cargo run -p openrust-server -- config show
cargo run -p openrust-server -- help
cargo test -p openrust-server
```

The repository root `config` link points to `apps/server/config`, so `cargo run` reads the server's configuration. `OPENRUST_SERVER__PORT=4000 cargo run` overrides its port; `PORT=4000 cargo run` also works.

## The golden path

```bash
cargo install openrust-cli
openrust new blog --db postgres
cd blog
openrust dev
```

After `openrust dev` the database and mail catcher are running, migrations are applied, the server listens on `http://localhost:3000`, `/health` returns 200, `/docs` serves the OpenAPI UI, auth endpoints work and `cargo test` is green.

## Documentation

Design and plan documents live in [`.agents/project/plans/`](.agents/project/plans/README.md) during the design phase and will move to a public documentation site before the 0.1 release.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
