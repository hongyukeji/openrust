# Docker 本机运行环境

唯一配置为 `docker/docker-compose.yml`，从仓库根目录直接使用原生 Docker Compose。只加载项目根 `.env`：

```sh
docker compose --env-file .env -f docker/docker-compose.yml up -d --wait postgres mysql redis
docker compose --env-file .env -f docker/docker-compose.yml ps
```

Docker专用配置示例位于docker/.env.example，仅供按需合并到根.env，不创建docker/.env。仅根.env提供配置，Shell插值按Compose原生规则优先。镜像为`${APP_IMAGE_NAME:-openrust}:${APP_IMAGE_VERSION:-latest}`，容器使用`${APP_NAME}-服务名`。APP_IMAGE_NAME须为合法小写仓库名；APP_IMAGE_VERSION指定发布标签，默认latest。Compose不读取Cargo或其他环境文件，不增加启动脚本。

根 `.env` 中DB_HOST／DB_PORT／REDIS_HOST供宿主应用使用；DOCKER_DB_HOST／DOCKER_DB_PORT／DOCKER_REDIS_HOST只覆盖容器连接，未设置时复用普通字段，再回退postgres／5432／redis。本机PostgreSQL组合设置DOCKER_DB_HOST=postgres、DOCKER_REDIS_HOST=redis；MySQL使用DOCKER_DB_HOST=mysql和DOCKER_DB_PORT=3306。外部服务可省略覆盖，直接使用普通字段中的实际地址。已有库名、root/root账号及APP_KEY保留；MySQL仍通过MYSQL_ROOT_PASSWORD初始化，环境文件变更不会自动修改实际账号。

PostgreSQL默认127.0.0.1:5432，MySQL默认127.0.0.1:3306，Redis默认127.0.0.1:6379。POSTGRES_PORT／MYSQL_PORT只改变宿主映射，容器数据库端口仍为5432／3306。REDIS_PORT同时改变Redis内部监听、宿主映射、健康检查和应用连接。

首次先启动依赖，再按[部署文档](../../../docs/zh-Hans/getting-started/deployment.md#docker-deployment)准备可写存储、构建镜像、生成缺失密钥并迁移。已有APP_KEY必须保留。初始化后启动全部五服务（含 Nginx）：

```sh
docker compose --env-file .env -f docker/docker-compose.yml up -d --wait
```

原生up默认启动完整环境。app等待三项依赖健康，内部固定监听0.0.0.0:8000，宿主绑定复用APP_HOST／APP_PORT。外部服务模式只启动应用时加--no-deps；一次性Artisan可用run --rm --no-deps app artisan ...。

应用挂载路径可用APP_STORAGE_PATH和APP_PUBLIC_PATH调整；Dockerfile构建参数使用APP_BUILD_FEATURES及APP_BUILD_JOBS，镜像使用APP_IMAGE_NAME及APP_IMAGE_VERSION，统一采用APP_前缀。

数据目录为docker/data/postgres、mysql、redis，应用存储为docker/data/storage。PG18挂载整个/var/lib/postgresql；public只读挂载。所有服务使用unless-stopped。暂停用同一Compose命令的stop；不删除数据目录和数据卷。

## 数据迁移记录

2026-11-21 已将旧 `openrust-dev` 服务干净停止，把 `docker/data/dev/` 下三个完整数据目录同盘移动至上述路径，再按最新配置启动 `openrust`。迁移前后 PostgreSQL、MySQL 完整 SQL 导出一致，Redis 键空间均为空；三个服务健康检查和宿主映射端口连接通过。旧容器和已空的旧网络、目录已移除，未删除数据卷。SQL 备份及验证记录位于项目内忽略目录 `tmp/compose-data-migration-20261121/`，其中 SQL 包含数据库数据和账号信息，不应提交或公开。

上述迁移记录对应当时配置；当前运行与验证结果见[当前Docker核对](../audits/08-04/docker.md#当前配置与关联资料核对2026-11-21)。

## Nginx 对外入口

默认随完整环境启动，转发至 `app:8000`，等待应用健康后启动。默认对外绑定 `0.0.0.0:8080`；根 `.env` 可设置 `NGINX_HOST` 和 `NGINX_PORT`，Nginx 内部端口始终为 `80`。`APP_URL` 设为实际对外地址。配置文件为 `docker/nginx/default.conf`，支持 WebSocket、流式响应及应用重建后的 DNS 地址刷新。仅运行外部依赖应用时，可在应用就绪后用 `docker compose --env-file .env -f docker/docker-compose.yml up -d --no-deps nginx` 启动代理。

Nginx请求体上限8 MiB，与框架输入限制一致；代理读写超时沿用Nginx默认60秒，流式／WebSocket应及时传送数据或心跳；有明确需求时再调整超时。Compose已传入APP_BUILD_JOBS（默认4）。更新及回滚步骤见[部署文档](../../../docs/zh-Hans/getting-started/deployment.md#更新与回滚)，迁移显式执行一次；镜像回滚不会回滚数据。
