# 配置读取公开 API consumer

本工程作为独立 Cargo package，从仓库路径依赖公开 `openrust::config::repository::ConfigRepository`。`src/lib.rs` 的业务函数只读取预先选定的 `app.name`、`app.locale`，不接受请求方提供的键，也不把 `all()` 输出到 HTTP。

在本目录运行 `cargo test`；本分支已有 3 个 consumer 测试通过。测试覆盖缺失、显式 null、默认值、false/零/空字符串、字面点键、数组索引、批量键顺序及独立快照。

生成项目同样通过公开 `openrust` 依赖编译，不需要复制框架私有源码。其应用 Provider 清单可显式加入 `config_repository_reading_consumer::ReadingProvider`；fixture 在所有 boot 覆盖完成后经 `BootContext::config_repository()` 读取固定 `app.name` / `app.locale`，并登记只返回白名单名称和缺失时默认语言的 `GET /api/config-reading`。生成器当前只建立空 `modules/`，不会自动复制本仓唯一的 `modules/example`。

总控生成全新应用后，从本分支根运行 `python3 -B tests/consumers/config-repository-reading/scripts/install_generated_fixture.py <生成应用目录> "$PWD"`。脚本先检查生成应用的 `openrust` 依赖确实指向本分支，再在应用自己的 manifest、bootstrap 和 tests 中显式安装 fixture，输出修改前后的 SHA-256。随后在生成应用执行 `cargo check` 和 `cargo test generated_application_installs_config_reading_provider -- --exact`；实际过滤应出现一个测试，不能用零测试通过作证据。
