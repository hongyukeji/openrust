
## 开发与部署

在源码目录运行 `cargo run`。`./artisan` 默认列出应用命令。

在源码目录用 Docker Buildx 导出 Linux 发布目录。按部署主机选择平台。

```bash
./artisan build --platform linux/arm64
# 输出 dist/{{name}}-linux-arm64
```

默认按构建机 CPU 选 Linux 平台。可用 `--output` 指定新目录，父目录须已存在。用 `./artisan help build` 查看选项，用 `--env` 选择构建插值使用的 dotenv。构建机需保留当前应用与 OpenRust 源码的相对位置。项目名与发布保留名称冲突时，构建会拒绝创建目标。

发布时搬移整个目录，保留可执行权限，并为实际运行账号开放 `storage/` 和 `bootstrap/cache/` 的写入权限。生产环境与服务凭据在目标机配置。应用使用数据库时，先从发布目录运行 `./artisan migrate` 并确认生产操作，再运行包根的 `./{{name}}`。目标机须支持 Debian Bookworm 构建环境的 Linux ABI。

用 `docker compose up --build -d --wait` 构建并运行镜像。镜像启动 `/app/{{name}}`，Dockerfile 的 `artifacts` 阶段导出 `{{name}}` 和 `artisan`，两者可随部署目录一起移动。

生产环境的无人值守迁移须显式使用 `./artisan migrate --force --no-interaction`。Compose 的迁移服务已使用该命令。
