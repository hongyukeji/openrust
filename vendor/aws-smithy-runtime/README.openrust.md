# OpenRust 本地依赖补丁

来源：crates.io `aws-smithy-runtime 1.11.3` 发布包，保留原始许可证和包版本。

发布包 SHA-256（根锁文件）：`b8e6f5caf6fea86f8c2206541ab5857cfcda9013426cdbe8fa0098b9e2d32182`。

回移上游移除 pin-utils 的改动；实际局部固定使用标准库 std::pin::pin!。

参考：https://github.com/smithy-lang/smithy-rs/commit/66b12d3e2165f98ff62146b46fb248c5845e8ea4

采用此最小回移是为了移除停止维护的间接依赖，同时保持现有公开 API 和 Rust 1.94.0 最低版本。未将上游新版本要求的 Rust 1.94.1 引入应用。

当已发布、兼容项目最低 Rust 版本的上游包包含该修复时，升级上游包并删除本地补丁及根清单接线。
