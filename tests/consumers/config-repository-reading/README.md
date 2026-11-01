# 配置读取公开 API consumer

固定业务名称通过 `string_or` 严格读取，类型不符返回不含原值的 `ConfigError`；生成 fixture 也断言运行期写入后的 `string` 读取。
应用宿主的 `config:cache` 使用同一公开 `ConfigRepository::all()` 最终值。默认生成应用由 `ConfigManifest` 登记 `config/app.rs::config(&ConfigContext)`，在同次 dotenv 快照后读取 `APP_NAME`。在自有应用副本写入 `.env` 的初始名称，执行生成应用二进制的 `config:cache`，修改 `.env` 并重启访问 `GET /api/config-reading`：缓存命中仍读初始值，`config:clear` 后重启读新值。脚本还会将 `.env` 临时改为无效格式，核对缓存命中时跳过 dotenv 解析。该 consumer 不读取缓存文件或复制框架加载器；显式 TOML source 不代表默认代码配置。

生成应用完成 fixture 安装及构建后，保留本次构建日志与二进制 SHA；把生成应用复制到任务临时目录，再用[真实进程验证脚本](scripts/verify_cache_restart.py)执行缓存、改 `.env`、重启 GET、清除、再重启 GET。脚本要求副本标记与真实目录一致，且传入的二进制 SHA 与本次构建记录一致；它只修改并恢复副本的 `.env`，不会尝试改变已编译的 `config/app.rs`。以生成名 `sample-app` 为例：

```sh
set -e
freeze_receipt=/path/to/frozen-18-file-sha-receipt.json
verify_freeze() {
  python3 -B -c 'import hashlib,json,pathlib,sys; r=json.loads(pathlib.Path(sys.argv[1]).read_text()); changed=[p for p,s in r["files"].items() if hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()!=s]; print("freeze files:",len(r["files"]),"drift:",changed); sys.exit(bool(changed))' "$freeze_receipt"
}
case_root="$(mktemp -d /tmp/config-cache-generated.XXXXXX)"
verify_freeze
cargo build --locked --manifest-path /path/to/sample-app/Cargo.toml \
  > "$case_root/build-server.log" 2>&1
verify_freeze
binary="${CARGO_TARGET_DIR:-/path/to/sample-app/target}/debug/sample-app"
binary_sha="$(shasum -a 256 "$binary" | cut -d ' ' -f 1)"
mkdir "$case_root/app"
rsync -a --exclude='.git' --exclude='target' --exclude='.env' \
  --exclude='bootstrap/cache' /path/to/sample-app/ "$case_root/app/"
python3 -c 'import pathlib, sys; p = pathlib.Path(sys.argv[1]).resolve(strict=True); (p / ".config-cache-task-root").write_text(str(p) + "\n", encoding="utf-8")' "$case_root/app"
python3 -B tests/consumers/config-repository-reading/scripts/verify_cache_restart.py \
  "$case_root/app" "$binary" \
  /api/config-reading data.name sample-app cache-updated \
  --bad-content 'broken dotenv line' --expected-binary-sha256 "$binary_sha"
```

本工程作为独立 Cargo package，从仓库路径依赖公开 `openrust::config::repository::ConfigRepository` 与 `ConfigSources`。`src/lib.rs` 的业务函数只读写预先选定的 `app.name`、`app.locale`，不接受请求方提供的键或值，也不把 `all()` 输出到 HTTP。`POST /api/config-reading/review` 只把 `app.name` 写为固定的“已复核应用”；后续 `GET /api/config-reading` 读取同一仓储。`GET /api/config-reading/order` 只返回 `reader` 区段中固定的 `z_label`、`a_label` 键名，展示来源声明顺序。写入是当前应用内存状态，`App::config()` 保持启动快照。

独立 consumer 的 `public_console_loader_selects_file_without_changing_plain_application_loader` 通过公开 `ConfigLoader` 验证控制台可显式选择存在的 `.env.local`，而普通应用加载仍使用 `.env`；同一测试再通过公开 `ConfigManifest` 与 `application_code_sources` 证明 Rust 函数可读取所选 dotenv 快照、生成业务配置，并从同一 `ConfigRepository` 读取独立 typed 工厂值。

在本目录运行 `cargo test`；读取测试覆盖缺失、显式 null、默认值、false/零/空字符串、字面点键、数组索引、批量键顺序及独立快照。写入和有序输入测试待集中验证。

生成项目同样通过公开 `openrust` 依赖编译，不需要复制框架私有源码。测试 fixture 在生成副本的 `config/cache.rs` 显式选择 `cache.default="default"` 并登记 memory store，避免把缓存选择误交给框架缺省的具名 `redis` store；在 `bootstrap/providers.rs` 登记 `config_repository_reading_consumer::ReadingProvider`。fixture 在所有 boot 覆盖完成后经 `BootContext::config_repository()` 取得同一仓储，并登记固定业务读写路由。安装脚本新增 `config/reader.rs`，由 `config/mod.rs` 的同一 `ConfigManifest` 登记 `reader` 区段，按 `z_label`、`a_label` 顺序声明两个公开业务键；生成应用的真实 server 可通过 `GET /api/config-reading/order` 验证代码配置入口。生成器当前只建立空 `modules/`，不会自动复制本仓唯一的 `modules/example`。

外部 crate 的测试还通过公开 `App::register_provider` 和 `force_register_provider` 验证运行期登记：同类型普通重复保留首次服务，强制登记影响后续解析，已持有的 `Arc` 仍指向旧实例。自定义 Provider 的 `register` 返回 `BoxFuture`，在异步 `RegisterContext::instance` 调用处 `await?`。

总控生成全新应用后，从本分支根运行 `python3 -B tests/consumers/config-repository-reading/scripts/install_generated_fixture.py <生成应用目录> "$PWD"`。脚本先检查生成应用的 `openrust` 依赖确实指向当前框架目录，以及测试文件和 `config/reader.rs` 均不存在，再在自有生成副本的 manifest、Provider 清单、Rust 配置清单/缓存区段与测试中安装 fixture；回执逐文件记录安装前状态及安装后 SHA-256。随后在生成应用执行 `cargo check` 和 `cargo test generated_application_installs_config_reading_provider`；实际过滤应出现一个测试，不能用裸函数名配 `--exact` 导致零测试通过。

`ordered_source_overrides_keep_original_environment_for_module_config` 验证公开 `merge_ordered_entries` 在覆盖来源后仍向模块代码配置传递最初 dotenv 快照，并保留 typed 工厂；修改副本 `.env` 不触发第二次读取。typed/raw 路径冲突明确拒绝，缓存来源拒绝覆盖由框架同一语义组验证。
