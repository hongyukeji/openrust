
## 开发与部署

在源码目录运行 `cargo run`。`./artisan` 默认列出应用命令。

在 macOS 或 Linux 的源码目录编译应用。默认用本机 Cargo，也可显式用 Docker 构建 Linux 产物。

```bash
./artisan build
./artisan build --docker --platform linux/arm64
# 输出 dist/{{name}}-linux-arm64
```

本机构建可用 `--features` 启用应用的 Cargo features。已有 `Cargo.lock` 时使用锁定依赖。Docker 模式默认按构建机 CPU 选 Linux 平台，支持用 `--env` 选择 Compose 插值使用的 dotenv。可用 `--output` 指定新目录，父目录须已存在。用 `./artisan help build` 查看选项。构建机需保留当前应用与 OpenRust 源码的相对位置。项目名与发布保留名称冲突时，构建会拒绝创建目标。

发布时搬移整个目录，保留可执行权限，并为实际运行账号开放 `storage/` 和 `bootstrap/cache/` 的写入权限。生产环境与服务凭据在目标机配置。运行 `./{{name}} artisan` 列出命令，或用 `./{{name}} artisan help migrate` 查看帮助。应用使用数据库时，先运行 `./{{name}} artisan migrate` 并确认生产操作，再运行 `./{{name}}` 启动。目标机须匹配产物的系统、架构和动态库，Docker 产物按 Debian Bookworm 的 Linux ABI 构建。

用 `docker compose up --build -d --wait` 构建并运行镜像。镜像启动 `/app/{{name}}`，Dockerfile 的 `artifacts` 阶段导出 `{{name}}` 和 `artisan`，两者可随部署目录一起移动。

生产环境的无人值守迁移须显式使用 `./{{name}} artisan migrate --force --no-interaction`。包根脚本 `./artisan` 进入同一控制台，Compose 的迁移服务通过该脚本执行强制迁移。
