# 升级指南

## 从 v0.x 源码迁移

准备新的检出目录，对照当前的 `.env.example` 逐项迁移应用设置。按现有启动结构接入应用路由、Provider 和配置。将 `APP_SERVER_HOST` 迁移为 `APP_HOST`，将 `SERVER_PORT` 迁移为 `APP_PORT`，并核对公开的 `APP_URL`。重新构建并部署 `main`。源码命令使用 `./artisan`，部署后使用 `./main artisan`。 [应用生命周期](../architecture-concepts/lifecycle.md)

## 保留应用数据

备份数据库和持久化存储。现有数据依赖生产密钥时保留该密钥。用旧环境的副本验证加密数据、Cookie 和会话。对照当前迁移核对数据库结构和迁移账本，在副本先执行迁移及应用自己的数据转换。 [迁移](../database/migrations.md) · [加密](../security/encryption.md)

## 验证部署

使用锁文件构建新产物，在预发布环境验证 `/up`、应用路由、Artisan 和启用的队列 worker。核对部署后的视图、翻译和公共文件路径。保留上一版二进制和部署设置。旧二进制无法读取新数据库结构时，回滚应用须配合数据库恢复或已验证的逆向迁移。 [部署](../getting-started/deployment.md)
