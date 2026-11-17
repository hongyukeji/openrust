# gRPC

## 安裝

在應用程式原始碼目錄執行安裝命令：

```sh
./artisan install:grpc
```

安裝程式建立 `routes/grpc.rs`，並透過 `bootstrap/app.rs` 中的 `withGrpc` 接入註冊函式。重複執行會保留現有路由檔案。註冊服務後重新建置應用程式。

## 註冊服務

在 `routes/grpc.rs` 中，將產生的 Tonic 服務加入傳入的 `RoutesBuilder`。註冊函式接收應用程式實例，可以從中解析服務。OpenRust 在 Provider 和 HTTP 路由組裝後執行一次註冊函式。

```rust
use openrust::{foundation::Application, grpc::RoutesBuilder, support::ProviderResult};

pub(crate) fn register(_app: &Application, services: &mut RoutesBuilder) -> ProviderResult {
    let (_, health) = tonic_health::server::health_reporter();
    services.add_service(health);
    Ok(())
}
```

此範例需要在應用程式中加入 `tonic-health` 相依套件。

## 執行監聽器

啟動應用程式前設定 `GRPC_HOST` 和 `GRPC_PORT`。主機為空時停用監聽。HTTP 和 gRPC 連接埠繫結後才回報就緒，繫結失敗會終止啟動，兩者共用優雅關閉流程。Artisan 引導應用程式時無須繫結 gRPC 連接埠。

監聽器接收經 HTTP/2 傳輸的明文 gRPC。面向不受信任的網路時，由 TLS 代理終止加密連線。

## 具名用戶端 {#named-clients}

在 `config/grpc.rs` 回傳的 `clients` 物件中宣告對外連線。每個用戶端名稱對應目前應用程式共享的惰性 Tonic 連線，首次 RPC 時建立連線。

```rust
"clients": {
    "health": {
        "host": "127.0.0.1",
        "port": 50051,
        "connect_timeout_ms": 1000,
        "timeout_ms": 2000,
    },
    "billing": {"endpoint": "http://billing.example.test:50051"},
},
```

在 HTTP 處理器和 Artisan 命令中使用 `Grpc::connect("health")`，也可以從應用程式容器解析 `GrpcClients`。將回傳的服務傳給產生的 Tonic 用戶端，例如 `HealthClient::new(Grpc::connect("health")?)`。缺少用戶端或位址格式錯誤會在解析時回傳錯誤，網路故障由 RPC 回傳。
