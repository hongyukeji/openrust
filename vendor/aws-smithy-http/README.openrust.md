# OpenRust 本地依赖补丁

来源：crates.io `aws-smithy-http 0.63.6` 发布包，保留原始许可证和包版本。

发布包 SHA-256（根锁文件）：`ba1ab2dc1c2c3749ead27180d333c42f11be8b0e934058fb4b2258ee8dbe5231`。

回移上游删除未使用 pin-utils 依赖的改动。

参考：https://github.com/smithy-lang/smithy-rs/commit/66b12d3e2165f98ff62146b46fb248c5845e8ea4

采用此最小回移是为了移除停止维护的间接依赖，同时保持现有公开 API 和 Rust 1.94.0 最低版本。未将上游新版本要求的 Rust 1.94.1 引入应用。

当已发布、兼容项目最低 Rust 版本的上游包包含该修复时，升级上游包并删除本地补丁及根清单接线。
