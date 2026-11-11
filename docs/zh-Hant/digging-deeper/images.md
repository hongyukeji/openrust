# 影像處理

## 讀取影像 {#reading-images}

在應用程式中從 `openrust::support::facades` 匯入 `Image`。用 `request.image("avatar").await?` 從 multipart 欄位建立影像，或用 `Image::fromUpload` 讀取既有上傳。`Image::fromStorage` 和 `Storage::disk(Some("public"))?.image(path)` 使用設定的磁碟。也可使用位元組、Base64、路徑、HTTP URL 和非同步串流。串流在建構時讀取，其餘來源在轉譯或檢查影像時載入。

## 轉換與編碼 {#manipulating-images}

轉換依新增順序執行。`resize` 接收選用的寬與高。`scale` 保留長寬比且不會放大。`cover` 裁切並填滿目標尺寸，`contain` 保留整張影像並填補空白。也支援裁切、EXIF 方向、旋轉、模糊、灰階、銳化和翻轉。以複製分出處理鏈，透過 `toBytes().await?`、`toBase64().await?` 或 `toDataUri().await?` 取得結果。

```rust
use openrust::image::{ImageManager, Limits};

let manager = ImageManager::new(Limits::default());
let source = manager.fromBase64(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC",
);
let thumbnail = source.cover(4, 4).toWebp();
assert_eq!(thumbnail.dimensions().await?, (4, 4));
assert_eq!(thumbnail.mimeType().await?, "image/webp");
```

## 儲存影像 {#storing-images}

`store` 依編碼格式產生檔名。`storeAs` 使用指定檔名，`storePublicly` 和 `storePubliclyAs` 要求公開可見性。磁碟寫入成功後回傳儲存路徑。

## 檢查影像 {#inspecting-images}

用 `mimeType`、`extension`、`dimensions`、`width` 和 `height` 檢查處理後的結果。`dominantColor` 回傳依透明度加權的 RGB 平均色，格式為十六進位。

## 影像驅動 {#image-drivers}

應用程式預設使用 `image` 驅動。`IMAGE_DRIVER` 可選擇已註冊的驅動。用 `Image::extend` 註冊 `Driver` 實作，用 `using` 為影像選擇驅動。預設限制為每個輸入 32 MiB、每邊 16,384 像素、每張影像 2,400 萬像素。

JPEG、PNG、GIF、WebP 和 BMP 支援讀寫。AVIF 支援寫入。JPEG、WebP 和 AVIF 使用設定的品質值。對動畫輸入的轉換只處理單一影格。
