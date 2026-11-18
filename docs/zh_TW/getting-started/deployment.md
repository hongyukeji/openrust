# 部署

## 建置應用

使用 Rust 1.94 或更新版本及儲存庫鎖定檔。在目標作業系統與架構建置，或使用另行驗收的交叉編譯工具鏈。

```sh
cargo +1.94.0 build --locked --release --features standalone
mkdir -p release/resources/views release/public release/storage/app/public
cp target/release/main release/main
cp resources/views/welcome.blade.html release/resources/views/welcome.blade.html
```

將網站資源複製到 `release/public`，公共儲存檔案複製到 `release/storage/app/public`。歡迎頁在執行時讀取，必須隨二進位檔部署。Rust 設定和路由宣告編譯進 `main`，修改 Rust 原始碼需要重新建置；執行時環境值和視圖檔案不需要。

`standalone` 特性以可執行檔所在目錄作為應用根目錄，不依賴工作目錄。絕對 `APP_BASE_PATH` 可覆蓋該位置。使用程序環境變數，或將 `.env` 放在應用根目錄；程序環境值優先。不要公開正式環境金鑰，也不要未經修改就把 `.env.example` 用作正式環境設定。

## 準備執行環境

啟動前設定正式環境值。設定 `APP_ENV=production`、`APP_DEBUG=false` 和正式環境的 `APP_KEY`。授予服務帳戶 `storage/` 及本機資料庫目錄的寫入權限。將持久化儲存和資料庫放在可替換的發行目錄之外，依需要在部署設定中使用絕對路徑。

將流量切換至新版本前，使用新產物執行遷移：

```sh
./main artisan migrate --force
```

首次部署 SQLite 時先建立設定指定的資料庫檔案。現有應用程式繼續使用其應用程式金鑰和持久化資料。見[遷移](../database/migrations.md)。

使用 MySQL 或 PostgreSQL 時，先在[發佈矩陣](../prologue/releases.md)核對伺服器版本。OpenRust 的 MySQL 連線需要 TLS。應用程式預設啟動資料庫佇列 Worker，使用時須先完成佇列表遷移。

### 快取設定 {#cache-configuration}

部署環境設定就緒後，可以在啟動服務前產生設定快取：

```sh
./main artisan config:cache
```

在最終發行目錄中，使用服務啟動時的環境執行命令。設定快照可能包含絕對路徑。搬移發行目錄或修改設定後，先在新環境中重新產生快取，再重新啟動服務。需要在啟動時從環境讀取設定時，執行 `./main artisan config:clear`。見[設定快取](configuration.md#configuration-caching)。

## 啟動 HTTP 與 Artisan

```sh
cd release
APP_HOST=127.0.0.1 APP_PORT=8088 ./main
```

在另一個終端進入發布目錄：

```sh
./main artisan list
./main artisan help inspire
./main artisan inspire
curl -i http://127.0.0.1:8088/up
```

將 `APP_URL` 設定為應用的公開 URL。未設定時，預設本機 URL 的連接埠取自 `APP_PORT`。公共磁碟使用絕對 URL 時必須與 `APP_URL` 同源。HTTP 監聽器在綁定前驗證連接埠；Artisan 無需合法的 HTTP 連接埠或 HTTP 監聽器。

`SIGINT` 以及 Unix 上的 `SIGTERM` 會請求 HTTP 優雅關閉。使用程序管理器負責重新啟動。已驗證的平台與驅動見[發行說明](../prologue/releases.md)。

原始碼的 `artisan` 腳本執行 Cargo。獨立發布無需 Cargo，使用 `./main artisan …` 執行命令。參閱 [Artisan](../digging-deeper/artisan.md)和[應用設定](configuration.md)。

### 重新載入服務 {#reloading-services}

部署新版本後，可以請求獨立管理的服務優雅重新載入：

```sh
./main artisan reload
./main artisan reload --except=queue,reverb:restart
```

`reload` 呼叫 `queue:restart` 和 `schedule:interrupt`。Reverb 服務提供者為獨立 Reverb 服務接入 `reverb:restart`。服務提供者也可透過 `reloads` 註冊其他任務。用 `--except`（`-e`）傳入以逗號區隔的任務名稱或 Artisan 命令名稱。

使用與 Worker 相同的共用快取設定執行。獨立 Worker 和 Reverb 結束後由程序管理器重新啟動。統一啟動模式下，重新啟動整個 `main` 程序以載入新的執行檔並重新啟動內嵌服務。

### 驗證發布產物 {#verify-the-release}

應用程式開始監聽後，在發布目錄執行以下檢查。HTTP 請求使用部署設定的監聽連接埠。

```sh
curl --fail http://127.0.0.1:8088/up
./main artisan migrate:status
./main artisan schedule:list --json
```

遷移狀態會列出待執行的遷移。排程清單以 JSON 陣列傳回已註冊的任務，沒有註冊任務時傳回空陣列。

## 一起執行服務

直接執行 `./main` 會啟動 HTTP、預設非同步佇列的 Worker，以及已註冊工作的排程器。`sync` 連線會略過 Worker。gRPC 等監聽器遵循各自的設定。啟動前先準備佇列後端。使用資料庫佇列時，請先執行上面的遷移指令。

設定 `APP_DISABLED_RUNNERS=queue,schedule` 可停用佇列和排程。以逗號分隔的清單寫入 `app.disabled_runners`，空清單允許所有符合條件的 Runner。內建簽名為 `http`、`queue`、`schedule`、`grpc` 和 `reverb`。可用 `q*` 等萬用字元模式比對簽名。模式支援 `*`、`?`、`[a-z]` 和 `[^a-z]` 等字元類別，以及反斜線跳脫。`*` 和 `?` 不跨越 `/`。應用程式會警告並略過無效模式。所有 Runner 均被篩除或不符合條件時，應用程式會正常結束。

Provider 透過 `ServiceProvider::runners` 提供 Runner，應用程式可用 `withRunners(|| vec![...])` 加入自訂 Runner，多次呼叫時以最後傳入的回呼為準。相同簽名以首次註冊為準，Provider 優先於自訂清單。應用程式先篩除停用的簽名，再呼叫 `shouldRun`。符合條件的 Runner 在應用程式程序內並行執行。

Ctrl+C、Unix SIGTERM 或 Runner 執行失敗時，應用程式依 `shutdownPriority` 由小到大關閉，同組並行，預設優先級為零。`run` 可透過 `RunnerContext` 取消權杖完成處理中的工作，自訂 `shutdown` 可釋放其他資源。`run` 成功回傳後仍會執行關閉鉤子，以支援自行啟動背景工作的 Runner。

框架新增的常駐服務在完成裝配與設定並符合啟動條件時，預設納入主應用程式啟動。各服務仍可停用或獨立執行。一次性 Artisan 指令由明確呼叫執行。

```sh
./main artisan queue:work
./main artisan schedule:work
./main artisan schedule:run
```
HTTP、受管理的 Worker 和排程器共用應用程式程序。受管理的 Worker 沿用獨立 `queue:work` 的逾時、重試和結束碼。程序 RSS 涵蓋三個服務的記憶體用量，因此受管理的 Worker 不套用獨立記憶體門檻。獨立 `queue:work` 保留現有記憶體上限。啟用強制逾時的工作仍由隔離執行器執行。Ctrl+C 或 Unix SIGTERM 會停止接收新工作，並等待處理中的 HTTP 請求、佇列工作和排程工作完成。應用程式程序會傳遞 Worker 的非零結束碼。

## Windows 獨立部署 {#windows-standalone}

使用目標架構的 Rust 和 MSVC Build Tools 建置發佈產物。全新 SQLite 部署先準備執行檔、資源、公開檔案與環境，再建立資料庫並執行遷移。

```powershell
cargo +1.94.0 build --locked --release --features standalone
New-Item -ItemType Directory -Path release, release/database, release/storage/app/public
Copy-Item target/release/main.exe release/
Copy-Item resources, public release/ -Recurse
Copy-Item .env.example release/.env
New-Item -ItemType File release/database/database.sqlite
$env:APP_ENV = 'production'
$env:APP_DEBUG = 'false'
$app = Join-Path $PWD 'release/main.exe'
& $app artisan key:generate --force
& $app artisan migrate --force
& $app artisan inspire
& $app
```

後續發佈應保留應用程式金鑰、資料庫與可寫入儲存目錄。服務管理員可從其他工作目錄執行 `main.exe`，也可用 `APP_BASE_PATH` 指定絕對應用程式根目錄。

## TLS 終止 {#tls-termination}

將應用程式綁定在私有位址，由反向代理或負載平衡器終止 TLS。將 `APP_URL` 設為公開 HTTPS 位址，並設定代理轉送公開主機名稱和通訊協定。從用戶端驗證憑證鏈和主機名稱，檢查 HTTPS 首頁及健康路由。

隔離發佈探針已用測試 CA 驗證 TLS 1.2 和 1.3。正式端點和實際部署憑證仍待環境驗收。

## Docker 部署 {#docker-deployment}

Linux 使用具備 Compose 與 Buildx 的 Docker Engine，macOS 與 Windows 使用 Linux 容器模式的 Docker Desktop。同一份 Dockerfile 支援建置 Linux `amd64` 與 `arm64` 映像。以下命令從儲存庫根目錄執行，Windows PowerShell 第一步使用 `Copy-Item docker/.env.example docker/.env`。

Windows ARM64 的 Docker Desktop 後端需要可運作的 WSL 2。Windows 虛擬機器需要虛擬化平台提供巢狀虛擬化。請參閱 [Docker Desktop 系統需求](https://docs.docker.com/desktop/setup/install/windows-install/#system-requirements)。

本機開發可只啟動 MySQL 8.4 與 Redis，應用程式在主機執行。根目錄的應用程式 `.env` 設定 `DB_CONNECTION=mysql`、`DB_HOST=127.0.0.1`、`DB_PORT=33060`、`REDIS_HOST=127.0.0.1` 與 `REDIS_PORT=16379`。MySQL 資料庫名稱、使用者與密碼須與 `docker/.env` 一致。使用 Redis 快取與佇列時設定 `CACHE_STORE=redis` 與 `QUEUE_CONNECTION=redis`。應用程式金鑰為空時先產生金鑰，再執行 `./artisan migrate` 和 `cargo run`。Windows PowerShell 使用 `.\artisan.bat`。

單機部署應用程式容器的步驟如下。

1. 複製環境範本。正式環境在初始化 MySQL 前設定 `APP_ENV=production`、`APP_DEBUG=false`、高強度的 `DB_PASSWORD` 與公開的 `APP_URL`。妥善保存 `docker/.env`。
2. 啟動相依服務並建置映像。使用容器內的 Artisan 產生應用程式金鑰，填入 `docker/.env` 的 `APP_KEY`。升級與多執行個體部署仍使用原金鑰。

```sh
cp docker/.env.example docker/.env
docker compose --env-file docker/.env -f docker/docker-compose.yml --profile local up -d --wait mysql redis
docker compose --env-file docker/.env -f docker/docker-compose.yml build app
docker compose --env-file docker/.env -f docker/docker-compose.yml run --rm app artisan key:generate --show
```

3. 應用程式以 UID/GID 10001 執行，需能寫入 `docker/data/storage`。在 Linux 上可對新目錄執行 `sudo chown 10001:10001 docker/data/storage`。Docker Desktop 上請確認目錄已分享給 Docker 且容器可寫入。執行一次遷移，再啟動應用程式。

```sh
docker compose --env-file docker/.env -f docker/docker-compose.yml run --rm app artisan migrate --force
docker compose --env-file docker/.env -f docker/docker-compose.yml up -d --wait app
```

開啟 `http://localhost:8000/up`，連接埠可透過 `APP_PUBLISHED_PORT` 修改。主機連接埠預設繫結回送位址，便於搭配反向代理。MySQL、Redis 與應用程式儲存預設掛載到 `docker/data/` 下的相對目錄。若須唯讀掛載靜態資源，在 Compose 命令加入 `-f docker/docker-compose.public.yml`。預設來源為儲存庫的 `public/`，由主機目錄提供完整的公開檔案。

多台主機部署使用同一個映像，省略 `local` profile。設定 `OPENRUST_IMAGE`、共用的 `DB_HOST` 與 `REDIS_HOST`，各執行個體使用一致的應用程式金鑰、快取／佇列設定及憑證。拉取或載入映像後以 `up -d --no-build --wait app` 啟動。每次發行集中執行一次遷移，由負載平衡器處理 TLS，上傳檔案使用 S3 等共用儲存。若排程器只需在一台執行個體執行，其他執行個體設定 `APP_DISABLED_RUNNERS=schedule`。建置時以 `OPENRUST_FEATURES` 選擇 `reverb`、`sanctum` 等選用功能。
