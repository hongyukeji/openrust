# OpenRust 本地依赖补丁

来源：crates.io `imgalifier-avif 0.1.0` 发布包，保留原始许可证和包版本。

发布包 SHA-256（根锁文件）：`f4259408ebc637870778a8beb4a3bdb64da7ef45e7a2c945e38f43139ae9e488`。

将 paste 宏调用和依赖改为 pastey 0.1.1；保持原有宏展开接口。

参考：https://github.com/as1100k/pastey

采用此最小回移是为了移除停止维护的间接依赖，同时保持现有公开 API 和 Rust 1.94.0 最低版本。上游已发布版本仍依赖 paste，因此暂时保留最小本地回移。

当已发布、兼容项目最低 Rust 版本的上游包包含该修复时，升级上游包并删除本地补丁及根清单接线。
