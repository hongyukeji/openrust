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

監聽器預設使用明文 HTTP/2。可設定伺服器憑證直接提供 TLS，也可由反向代理終止 TLS。

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

## 憑證與攔截器 {#credentials-and-interceptors}

在應用程式啟動期間註冊 Tonic 憑證和攔截器群組，並在第一次解析用戶端或啟動監聽器前完成。在用戶端設定中填寫群組名稱。

```rust
use openrust::grpc::{Grpc, tonic::transport::ClientTlsConfig};

Grpc::clients()?.register_credentials(
    "internal-tls",
    ClientTlsConfig::new().ca_certificate(internal_ca).domain_name("health.example.test"),
);
Grpc::clients()?.register_interceptor("request-id", |mut request| {
    request.metadata_mut().insert("x-request-id", "grpc-probe".parse().unwrap());
    Ok(request)
});
```

```json
{"host":"health.example.test","port":50051,"credentials":"internal-tls","interceptors":["request-id"]}
```

設定 TLS 憑證群組後，使用主機與連接埠的用戶端會採用 HTTPS。明確指定 HTTPS 端點時，須使用已註冊的憑證群組。憑證群組未知，或端點協定與憑證不符時，會回傳設定錯誤。

用戶端依設定順序執行各攔截器群組，群組內依註冊順序執行。未註冊的攔截器群組會略過。使用 `Grpc::server()?.add_interceptor(...)` 為所有已註冊服務加入伺服器攔截器。攔截器回傳 Tonic 狀態時會停止執行，並將狀態回傳給呼叫端。

透過 `Grpc::server()?.set_credentials(ServerTlsConfig::new().identity(server_identity))` 啟用伺服器 TLS。若要求雙向 TLS，在伺服器加入 `client_ca_root(client_ca)`，並在用戶端憑證加入 `identity(client_identity)`。
