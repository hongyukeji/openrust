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

GIF、動畫 PNG（APNG）、WebP、AVIF、HEIC 和 JPEG XL 在變換及轉換時可保留顯示畫格、畫格順序及播放次數。GIF 的延遲單位為 10 毫秒。WebP、AVIF、HEIC、JPEG XL 的輸出延遲單位為 1 毫秒。APNG 可儲存分數延遲。JPEG XL 將零延遲取整為 1 毫秒，讓該畫格仍可顯示。

未經修改的動畫在通過畫格檢查後回傳原始位元組。JPEG、BMP 輸出和 `dominantColor()` 使用第一個顯示畫格。HEIF 輸入採用一條主要時序軌道，可包含輔助透明度軌道。HEIC 可以帶有編碼填補，並透過 clean aperture 指定顯示尺寸。

驅動對動畫輸入和輸出均設定 1,024 格和 9,600 萬總像素的上限。自訂像素限額可將總上限降低至單張影像限額的四倍。每個畫格同時受單張影像限額約束。

## 影像中繼資料 {#image-metadata}

內建驅動在 JPEG、PNG、WebP、AVIF、HEIC 和 JPEG XL 的變換及轉換中保留 EXIF、ICC 描述檔與 XMP。當 `orient()` 套用 JPEG、PNG 或 WebP 的 EXIF 方向時，輸出把方向欄位重設為正向，並保留其他 EXIF 位元組。HEIF 動畫輸出將這些欄位寫入封面影像。讀取 AVIF 或 HEIC 序列時，主軌道有 ICC 描述檔則優先使用。封面缺少 EXIF 或 XMP 時，從主軌道讀取對應欄位。JPEG 變換後繼續輸出 JPEG 時，也會保留包含 IPTC 的 Photoshop APP13 描述檔和 JPEG 註解。這些區段保留在 JPEG 容器中。

AVIF 與 HEIC 序列的變換和相互轉換保留每個顯示畫格各自的 ICC、EXIF 和 XMP。ICC 隨影像樣本描述，EXIF 和 XMP 透過關聯的中繼資料軌道按顯示時間對應到原畫格。轉為動畫 PNG、WebP 或 JPEG XL 時使用這些格式的全域中繼資料欄位。

## 儲存影像 {#storing-images}

`store` 依編碼格式產生檔名。`storeAs` 使用指定檔名，`storePublicly` 和 `storePubliclyAs` 要求公開可見性。磁碟寫入成功後回傳儲存路徑。

## 檢查影像 {#inspecting-images}

用 `mimeType`、`extension`、`dimensions`、`width` 和 `height` 檢查處理後的結果。`dominantColor` 回傳依透明度加權的 RGB 平均色，格式為十六進位。

## 影像驅動 {#image-drivers}

應用程式預設使用 `image` 驅動。`IMAGE_DRIVER` 可選擇已註冊的驅動。用 `Image::extend` 註冊 `Driver` 實作，用 `using` 為影像選擇驅動。預設限制為每個輸入 32 MiB、每邊 16,384 像素、每張影像 2,400 萬像素。

內建驅動可讀寫 JPEG、PNG、GIF、WebP、AVIF、HEIC、JPEG XL 與 BMP。可使用 `toAvif()`、`toHeic()`、`toJxl()` 或 `toFormat("heif")` 轉換格式。`heif` 別名輸出副檔名為 `.heic`、MIME 類型為 `image/heic` 的檔案。JPEG、WebP、AVIF、HEIC 與 JPEG XL 套用指定品質。WebP 和 JPEG XL 在品質為 100 時採用無損編碼。編解碼皆在 Rust 中執行。
