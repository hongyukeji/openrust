# 部署

## 建置應用

使用 Rust 1.94 或更新版本及儲存庫鎖定檔。在目標作業系統與架構建置，或使用另行驗收的交叉編譯工具鏈。

```sh
cargo +1.94.0 build --locked --release --features standalone
mkdir -p release/resources/views release/public release/storage/app/public
cp target/release/main release/main
cp resources/views/welcome.blade.html release/resources/views/welcome.blade.html
```

預設翻譯已編入執行檔。應用程式在 `lang/` 中提供自訂翻譯時，請隨應用程式部署這些檔案。

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

### 使用現有 MySQL 和 Redis 完整初始化

新應用程式設定 MySQL 和 Redis 連線後，在原始碼目錄執行一次 `./artisan init:full`。Windows 使用 `artisan.bat init:full`。命令建置應用程式，以互斥方式執行遷移，並驗證 HTTP、Artisan 和 Redis Worker。連線設定與唯讀預覽見[安裝指南](installation.md#complete-initialization)。

單機和多副本使用同一份應用程式產物。同一部署的各個程序共用 MySQL、Redis、Redis 鍵前綴和 `APP_KEY`。需要跨副本存取的檔案使用共用儲存空間。發行多個副本時應協調遷移和排程，由部署層設定負載平衡與 TLS。

## 啟動 HTTP 與 Artisan

準備好執行環境並完成必要的遷移後，直接啟動應用程式。HTTP 和 Artisan 都可以在沒有預先產生的快取時執行。

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

## 選用的部署最佳化

需要預先產生部署快取時，可以使用以下指令。

### 快取設定 {#cache-configuration}

部署環境設定就緒後，可以在啟動服務前產生設定快取：

```sh
./main artisan config:cache
```

在最終發行目錄中，使用服務啟動時的環境執行命令。設定快照可能包含絕對路徑。搬移發行目錄或修改設定後，先在新環境中重新產生快取，再重新啟動服務。需要在啟動時從環境讀取設定時，執行 `./main artisan config:clear`。見[設定快取](configuration.md#configuration-caching)。

### 快取部署資源 {#cache-deployment-resources}

```sh
./main artisan optimize
./main artisan optimize:clear --except=cache
```

`optimize` 快取設定、事件探索結果、路由比對索引和編譯檢視。`optimize:clear` 清除這些產物、套件清單和預設應用程式快取。用 `--except`（`-e`）傳入以逗號區隔的任務名稱或 Artisan 命令名稱。範例會保留預設應用程式快取。服務提供者可透過 `optimizes` 新增任務。

覆寫路由、事件或套件清單快取路徑時，在服務與執行 Artisan 的 Shell 環境中設定對應的 `APP_ROUTES_CACHE`、`APP_EVENTS_CACHE` 或 `APP_PACKAGES_CACHE`。設定快取命中後，啟動可能略過讀取 `.env`。產生、讀取與清除這些檔案時應使用同一組路徑。視圖編譯目錄取自設定快照。

參閱[路由快取](../the-basics/routing.md#route-caching)、[事件探索](../digging-deeper/events.md#event-discovery-in-production)與[套件探索](../digging-deeper/packages.md)。

## 一起執行服務

直接執行 `./main` 會啟動 HTTP、預設非同步佇列的 Worker，以及已註冊工作的排程器。`sync` 連線會略過 Worker。gRPC 等監聽器遵循各自的設定。啟動前先準備佇列後端。使用資料庫佇列時，請先執行上面的遷移指令。

設定 `APP_DISABLED_RUNNERS=queue,schedule` 可停用佇列和排程。以逗號分隔的清單寫入 `app.disabled_runners`，空清單允許所有符合條件的 Runner。內建簽名為 `http`、`queue`、`schedule`、`grpc` 和 `reverb`。可用 `q*` 等萬用字元模式比對簽名。模式支援 `*`、`?`、`[a-z]` 和 `[^a-z]` 等字元類別，以及反斜線跳脫。`*` 和 `?` 不跨越 `/`。應用程式會警告並略過無效模式。所有 Runner 均被篩除或不符合條件時，應用程式會正常結束。

Provider 透過 `ServiceProvider::runners` 提供 Runner，應用程式可用 `with_runners(|| vec![...])` 加入自訂 Runner，多次呼叫時以最後傳入的回呼為準。相同簽名以首次註冊為準，Provider 優先於自訂清單。應用程式先篩除停用的簽名，再呼叫 `should_run`。符合條件的 Runner 在應用程式程序內並行執行。

Ctrl+C、Unix SIGTERM 或 Runner 執行失敗時，應用程式依 `shutdown_priority` 由小到大關閉，同組並行，預設優先級為零。`run` 可透過 `RunnerContext` 取消權杖完成處理中的工作，自訂 `shutdown` 可釋放其他資源。`run` 成功回傳後仍會執行關閉鉤子，以支援自行啟動背景工作的 Runner。

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

## Docker 部署 {#docker-deployment}

Linux 使用具備 Compose 與 Buildx 的 Docker Engine，macOS 與 Windows 使用 Linux 容器模式的 Docker Desktop。同一份 Dockerfile 支援 Linux `amd64` 與 `arm64`。以下命令從儲存庫根目錄執行，統一使用 `docker/docker-compose.yml` 與根目錄 `.env`；檔案尚不存在時，複製 `.env.example` 為 `.env`（PowerShell 使用 `Copy-Item .env.example .env`），已有檔案直接編輯。

直接使用原生 Docker Compose，無須額外啟動腳本，只載入專案根目錄 `.env`。映像名稱為 `${APP_IMAGE_NAME:-openrust}:${APP_IMAGE_VERSION:-latest}`；名稱使用合法小寫映像倉庫名稱，`APP_IMAGE_VERSION` 可指定發布標籤，未設定時使用 `latest`。Compose 不讀取 Cargo；應用程式版本仍使用編譯時的 Cargo 預設值。開發機與部署伺服器各自維護根 `.env`，實際環境檔案不提交 Git。

Docker 專用設定範例見 `docker/.env.example`，依需要將設定項合併至根 `.env`。它僅作為參考模板，Compose 仍只載入根 `.env`。

```sh
docker compose --env-file .env -f docker/docker-compose.yml up -d --wait postgres mysql redis
```

命令從專案根目錄執行，統一傳入 `--env-file .env`。Shell 值遵循 Compose 原生插值優先順序。應用程式載入同一份檔案，容器明確設定保持內部監聽 `0.0.0.0:8000`，主機繫結由 `APP_HOST` 和 `APP_PORT` 控制。

Windows ARM64 後端需要可運作的 WSL 2；Windows 虛擬機需要巢狀虛擬化。參見 [Docker Desktop 系統要求](https://docs.docker.com/desktop/setup/install/windows-install/#system-requirements)。

### 本機資料庫與快取

在根 `.env` 填寫非空的 `DB_DATABASE`、`DB_USERNAME` 與 `DB_PASSWORD`。本機資料庫使用者名稱與密碼統一使用 `root`，即 `DB_USERNAME=root`、`DB_PASSWORD=root`。MySQL 透過專用的 `MYSQL_ROOT_PASSWORD` 初始化 root 帳號。以下命令啟動 PostgreSQL 18、MySQL 8.4 與 Redis 8；也可指定所需服務，例如 `up -d --wait postgres redis`。

```sh
docker compose --env-file .env -f docker/docker-compose.yml up -d --wait postgres mysql redis
docker compose --env-file .env -f docker/docker-compose.yml ps
```

預設專案名稱為 `openrust`，容器使用 `${APP_NAME}-服務名` 命名。應用程式映像使用 `${APP_IMAGE_NAME:-openrust}:${APP_IMAGE_VERSION:-latest}`。PostgreSQL `5432`、MySQL `3306`、Redis `6379` 預設僅映射回送位址；資料分別位於 `docker/data/postgres`、`docker/data/mysql`、`docker/data/redis`，PostgreSQL 18 掛載整個 `/var/lib/postgresql`。`POSTGRES_PORT`、`MYSQL_PORT` 只改變主機映射；容器資料庫連接埠不變。`REDIS_PORT` 同時改變 Redis 內部監聽、主機映射、健康檢查及應用程式連線埠。環境中的資料庫憑據只在空資料目錄首次初始化時生效，後續改密須更新實際帳號。

應用程式在主機執行時，根 `.env` 設定 `DB_CONNECTION=pgsql` 或 `mysql`、`DB_HOST=127.0.0.1`、對應資料庫連接埠（PostgreSQL `5432`、MySQL `3306`），以及 `REDIS_HOST=127.0.0.1`、`REDIS_PORT=6379`。資料庫名稱與憑據仍使用同一份設定。按需設定 `CACHE_STORE=redis`、`QUEUE_CONNECTION=redis`，產生應用程式金鑰、執行 `./artisan migrate` 並啟動應用程式。Windows 使用 `.\artisan.bat`。

### 應用程式容器

預設啟動應用程式與全部配套服務；僅需資料庫與快取時，在命令末尾指定 `postgres mysql redis`。主機開發保留 `DB_HOST=127.0.0.1` 和 `REDIS_HOST=127.0.0.1`；同一份根 `.env` 設定 `DOCKER_DB_HOST=postgres`、`DOCKER_REDIS_HOST=redis` 供應用程式容器使用。使用 MySQL 時設定 `DOCKER_DB_HOST=mysql`、`DOCKER_DB_PORT=3306`；容器連接埠與主機 `DB_PORT` 不同時才需要覆寫 `DOCKER_DB_PORT`，PostgreSQL 內部使用 `5432`。使用外部服務時，省略這些容器覆寫項，直接填寫實際 `DB_HOST`、`DB_PORT`、`REDIS_HOST`，需要時再個別覆寫。正式環境設定 `APP_ENV=production`、`APP_DEBUG=false`、強密碼和公開 `APP_URL`。

先檢查 `APP_KEY`：已有金鑰直接保留；僅首次缺少金鑰時執行下方產生命令，並將結果填入根 `.env`。應用程式使用 UID/GID `10001`；先建立 `docker/data/storage` 並確保可寫，Linux 可用 `sudo chown 10001:10001 docker/data/storage`。公共資源預設唯讀掛載根目錄 `public/`，可在根 `.env` 設定 `APP_PUBLIC_PATH`，相對路徑以 `docker/` 為基準；目錄必須存在。

```sh
docker compose --env-file .env -f docker/docker-compose.yml build app
docker compose --env-file .env -f docker/docker-compose.yml run --rm --no-deps app artisan key:generate --show
```

```sh
docker compose --env-file .env -f docker/docker-compose.yml run --rm --no-deps app artisan migrate --force
docker compose --env-file .env -f docker/docker-compose.yml up -d --wait
```

開啟 `http://localhost:8000/up`，主機連接埠可用 `APP_PORT` 修改，預設繫結回送位址。映像使用 `APP_PACKAGES_CACHE=storage/framework/cache/packages.json`，一次性命令與應用程式共用可寫儲存。使用 `up -d --wait` 可同時啟動應用程式與全部配套服務。

多機部署使用相同 `APP_IMAGE_NAME` / `APP_IMAGE_VERSION`、應用程式金鑰、外部資料庫及共享快取／佇列設定，執行 `up -d --no-build --no-deps --wait app`。每次發布集中執行一次遷移，由負載平衡器終止 TLS，上傳檔案使用 S3 等共享儲存。只需一台執行排程器時，其他執行個體設定 `APP_DISABLED_RUNNERS=schedule`。建置時以 `APP_BUILD_FEATURES` 選擇可選功能。

### Nginx 對外入口 {#nginx}

預設啟動的 Nginx 將 HTTP 請求轉送至 `app:8000`，對外監聽 `0.0.0.0:8080`。在根 `.env` 設定 `NGINX_PORT` 可修改主機連接埠，設定 `NGINX_HOST` 可限制繫結位址；Nginx 容器內部固定監聽 `80`。例如 `NGINX_HOST=127.0.0.1`、`NGINX_PORT=18080` 表示只供本機存取。應用程式的 `APP_URL` 應填寫實際對外位址，例如 `http://localhost:8080`。

代理設定位於 `docker/nginx/default.conf`，保留請求路徑和 Host，傳遞用戶端及協定資訊，支援 WebSocket 升級並關閉回應緩衝以適配串流回應。Nginx 等待應用程式健康後啟動；應用程式重建後會透過 Docker DNS 重新解析位址。外部存取使用 Nginx 連接埠，`APP_HOST`／`APP_PORT` 繼續控制應用程式的直接存取入口。TLS 由實際部署的 HTTPS 入口設定。

建置參數使用 `APP_BUILD_FEATURES` 和 `APP_BUILD_JOBS`（預設 `4`），由 Compose 傳入 Dockerfile。`APP_STORAGE_PATH` 可修改儲存掛載路徑，相對路徑從 `docker/` 計算，目錄須對 UID/GID `10001` 可寫。

Nginx 請求本文上限為 8 MiB，與框架輸入限制一致。代理讀取與傳送逾時使用 Nginx 預設的 60 秒；串流回應與 WebSocket 應及時傳送資料或心跳。確需更長閒置等待時，在 `docker/nginx/default.conf` 中按需設定相應逾時。

### 更新與回復

先備份資料庫和儲存內容，保留已有 `APP_KEY`。原始碼發布依序執行 `build app`、`run --rm --no-deps app artisan migrate --force`、`up -d --no-build --wait`；每條命令均使用上文相同的 Compose 及環境檔案參數。使用發布映像時設定固定的 `APP_IMAGE_NAME` / `APP_IMAGE_VERSION`，以 `pull app` 取代 `build app`。遷移每次發布集中執行一次，不在每個容器啟動時自動執行。

回復時選擇仍保留的上一版映像標籤，再執行 `up -d --no-build --wait`。映像回復不會撤銷資料庫遷移，須先確認資料表結構相容，必要時使用備份還原。`stop` 保留資料；更新時不要刪除 `docker/data/` 或使用 `down -v`。

升級既有專案時，請依[語言標識升級說明](../digging-deeper/localization.md#bcp47-upgrade)同步遷移語言資源、持久化值與快取。
