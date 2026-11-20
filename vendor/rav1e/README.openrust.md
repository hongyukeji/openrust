# OpenRust 本地依赖补丁

来源：crates.io `rav1e 0.8.1` 发布包，保留原始许可证和包版本。

发布包 SHA-256（根锁文件）：`43b6dd56e85d9483277cde964fd1bdb0428de4fec5ebba7540995639a21cb32b`。

将 paste 宏调用和依赖改为 pastey 0.1.1；保持原有宏展开接口。

参考：https://github.com/xiph/rav1e/commit/c247d53ae43dd1312dbd90117c45e4c0ee6b06ce

采用此最小回移是为了移除停止维护的间接依赖，同时保持现有公开 API 和 Rust 1.94.0 最低版本。上游已发布版本仍依赖 paste，因此暂时保留最小本地回移。

当已发布、兼容项目最低 Rust 版本的上游包包含该修复时，升级上游包并删除本地补丁及根清单接线。
