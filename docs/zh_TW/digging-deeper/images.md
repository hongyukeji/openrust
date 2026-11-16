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

## 動畫影像 {#animated-images}

GIF、動畫 PNG（APNG）和動畫 WebP 互相轉換時保留合成畫格、畫格順序及播放次數。管線會為每個顯示畫格套用變換。GIF 的延遲單位為 10 毫秒，WebP 為 1 毫秒，APNG 可儲存分數延遲。轉換時依目標格式的精度取整。

未經修改的 GIF、APNG 和 WebP 在通過所有畫格檢查後回傳原始位元組。JPEG、BMP、AVIF、HEIC 和 JPEG XL 輸出為靜態影像。對於這些輸出和 `dominantColor()`，驅動使用受支援動畫的第一個顯示畫格。驅動對 AVIF、HEIC 和 JPEG XL 序列提供靜態影像支援。

驅動對 GIF、APNG 和 WebP 動畫設定 1,024 格以及輸入、輸出各 9,600 萬總像素的上限。自訂像素限額可將總上限降低至單張影像限額的四倍。每個畫格同時受單張影像限額約束。

## 儲存影像 {#storing-images}

`store` 依編碼格式產生檔名。`storeAs` 使用指定檔名，`storePublicly` 和 `storePubliclyAs` 要求公開可見性。磁碟寫入成功後回傳儲存路徑。

## 檢查影像 {#inspecting-images}

用 `mimeType`、`extension`、`dimensions`、`width` 和 `height` 檢查處理後的結果。`dominantColor` 回傳依透明度加權的 RGB 平均色，格式為十六進位。

## 影像驅動 {#image-drivers}

應用程式預設使用 `image` 驅動。`IMAGE_DRIVER` 可選擇已註冊的驅動。用 `Image::extend` 註冊 `Driver` 實作，用 `using` 為影像選擇驅動。預設限制為每個輸入 32 MiB、每邊 16,384 像素、每張影像 2,400 萬像素。

內建驅動可讀寫 JPEG、PNG、GIF、WebP、AVIF、HEIC、JPEG XL 與 BMP。可使用 `toAvif()`、`toHeic()`、`toJxl()` 或 `toFormat("heif")` 轉換格式。`heif` 別名輸出副檔名為 `.heic`、MIME 類型為 `image/heic` 的檔案。JPEG、WebP、AVIF、HEIC 與 JPEG XL 套用指定品質。WebP 和 JPEG XL 在品質為 100 時採用無損編碼。編解碼皆在 Rust 中執行。
