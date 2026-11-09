
## 开发与部署

在源码目录运行 `cargo run`。`./artisan` 默认列出应用命令。

在目标平台运行 `cargo build --release --bin {{name}}`，将生成的 `target/release/{{name}}` 安装为部署目录中的 `bin/{{name}}`。保留可执行的 `artisan` 与应用的公开资源、模板和语言文件。配置部署环境后，在部署目录运行 `./bin/{{name}}` 启动应用。应用使用数据库时，先运行 `./artisan migrate`。

用 `docker compose up --build -d --wait` 构建并运行镜像。镜像启动 `/app/bin/{{name}}`，Dockerfile 的 `artifacts` 阶段导出 `bin/{{name}}` 和 `artisan`，两者可随部署目录一起移动。
