# 配置读取公开 API consumer

固定业务名称通过 `string_or` 严格读取，类型不符返回不含原值的 `ConfigError`；生成 fixture 也断言运行期写入后的 `string` 读取。

本工程作为独立 Cargo package，从仓库路径依赖公开 `openrust::config::repository::ConfigRepository` 与 `ConfigSources`。`src/lib.rs` 的业务函数只读写预先选定的 `app.name`、`app.locale`，不接受请求方提供的键或值，也不把 `all()` 输出到 HTTP。`POST /api/config-reading/review` 只把 `app.name` 写为固定的“已复核应用”；后续 `GET /api/config-reading` 读取同一仓储。`GET /api/config-reading/order` 只返回 `reader` 区段中固定的 `z_label`、`a_label` 键名，展示来源声明顺序。写入是当前应用内存状态，`App::config()` 保持启动快照。

在本目录运行 `cargo test`；读取测试覆盖缺失、显式 null、默认值、false/零/空字符串、字面点键、数组索引、批量键顺序及独立快照。写入和有序输入测试待集中验证。

生成项目同样通过公开 `openrust` 依赖编译，不需要复制框架私有源码。测试 fixture 显式选择 `cache.default="default"` 并登记 memory store，避免把缓存选择误交给框架缺省的具名 `redis` store。其应用 Provider 清单可显式加入 `config_repository_reading_consumer::ReadingProvider`；fixture 在所有 boot 覆盖完成后经 `BootContext::config_repository()` 取得同一仓储，并登记固定业务读写路由。安装脚本额外建立自有 `config/reader.toml`，按 `z_label`、`a_label` 顺序声明两个公开业务键；生成应用的真实 server 可通过 `GET /api/config-reading/order` 验证默认配置加载入口。生成器当前只建立空 `modules/`，不会自动复制本仓唯一的 `modules/example`。

总控生成全新应用后，从本分支根运行 `python3 -B tests/consumers/config-repository-reading/scripts/install_generated_fixture.py <生成应用目录> "$PWD"`。脚本先检查生成应用的 `openrust` 依赖确实指向本分支，以及测试文件和 `config/reader.toml` 均不存在，再在应用自己的 manifest、bootstrap、tests 和 config 中显式安装 fixture；回执为四个文件分别记录安装前状态及安装后 SHA-256。随后在生成应用执行 `cargo check` 和 `cargo test generated_application_installs_config_reading_provider -- --exact`；实际过滤应出现一个测试，不能用零测试通过作证据。
