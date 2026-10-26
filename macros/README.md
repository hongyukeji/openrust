# openrust-macros

Procedural macros of the [OpenRust](https://github.com/hongyukeji/openrust) framework. `#[openrust::resource]` generates a `Resource` implementation from action methods in an inherent controller `impl`. `#[derive(Job)]` and `#[derive(Event)]` are planned for milestone M4.

Action methods are named `list`, `get`, `create`, `update`, and `delete`; each must be an associated function without a receiver or method generics. Other methods remain ordinary helpers.

```rust,ignore
#[openrust::resource]
impl Posts {
    async fn list() -> Json<Vec<Post>> { /* ... */ }
    async fn get() -> Json<Post> { /* ... */ }
}
```
