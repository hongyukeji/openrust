# 貢獻指南

## 回報問題

提供原始碼提交、作業系統、Rust 版本與最小重現方式。HTTP 或 Artisan 問題請附實際請求或命令、觀察結果及預期結果。涉及資料庫或驅動時註明使用的環境。

## 提交變更

應用程式接線放在根應用程式，框架原始碼放在 `vendor/openrust/framework/`。遵循固定的 Laravel 基準及專案的 Rust 約定。公開行為有變更時同步英文、簡體中文與繁體中文文件。

從儲存庫根目錄執行檢查：

```sh
cargo +1.94.0 fmt --all --check
cargo +1.94.0 test --workspace --locked
cargo +1.94.0 clippy --workspace --all-targets --locked -- -D warnings
```

涉及 HTTP、Artisan 或後端驅動的行為時，使用對應的實際入口驗收，並在提交說明中記錄環境與結果。文件變更見[文件網站指南](https://github.com/hongyukeji/openrust/tree/master/docs)。
