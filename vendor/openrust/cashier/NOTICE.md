# Upstream attribution

Selected customer API branches and public contracts adapt Laravel Cashier Stripe and Paddle. HTTP transport, operation claims and transactional storage are independent Rust implementations using OpenRust services; no PHP SDK is bundled. The upstream MIT notices are retained verbatim. OpenRust additions are covered by the repository LICENSE.

- laravel/cashier-stripe at `d5c015490ff77f2c138fe79b86d2a982787fe3ac`: https://github.com/laravel/cashier-stripe/tree/d5c015490ff77f2c138fe79b86d2a982787fe3ac. License: [LICENSE-cashier-stripe.md](LICENSE-cashier-stripe.md).
- laravel/cashier-paddle at `82bfc68eb72c38e666a4900baeecca3621840259`: https://github.com/laravel/cashier-paddle/tree/82bfc68eb72c38e666a4900baeecca3621840259. License: [LICENSE-cashier-paddle.md](LICENSE-cashier-paddle.md).

Vendor HTTP protocols and signature verification use OpenRust HTTP and existing cryptography. No PHP vendor SDK is bundled; SDK references do not imply those SDKs execute in Rust.
