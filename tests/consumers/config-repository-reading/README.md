# 配置读取公开 API consumer

固定业务名称通过 `string_or` 严格读取，类型不符返回不含原值的 `ConfigError`；生成 fixture 也断言运行期写入后的 `string` 读取。
应用宿主的 `config:cache` 使用同一公开 `ConfigRepository::all()` 最终值。可先执行生成应用二进制的 `config:cache`，修改 `config/app.toml` 后重新启动并访问 `GET /api/config-reading`，确认仍读到缓存值；运行 `config:clear` 并再次启动，确认新文件值出现。该 consumer 不读取缓存文件或复制框架加载器。

生成应用完成 fixture 安装及构建后，保留本次构建日志与二进制 SHA；把生成应用复制到任务临时目录，再用[真实进程验证脚本](scripts/verify_cache_restart.py)执行缓存、改配置、重启 GET、清除、再重启 GET。脚本要求副本标记与真实目录一致，且传入的二进制 SHA 与本次构建记录一致。以生成名 `sample-app` 为例：

```sh
set -e
freeze_receipt=/path/to/frozen-18-file-sha-receipt.json
verify_freeze() {
  python3 -B -c 'import hashlib,json,pathlib,sys; r=json.loads(pathlib.Path(sys.argv[1]).read_text()); changed=[p for p,s in r["files"].items() if hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()!=s]; print("freeze files:",len(r["files"]),"drift:",changed); sys.exit(bool(changed))' "$freeze_receipt"
}
case_root="$(mktemp -d /tmp/openrust-config-cache-generated.XXXXXX)"
verify_freeze
cargo build --locked --offline --manifest-path /path/to/sample-app/Cargo.toml \
  > "$case_root/build-server.log" 2>&1
verify_freeze
binary="${CARGO_TARGET_DIR:-/path/to/sample-app/target}/debug/sample-app"
binary_sha="$(shasum -a 256 "$binary" | cut -d ' ' -f 1)"
mkdir "$case_root/app"
rsync -a --exclude='.git' --exclude='target' --exclude='.env' \
  --exclude='bootstrap/cache' /path/to/sample-app/ "$case_root/app/"
python3 -c 'import pathlib, sys; p = pathlib.Path(sys.argv[1]).resolve(strict=True); (p / ".openrust-config-cache-task-root").write_text(str(p) + "\n", encoding="utf-8")' "$case_root/app"
python3 -B tests/consumers/config-repository-reading/scripts/verify_cache_restart.py \
  "$case_root/app" "$binary" \
  config/app.toml 'name = "sample-app"' 'name = "cache-updated"' \
  /api/config-reading data.name sample-app cache-updated \
  --bad-content 'name = [' --expected-binary-sha256 "$binary_sha"
```

本工程作为独立 Cargo package，从仓库路径依赖公开 `openrust::config::repository::ConfigRepository` 与 `ConfigSources`。`src/lib.rs` 的业务函数只读写预先选定的 `app.name`、`app.locale`，不接受请求方提供的键或值，也不把 `all()` 输出到 HTTP。`POST /api/config-reading/review` 只把 `app.name` 写为固定的“已复核应用”；后续 `GET /api/config-reading` 读取同一仓储。`GET /api/config-reading/order` 只返回 `reader` 区段中固定的 `z_label`、`a_label` 键名，展示来源声明顺序。写入是当前应用内存状态，`App::config()` 保持启动快照。

在本目录运行 `cargo test`；读取测试覆盖缺失、显式 null、默认值、false/零/空字符串、字面点键、数组索引、批量键顺序及独立快照。写入和有序输入测试待集中验证。

生成项目同样通过公开 `openrust` 依赖编译，不需要复制框架私有源码。测试 fixture 显式选择 `cache.default="default"` 并登记 memory store，避免把缓存选择误交给框架缺省的具名 `redis` store。其应用 Provider 清单可显式加入 `config_repository_reading_consumer::ReadingProvider`；fixture 在所有 boot 覆盖完成后经 `BootContext::config_repository()` 取得同一仓储，并登记固定业务读写路由。安装脚本额外建立自有 `config/reader.toml`，在文件根按 `z_label`、`a_label` 顺序声明两个公开业务键；生成应用的真实 server 可通过 `GET /api/config-reading/order` 验证默认配置加载入口。生成器当前只建立空 `modules/`，不会自动复制本仓唯一的 `modules/example`。

外部 crate 的测试还通过公开 `App::register_provider` 和 `force_register_provider` 验证运行期登记：同类型普通重复保留首次服务，强制登记影响后续解析，已持有的 `Arc` 仍指向旧实例。自定义 Provider 的 `register` 返回 `BoxFuture`，在异步 `RegisterContext::instance` 调用处 `await?`。

总控生成全新应用后，从本分支根运行 `python3 -B tests/consumers/config-repository-reading/scripts/install_generated_fixture.py <生成应用目录> "$PWD"`。脚本先检查生成应用的 `openrust` 依赖确实指向本分支，以及测试文件和 `config/reader.toml` 均不存在，再在应用自己的 manifest、bootstrap、tests 和 config 中显式安装 fixture；回执为四个文件分别记录安装前状态及安装后 SHA-256。随后在生成应用执行 `cargo check` 和 `cargo test generated_application_installs_config_reading_provider -- --exact`；实际过滤应出现一个测试，不能用零测试通过作证据。
