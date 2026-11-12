# Contribution Guide

## Report a problem

Include the source revision, operating system, Rust version, and the smallest reproducible example. For HTTP or Artisan behavior, include the request or command, the observed result, and the expected result. Include the selected database or driver when relevant.

## Make a change

Keep application integration in the root application and framework code in `vendor/openrust/framework/`. Follow the fixed Laravel baseline and the project's Rust conventions. Update the English, Simplified Chinese, and Traditional Chinese guides together when public behavior changes.

Run the repository checks from the root:

```sh
cargo +1.94.0 fmt --all --check
cargo +1.94.0 test --workspace --locked
cargo +1.94.0 clippy --workspace --all-targets --locked -- -D warnings
```

Use actual HTTP, Artisan or backend requests for behavior specific to those entry points. State the environment and result in the contribution. For documentation changes, see the [documentation workspace](https://github.com/hongyukeji/openrust/tree/master/docs).
