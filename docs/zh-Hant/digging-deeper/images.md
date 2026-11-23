# 影像處理

## 讀取影像 {#reading-images}

`file()` 在轉譯、複製、繼續變換、切換格式或驅動後仍保留原始上傳檔案。

在應用程式中從 `openrust::support::facades` 匯入 `Image`。用 `request.image("avatar").await?` 從 multipart 欄位建立影像，或用 `Image::fromUpload` 讀取既有上傳。`Image::fromStorage` 和 `Storage::disk(Some("public"))?.image(path)` 使用設定的磁碟。也可使用位元組、Base64、路徑、HTTP URL 和非同步串流。串流在建構時讀取，其餘來源在轉譯或檢查影像時載入。

## 轉換與編碼 {#manipulating-images}

轉換依新增順序執行。`resize` 接收選用的寬與高。`scale` 保留長寬比且不會放大。`cover` 裁切並填滿目標尺寸，`contain` 保留整張影像並填補空白。也支援裁切、EXIF 方向、旋轉、模糊、灰階、銳化和翻轉。以複製分出處理鏈，透過 `toBytes().await?`、`toBase64().await?` 或 `toDataUri().await?` 取得結果。

`flip()` 垂直翻轉，`flop()` 水平翻轉。`resize(width, height)` 和 `scale(width, height)` 回傳 `Result<Image>`，兩個尺寸都缺失時立即報錯；傳入的零尺寸歸一為 1。模糊和銳化強度在交給驅動處理器之前限制為最大 100。

成功載入的來源在分支之間共用。修改已渲染的圖像時以處理後的位元組為起點，不重複此前的變換。`hashName("").await?` 回傳該圖像穩定的檔案名稱，`store` 重用此名稱；`hashName("avatars").await?` 加入路徑前綴。`toString().await?` 回傳資料 URI。路由處理器可直接回傳 `Image` 或 `Result<Image, HttpError>`，回應包含圖像位元組及對應 MIME 類型。

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

`storeAs("avatar.png", None, None).await?` 將第一個參數作為完整檔案名稱；同時指定目錄和名稱時，只去掉合併路徑兩端的斜線。

## 檢查影像 {#inspecting-images}

用 `mimeType`、`extension`、`dimensions`、`width` 和 `height` 檢查處理後的結果。`dominantColor` 回傳依透明度加權的 RGB 平均色，格式為十六進位。

## 影像驅動 {#image-drivers}

應用程式預設使用 `image` 驅動。`IMAGE_DRIVER` 可選擇已註冊的驅動。用 `using` 為影像選擇驅動。預設限制為每個輸入 32 MiB、每邊 16,384 像素、每張影像 2,400 萬像素。

內建驅動可讀寫 JPEG、PNG、GIF、WebP、AVIF、HEIC、JPEG XL 與 BMP。可使用 `toAvif()`、`toHeic()`、`toJxl()` 或 `toFormat("heif")` 轉換格式。`heif` 別名輸出副檔名為 `.heic`、MIME 類型為 `image/heic` 的檔案。JPEG、WebP、AVIF、HEIC 與 JPEG XL 套用指定品質。WebP 和 JPEG XL 在品質為 100 時採用無損編碼。編解碼皆在 Rust 中執行。

用 `Image::extend("custom", creator)` 登記自訂驅動；回呼回傳 `Result<Arc<dyn Driver>>`。建立器在首次呼叫 `driver` 時執行，接收管理器，可透過 `getContainer()` 解析應用程式服務。替換建立器不會替換已快取驅動；`forgetDrivers()` 清除驅動實例，保留建立器與轉換處理器。`setContainer(container)` 改變後續建立器及尚未載入的儲存、URL 來源所用的容器，保留原設定儲存庫與已解析驅動。應用程式須繼續持有容器；獨立管理器的 `getContainer()` 回傳 `None`。

後端需要檢查依賴時，在 `Driver` trait 中實作 `ensureRequirementsAreMet`，預設實作直接成功。管理器在建立之後、登記轉換處理器與快取之前呼叫；失敗會傳回錯誤且不快取該驅動，後續解析可重試。

### 自訂轉換處理器 {#custom-transformation-handlers}

用 `Image::transformUsing(driver, name, callback)?` 或 `ImageManager` 的同名方法為指定驅動註冊處理器。也可先註冊處理器，再透過 `extend` 註冊應用驅動；管理器的 `extend` 回傳 `Result`。驅動契約提供 `transformUsing` 和 `dimensions`；不支援擴充處理器的驅動回傳 `UnsupportedTransformationHandler`。`Image::driver(None)?`、`getDefaultDriver()` 和 `getDrivers()` 提供目前設定驅動與已註冊實例。

```rust
use std::sync::Arc;
use openrust::image::{Error, NativeImage, Transformation};
use openrust::support::facades::Image;

Image::transformUsing(
    "image",
    "app::Invert",
    Arc::new(|native, _transformation| {
        let image = native.downcast_mut::<NativeImage>()
            .ok_or(Error::InvalidTransformation)?;
        image.invert();
        Ok(())
    }),
)?;
let output = Image::fromBytes(bytes)?
    .transform(Transformation::Custom {
        name: "app::Invert".into(),
        options: serde_json::Value::Null,
    })
    .toBytes().await?;
```

內建轉換使用變體原名，如 `Resize` 或 `Grayscale`，同名處理器覆寫內建操作。自訂轉換使用傳入名稱，建議加應用命名空間以避免衝突。回呼接收驅動實際物件的 `&mut dyn Any`，內建驅動使用 `NativeImage`；回傳錯誤即停止流水線。原生動畫處理器逐一處理參與轉換的影格，修改後的影格仍須符合設定限額。

`usingGd()` 和 `usingImagick()` 分別選擇應用已註冊的 `gd` 和 `imagick` 驅動。框架內建原生 `image` 後端；轉換需要的後端未註冊時回傳 `UnknownDriver`，原樣讀取位元組不解析所選後端。
