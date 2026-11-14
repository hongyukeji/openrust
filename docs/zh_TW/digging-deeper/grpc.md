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
