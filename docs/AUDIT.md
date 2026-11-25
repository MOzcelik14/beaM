# beaM. Teknik Denetim ve Mimari Yol Haritası Raporu (`AUDIT.md`)

**Tarih:** 2026-11-16  
**İncelenen Sürüm:** `1.0.0` (Workspace)  
**Kapsam:** `beam-core`, `beam-gpu`, `beam-media`, `beam-library`, `beam-app`, `shaders/`, `packaging/`, `.github/workflows/`  
**Durum:** Tarihsel v1.0.0 denetimi — aşağıda v1.2.8 (`fix/audit-findings`) itibarıyla 35 bulgunun güncel durum tablosu eklenmiştir.

---

## Güncel Durum Tablosu (v1.2.8 / `fix/audit-findings`)

| ID | Şiddet | Başlık | Durum (v1.2.8) | Not / Kanıt |
| :-: | :--- | :--- | :---: | :--- |
| 1 | Kritik | 3D LUT `.cube` kanal kayması | **Düzeltildi** | `pipeline.rs` içinde `as_chunks::<4>()` kullanılıyor. |
| 2 | Kritik | Tone Curve & HSL HDR kırpılması | **Düzeltildi** | `tone_curve.wgsl` 2.2 shaper + `soft_highlight_rolloff`; `hsl.wgsl` normlu. |
| 3 | Kritik | Tiled export halo, UV tekrarı, vinyet sırası | **Düzeltildi** | `compute_tile_ext_bounds` + `uv_rect` + `test_render_tiled_matches_monolithic_with_spatial_stages`. |
| 4 | Kritik | Asenkron dosya açma & export yarışları | **Düzeltildi** | `decode_generation` koruması ve `export_is_running` kilidi mevcut. |
| 5 | Yüksek | Color Grading ölçek uyumsuzluğu | **Düzeltildi** | `color_grading.wgsl` normalize ağırlıklar, `pipeline.rs` Rec.2020 luma. |
| 6 | Yüksek | 3D LUT 8-bit nicemleme & Rec.2020 kırpılması | **Açık / Kısmi** | `Rgba8Unorm` 3D LUT sürüyor; geniş gamut sRGB eşlemesi korundu. |
| 7 | Yüksek | `Tiff16` export'un 8-bit olması & ICC eksikliği | **Düzeltildi** | `render_to_rgba16_with_colorspace_and_uv_rect` + ICC tag `34675` (`export.rs`). |
| 8 | Yüksek | Undo/Redo sonrası sidecar kopukluğu | **Düzeltildi** | `undo`/`redo`/`jump_to_history_step` sonrası kayıt senkronize. |
| 9 | Yüksek | `update_rating`/`color_label` sidecar ezme | **Düzeltildi** | Parse hatasında ezme önlendi, `RETURNING id` eklendi. |
| 10 | Yüksek | `libsqlite3-sys 0.31.0` (`RUSTSEC-2026-0459`) | **Düzeltildi** | `rusqlite = "0.43"` sürümüne yükseltildi. |
| 11 | Yüksek | Straighten döndürmesinde aspect çarpılması | **Düzeltildi** | `map_output_uv_to_source_with_aspect` ve shader aspect düzeltmesi. |
| 12 | Yüksek | Global kısayolların `TextEdit` odaklıyken tetiklenmesi | **Düzeltildi** | `!ctx.wants_keyboard_input()` koruması eklendi. |
| 13 | Yüksek | UI thread bloklayan `rfd` & senkron I/O | **Düzeltildi** | `rfd` ve thumbnail/plugin işleri arka plana taşındı. |
| 14 | Yüksek | Her slider hareketinde gereksiz GPU çalışması | **Açık** | BindGroup önbellekleme/stage bypass iyileştirmesi bekliyor. |
| 15 | Yüksek | SQLite ölçekleme (transaction, indeks, `busy_timeout`) | **Düzeltildi** | `BEGIN IMMEDIATE`, indeksler ve `PRAGMA busy_timeout` eklendi. |
| 16 | Yüksek | WGSL golden testleri & `build-linux.sh` `\|\| true` | **Düzeltildi** | Uzamsal/lens tiled testleri eklendi; `\|\| true` kaldırıldı. |
| 17 | Orta | RAW `WhiteBalance`/matris & EXIF alanları | **Düzeltildi** | `ProcessingStep::WhiteBalance` + `Calibrate` + RAW EXIF alanları dolu. |
| 18 | Orta | Düzenleme sırasında histogram güncellemesi | **Düzeltildi** | `histogram_data` düzenlenmiş görüntüden hesaplanıyor. |
| 19 | Orta | `PresetGroupMask` yeni ayar desteği | **Açık** | Kısmi; preset maske genişletmesi bekliyor. |
| 20 | Orta | Mask overlay başlangıç tutamacında `drag_stopped` | **Doğrulanmadı** | Etkileşimli GUI testi gerektiriyor. |
| 21 | Orta | Kütüphane görünümünde hata mesajı | **Doğrulanmadı** | Etkileşimli GUI testi gerektiriyor. |
| 22 | Orta | Üretim kodunda `unwrap()`/`expect()` | **Düzeltildi** | `i18n.rs`, `pipeline.rs`, `app.rs` temizlendi; CI kapısı eklendi. |
| 23 | Orta | CPU `rayon` paralelleştirme & kopyalar | **Açık** | Bellek/hız iyileştirmesi Aşama 5+ kapsamında. |
| 24 | Orta | Thumbnail önbellek anahtarı & temizliği | **Düzeltildi** | `VecDeque` deterministic LRU tahliyesi eklendi. |
| 25 | Orta | `BeamApp` monolitik yapısı | **Kısmi** | `dialogs.rs` ayrıldı; `app.rs` bölünmesi Aşama 5'te sürüyor. |
| 26 | Orta | Windows `.iss` & RPM `.spec` sürüm uyumsuzluğu | **Düzeltildi** | `/DMyAppVersion` ve dinamik `.spec` sürümü eklendi. |
| 27 | Orta | Sabit İngilizce metinler (i18n) | **Düzeltildi** | Eksik anahtarlar `en.ftl`/`tr.ftl`'ye eklendi. |
| 28 | Orta | `README.md` / `README.tr.md` güncelliği | **Düzeltildi** | Koddan doğrulanmış kapsamlı README'ler yazıldı. |
| 29 | Düşük | Thumbnail hızlı yolunda `image::Limits` | **Düzeltildi** | `image::Limits` ve `MAX_IMAGE_DIMENSION` uygulandı. |
| 30 | Düşük | Kullanılmayan bağımlılıklar / çoklu sürümler | **Kısmi** | `deny.toml` sadeleştirildi; `multiple-versions` izleniyor. |
| 31 | Düşük | `.cube` `DOMAIN_MIN >= DOMAIN_MAX` kontrolü | **Düzeltildi** | `lut.rs` içinde doğrulama ve test eklendi. |
| 32 | Düşük | `save_user_preset` dosya adı çakışması | **Açık** | Düşük öncelikli. |
| 33 | Düşük | `beam_core::i18n::t` doc-test `ignored` | **Açık** | Bilinçli `ignored`. |
| 34 | Düşük | `beam-gpu` testlerinin sessiz atlanması | **Düzeltildi** | `HeadlessGpu::new()` artık format yeteneklerini de kontrol edip logluyor. |
| 35 | Düşük | Flatpak manifesti eksikliği | **Açık** | Dağıtım yol haritasında. |

---

## Özet (En Kritik 5 Bulgu)

Bu denetimde toplam **35 bulgu** (4 Kritik, 12 Yüksek, 13 Orta, 6 Düşük) ve **2 doğrulanamayan alan** tespit edilmiştir. Derleme, `clippy`, `fmt` ve mevcut 93 birim testi geçmesine rağmen, görüntü işleme hattı ve uygulama durumu içinde doğrudan veri bozulmasına veya yanlış çıktıya yol açan kritik hatalar bulunmaktadır:

1. **3D LUT (`.cube`) Veri Bozulması (Kritik — `crates/beam-gpu/src/pipeline.rs:987-990`)**: `parse_cube_file` her texel için 4 `f32` (RGBA) üretirken GPU yükleme kodu `cube.data.as_chunks::<3>()` ile 3'lü okuma yapmaktadır. Bu durum tüm 3D LUT'ları 1 kanal kaydırarak renkleri bozar ve verinin %25'ini keser.
2. **Varsayılan Tone Curve ve HSL ile HDR (`> 1.0`) Kırpılması (Kritik — `shaders/tone_curve.wgsl:42`, `shaders/hsl.wgsl:114, 154`)**: `ToneCurveAdjustment` ve `HslAdjustment` varsayılan olarak `enabled: true` gelmektedir. Varsayılan ayarlarda bile her kare bu aşamalardan geçer ve lineer sahne değerlerini `[0.0, 1.0]` aralığına kırparak `+EV` pozlama kurtarmasını yok eder.
3. **Tiled Export'ta Geometri Eksikliği ve Tile Sınır/UV Tekrarı Bozulması (Kritik — `crates/beam-gpu/src/pipeline.rs:709-768, 1973-2013`, `shaders/export_transform.wgsl:63-96`)**: `ExportDisplayPipeline` kırpma/döndürme/düzeltme geometrisini uygulamaz (bunu export sırasında CPU üstlenir), ancak GPU tarafındaki `render_tiled_to_rgba8` tile'ları sıfır taşma (halo) ile keser. Bu nedenle `detail.wgsl` tile sınırlarında dikiş izi bırakır; `vignette_grain.wgsl` ve `mask_adjust.wgsl` ise tam görüntü yerine her tile'da `0..1` UV ile tekrarlanır. Ayrıca `vignette_grain` kırpma öncesi çalıştığı için kırpılmış fotoğraflarda vinyet merkezi kayar.
4. **Asenkron Görüntü Yükleme ve Toplu İşlemlerde Yarış Durumları (Kritik — `crates/beam-app/src/app.rs:583-676, 854-879, 5777-5872`)**: Fotoğraf açma işlemlerinde istek/jenerasyon kimliği yoktur; hızlı art arda iki fotoğraf açıldığında eski decode sonucu yeni açılan fotoğrafın `current_path` ve sidecar'ı üzerine yazılabilir. Benzer şekilde `start_export` çalışan export varken ikinci export'u engellemez ve iptal bayrağını (`export_cancel_flag`) ezer.
5. **Kütüphane/Sidecar Veri Kaybı Riski ve Güncel Olmayan SQLite Bağımlılığı (Yüksek — `crates/beam-library/src/lib.rs:292-299, 650-700`, `Cargo.lock`)**: `update_rating`/`update_color_label` mevcut sidecar okunamadığında sessizce yeni `Sidecar` oluşturup mevcut `EditStack` ayarlarını ezme riski taşır ve yazma hatalarını (`let _ =`) yutar. Ayrıca `cargo audit`, `libsqlite3-sys 0.31.0` içinde orta şiddette güvenlik açığı (`RUSTSEC-2026-0459`) raporlamaktadır.

---

## Aşama 1: Proje Yapısı ve Keşif

### 1.1 Workspace ve Crate Mimarisi

```text
beaM/
├── Cargo.toml                  # Workspace tanımı (resolver = "2", edition = "2021", v1.0.0)
├── deny.toml                   # Lisans ve bağımlılık politikası (GPL-3.0-only, LGPL-2.1 istisnası)
├── crates/
│   ├── beam-core/              # Saf veri modelleri, renk matematiği, geometri, LUT, preset, sidecar, i18n
│   ├── beam-gpu/               # wgpu headless/render pipeline, 12 ping-pong WGSL aşaması, tiled export
│   ├── beam-media/             # rawler/image RAW+raster decode, moxcms ICC dönüşümü, CPU thumbnail, export
│   ├── beam-library/           # rusqlite katalog DB, klasör tarama, filtreleme, virtual copy yönetimi
│   └── beam-app/               # eframe/egui masaüstü arayüzü, worker thread'ler, widget'lar, event döngüsü
├── shaders/                    # 14 adet WGSL compute/fragment benzeri tam ekran geçiş shader'ı
├── locales/                    # Fluent yerelleştirme dosyaları (en.ftl, tr.ftl)
├── packaging/                  # Linux (deb, rpm, AppImage, metainfo, desktop) ve Windows (ps1, Inno Setup)
└── fuzz/                       # Workspace dışı fuzzing hedefleri
```

### 1.2 Uçtan Uca Veri Akışı

1. **Dosya Açma (`beam-app/src/app.rs`)**: Kullanıcı dosya seçer veya CLI argümanı ile açar; `thread::spawn` üzerinden arka plan iş parçacığına gönderilir ve `mpsc::Sender<BackgroundMsg>` ile UI'a dönülür.
2. **RAW / Raster Çözme (`beam-media/src/decoder.rs`)**:
   - Uzantıya göre `decode_raster_image` (`jpg/png/tif/webp`) veya `decode_raw_with_rawler` (yapısal konteyner doğrulaması + `catch_unwind` içinde `rawler::decode_file` + `RawDevelop`) çalışır.
   - Çıktı: Tam çözünürlüklü lineer Rec.2020 `RGBA32F` (`data`), 1920 px sınırlı `proxy_data`, ve `ImageMetadata`.
3. **GPU Düzenleme Pipeline'ı (`beam-gpu/src/pipeline.rs` + `shaders/*.wgsl`)**:
   - `GpuImage::from_rgba32f` proxy'yi `Rgba32Float` dokusuna yükler.
   - `EditPipeline::render_intermediate_stages` 12 sabit ping-pong `Rgba32Float` geçişini sırayla çalıştırır: `white_balance` → `exposure` → `highlights_shadows` → `contrast` → `tone_curve` → `hsl` → `vibrance_saturation` → `color_grading` → `detail` → `lut3d` → `vignette_grain` → `mask_adjust`.
4. **Önizleme (`beam-app/src/app.rs`)**:
   - `DisplayPipeline` (`display_transform.wgsl`) geometri (kırpma/döndürme/düzeltme), clipping overlay ve Rec.2020 → sRGB dönüşümünü uygulayıp `egui` dokusuna yazar. 1:1 modunda tam çözünürlük dokusu kullanılır.
5. **Kütüphane & Sidecar (`beam-library/src/lib.rs`, `beam-core/src/sidecar.rs`)**:
   - `LibraryDb` (`rusqlite` + WAL) fotoğraf yollarını, puanları, renk etiketlerini, EXIF özetini ve `virtual_copies` kayıtlarını tutar.
   - Düzenlemeler fotoğrafın yanında `.beam` (RON) sidecar dosyası olarak atomik yazılır.
6. **Export (`beam-media/src/export.rs`)**:
   - Tam çözünürlük `decode_image` → `HeadlessGpu` üzerinde `EditPipeline::render_tiled_to_rgba8(..., 2048)` → CPU tarafında `apply_crop_and_orientation` → `Lanczos3` yeniden boyutlandırma → `apply_unsharp_mask` → `apply_watermark` → `moxcms` ICC profili + temizlenmiş EXIF ile geçici dosyaya yazım ve atomik yeniden adlandırma.

### 1.3 Keşif Komutları ve Çıktı Özeti

| Komut | Sonuç | Özet Çıktı |
| :--- | :--- | :--- |
| `cargo build --release` | Başarılı (`0`) | 0 hata, 0 uyarı (`0.34s`, artımlı önbellekli). |
| `cargo clippy --all-targets --all-features -- -W clippy::all` | Başarılı (`0`) | 0 hata, 0 clippy uyarısı (`2.64s`). |
| `cargo test --workspace` | Başarılı (`0`) | **93 passed, 0 failed, 1 doc-test ignored** (`beam-app`: 4, `beam-core`: 37, `beam-gpu`: 9, `beam-library`: 13, `beam-media`: 30; `beam-core/src/i18n.rs:321` doc-test ignored). |
| `cargo fmt --check` | Başarılı (`0`) | Biçimlendirme hatası yok. |
| `cargo tree -d` | Başarılı (`0`) | Yinelenen bağımlılıklar: `bit-set` (`0.10.0`, `0.11.1`), `bit-vec` (`0.9.1`, `0.10.1`), `foldhash` (`0.1.5`, `0.2.0`), `getrandom` (`0.3.4`, `0.4.3`), `hashbrown` (`0.15.5`, `0.16.1`, `0.17.1`), `moxcms` (`0.8.1`, `0.9.1`), `pollster` (`0.4.0`, `1.0.1`), `rustc-hash` (`1.1.0`, `2.1.3`), `syn` (`2.0.119`, `3.0.9`). |
| `cargo audit` | **1 Uyarı (`error`)** | **RUSTSEC-2026-0459**: `libsqlite3-sys 0.31.0` (CVE-2026-119375, Şiddet: 6.8 medium, Çözüm: `>= 0.39.0` yükseltmesi). |

### 1.4 Kod Hijyeni Envanteri (`TODO`, `unsafe`, `unwrap`/`expect`/`panic!`, Kullanılmayan Bağımlılıklar)

- **`TODO` / `FIXME`:** `crates/` ve `shaders/` içinde **0 adet**.
- **`unsafe`:** `crates/` içinde **0 adet** (`unsafe`-free workspace).
- **Kullanılmayan Bağımlılıklar:** `crates/beam-app/Cargo.toml:19` (`egui-wgpu`) ve `crates/beam-app/Cargo.toml:24` (`pollster`) bağımlılık olarak tanımlı ancak `beam-app/src/` içinde hiç kullanılmıyor.
- **Üretim Kodundaki `unwrap()` / `expect()` Noktaları** (toplam 255 eşleşmenin büyük çoğunluğu `#[cfg(test)]` bloklarındadır; üretim kodundakiler aşağıdadır):
  - `crates/beam-app/src/app.rs:285`: `.expect("Failed to initialize catalog database")`
  - `crates/beam-app/src/app.rs:305, 307, 1516, 1533`: `thumb_queue` `Mutex::lock().unwrap()` ve `Condvar::wait().unwrap()`
  - `crates/beam-app/src/app.rs:420`: `.expect("Valid embedded logo asset")`
  - `crates/beam-app/src/app.rs:2795`: `changed_resp.unwrap()` (koşul korumalı)
  - `crates/beam-gpu/src/pipeline.rs:1631, 1632, 1737, 1739, 1827, 1829, 1838`: `ping_view`, `pong_view`, `export_display_pipeline` üzerinde `.unwrap()`
  - `crates/beam-core/src/i18n.rs:204, 205, 210, 213, 214, 219`: Gömülü `EN_FTL`/`TR_FTL` ayrıştırmasında `.expect(...)`
  - `crates/beam-media` ve `crates/beam-library` üretim kodunda **0 adet** `unwrap`/`expect`/`panic!`.

---

## Aşama 2: Sorun Tespiti (Şiddete Göre Sıralı)

### Kritik Sorunlar

#### 1. 3D LUT (`.cube`) Yüklemesinde Veri Kayması ve Bozulma
- **Alan:** 2. Renk ve görüntü pipeline'ı / 1. Doğruluk
- **Şiddet:** Kritik
- **Konum:** `crates/beam-core/src/lut.rs:18-19, 95-98` ve `crates/beam-gpu/src/pipeline.rs:987-990`
- **Açıklama ve neden sorun olduğu:** `parse_cube_file`, `CubeLut3D::data` vektörüne her texel için 4 `f32` (RGBA) yazar. Ancak `Lut3DPass::update_from_stack` veriyi `cube.data.as_chunks::<3>()` ile 3'erli dilimleyerek okur. Bu uyumsuzluk her texel'de 1 kanal kaymasına yol açar ve LUT verisinin son %25'ini yok sayarak tüm `.cube` LUT renklerini bozar.
- **Önerilen çözüm:** `pipeline.rs:987` içinde `as_chunks::<4>()` kullanın ve `[0], [1], [2]` kanallarını okuyun; ek olarak `CubeLut3D` üzerine bir altın (golden) LUT uygulama testi ekleyin.
- **Tahmini efor:** S

#### 2. Varsayılan Tone Curve ve HSL Aşamalarının HDR (`> 1.0`) Veriyi Kırpması
- **Alan:** 2. Renk ve görüntü pipeline'ı
- **Şiddet:** Kritik
- **Konum:** `crates/beam-core/src/sidecar.rs:185, 296`, `shaders/tone_curve.wgsl:42`, `shaders/hsl.wgsl:114, 154`
- **Açıklama ve neden sorun olduğu:** `ToneCurveAdjustment::default()` ve `HslAdjustment::default()` alanları `enabled: true` ile başlar. `tone_curve.wgsl:42` girdiyi `[0.0, 1.0]` aralığına sıkıştırır (`clamp`); `hsl.wgsl:114` ise tüm bantlar 0 olsa bile `rgb_to_hsl`/`hsl_to_rgb` çalıştırıp `new_l`'yi `[0.0, 1.0]` aralığına kırpar (`hsl.wgsl:154`). Sonuç olarak varsayılan düzenleme durumunda bile `+EV` pozlama veya parlak RAW vurguları bir sonraki aşamaya (Color Grading, Detail, LUT) geçmeden kalıcı olarak kırpılır. Ayrıca `hsl.wgsl:47` içinde `c_max + c_min >= 2.0` olduğunda sıfıra bölme / negatif satürasyon oluşur.
- **Önerilen çözüm:** Kimlik (identity) durumunda olan aşamaları CPU tarafında tamamen atlayın (`disabled`/no-op); `tone_curve.wgsl` için `> 1.0` değerlerini log/toe-shoulder veya lineer uzantı ile koruyun; `hsl.wgsl` içinde `max(2.0 - c_max - c_min, 1e-5)` koruması ve `> 1.0` HDR parlaklık oranını koruyan ölçekleme uygulayın.
- **Tahmini efor:** M

#### 3. Tiled Export'ta Halo Eksikliği, UV Tekrarı ve Vinyet/Geometri Sırası Hatası
- **Alan:** 2. Renk ve görüntü pipeline'ı / 6. Export
- **Şiddet:** Kritik
- **Konum:** `crates/beam-gpu/src/pipeline.rs:709-768, 1668-1677, 1973-2013`, `shaders/vignette_grain.wgsl:1, 55`, `shaders/mask_adjust.wgsl`, `shaders/export_transform.wgsl:63-96`
- **Açıklama ve neden sorun olduğu:** `render_tiled_to_rgba8` büyük görüntüleri 2048 px tile'lara bölerken komşu pikseller için taşma (overlap/halo) bırakmaz. Bu nedenle ±4 px komşuluk okuyan `detail.wgsl` tile kenarlarında dikiş izi üretir. Daha da kötüsü, `vignette_grain.wgsl` ve `mask_adjust.wgsl` tam görüntü koordinatı yerine tile-lokal `in.uv` (`0..1`) kullanır; yani her 2048 px tile kendi içinde ayrı vinyet ve ayrı radyal/gradyan maske çizer. Ayrıca `vignette_grain` Stage 11'de kırpma öncesi çalıştığı için kırpılan fotoğraflarda vinyet merkezi kayar.
- **Önerilen çözüm:** Tile işleme döngüsüne `TILE_HALO = 8` px taşma ekleyin (yalnızca iç bölgeyi çıktıya yazın); `vignette_grain.wgsl` ve `mask_adjust.wgsl` shader'larına `tile_uv_offset` ve `tile_uv_scale` uniform'ları ile tam görüntü normalize koordinatı (ve mümkünse kırpma dikdörtgeni) geçin.
- **Tahmini efor:** L

#### 4. Asenkron Dosya Açma ve Toplu Export Yarış Durumları
- **Alan:** 1. Doğruluk ve hatalar
- **Şiddet:** Kritik
- **Konum:** `crates/beam-app/src/app.rs:583-676, 854-879, 1064, 5777-5872`
- **Açıklama ve neden sorun olduğu:** Arka plan görüntü çözme iş parçacıkları istek ID'si taşımaz. Kullanıcı hızlıca A ardından B fotoğrafını açarsa `B` aktif yol olarak atanır, ancak daha yavaş çözülen `A` bittiğinde `B`'nin piksel tamponu ve sidecar durumu üzerine yazılabilir. Ayrıca `start_export` çalışan bir export varken yeni export başlatılmasını engellemez ve `export_cancel_flag` üzerine yazar; `import_folder_path` da eşzamanlı içe aktarmalarda ilk biten iş `is_importing = false` yapar.
- **Önerilen çözüm:** `BeamApp` içine artan bir `decode_generation: u64` ekleyin ve `BackgroundMsg` dönüşlerinde eşleşmeyen eski yanıtları atın. `export_is_running` ve `active_imports > 0` kontrolleri ile çakışan başlatmaları engelleyin.
- **Tahmini efor:** S

---

### Yüksek Şiddetli Sorunlar

#### 5. Color Grading Parametre Ölçek Uyumsuzluğu (0..100 vs 0..1)
- **Alan:** 1. Doğruluk ve hatalar / 2. Renk ve görüntü pipeline'ı
- **Şiddet:** Yüksek
- **Konum:** `crates/beam-core/src/sidecar.rs:198-241, 633-644`, `crates/beam-gpu/src/pipeline.rs:1510, 1515`, `shaders/color_grading.wgsl:20, 64`
- **Açıklama ve neden sorun olduğu:** `ColorWheelGrade.saturation` (`0..=100`) ve `luminance` (`-100..=100`) olarak tanımlı/temizlenirken, `pipeline.rs:1510` satürasyonu `/ 100.0` yapmak yerine `clamp(0.0, 1.0)` yapar (yani `>= 1.0` her değer %100 doygunluk olur). `luminance` ise ölçeklenmeden doğrudan `-1..1` bekleyen shader'a gider. Benzer şekilde `balance`/`blending`/`DetailAdjustment` doc yorumları (`-100..100`) ile `validate_and_sanitize` (`-1..1` / `0..1`) çelişir.
- **Önerilen çözüm:** Tüm parametre sözleşmelerini tek bir ölçekte (UI `0..100` veya normalize `0..1`) standartlaştırın ve `pipeline.rs` içinde açık birim dönüşümü + birim test ekleyin.
- **Tahmini efor:** S

#### 6. 3D LUT'un `Rgba8Unorm` Nicemlenmesi ve Rec.2020 → sRGB Gamut Kırpılması
- **Alan:** 2. Renk ve görüntü pipeline'ı
- **Şiddet:** Yüksek
- **Konum:** `crates/beam-core/src/lut.rs:95-97`, `crates/beam-gpu/src/pipeline.rs:810, 994-996`, `shaders/lut3d.wgsl:32, 71-80`
- **Açıklama ve neden sorun olduğu:** `.cube` değerleri 8-bit `Rgba8Unorm` 3D dokuya sıkıştırılır (gölge/ton geçişlerinde bantlaşma riski) ve `lut3d.wgsl` LUT öncesi Rec.2020'yi sRGB'ye çevirip `[0, 1]`'e kırptığı için geniş gamutlu renkler kaybolur.
- **Önerilen çözüm:** 3D LUT dokusunu `Rgba16Float` formatına taşıyın (`wgpu::TextureFormat::Rgba16Float` ve `half::f16`) ve LUT öncesi yumuşak gamut eşleme veya seçilebilir çalışma alanı seçeneği sunun.
- **Tahmini efor:** M

#### 7. `Tiff16` Export'un Gerçekte 8-Bit Olması ve ICC Profili Eksikliği
- **Alan:** 6. Export / 2. Renk ve görüntü pipeline'ı
- **Şiddet:** Yüksek
- **Konum:** `crates/beam-gpu/src/pipeline.rs:1808`, `crates/beam-media/src/export.rs:994-1014`
- **Açıklama ve neden sorun olduğu:** `render_tiled_to_rgba8` çıktıyı 8-bit `Rgba8Unorm` olarak geri okur; `Tiff16` export yolu bu 8-bit kanalları `257` ile çarpıp 16-bit TIFF'e yazar (`export.rs:998-1004`). Yani dosya 16-bit görünse de gerçekte 8-bit dinamik aralıktadır. Ayrıca JPEG, PNG ve WebP'ye ICC profili gömülürken `Tiff16` bloğunda ICC profili gömülmez.
- **Önerilen çözüm:** 16-bit export için GPU'dan `Rgba16Unorm` (veya `Rgba16Float` → `u16`) readback yolu ekleyin ve `tiff` encoder üzerinden ICC tag (`34675`) yazın.
- **Tahmini efor:** M

#### 8. Undo/Redo Sonrası Sidecar Senkronizasyon Kopukluğu
- **Alan:** 4. Mimari ve kod kalitesi / 5. Kütüphane ve veri
- **Şiddet:** Yüksek
- **Konum:** `crates/beam-app/src/app.rs:1385-1434, 7009-7017`
- **Açıklama ve neden sorun olduğu:** `push_history_step` `save_sidecar()` çağırırken `undo`, `redo` ve `jump_to_history_step` yalnızca bellek içi `edit_stack`'i günceller ve diske yazmaz. Kullanıcı geri alma yapıp uygulamayı kapatırsa veya başka fotoğrafa geçerse diskteki `.beam` dosyası eski durumda kalır.
- **Önerilen çözüm:** `undo`, `redo` ve `jump_to_history_step` sonunda `save_sidecar()` (veya kirli bayraklı otomatik kayıt) çağırın.
- **Tahmini efor:** S

#### 9. `update_rating` / `update_color_label` Sidecar Ezme ve Hata Yutma Riski
- **Alan:** 5. Kütüphane ve veri
- **Şiddet:** Yüksek
- **Konum:** `crates/beam-library/src/lib.rs:292-299, 367-390, 650-700`
- **Açıklama ve neden sorun olduğu:** Sidecar okuma geçici veya parse hatası verirse `.unwrap_or_else(|_| Sidecar::new(...))` mevcut düzenlemeleri (`EditStack`) sıfırlayan yeni bir sidecar ile diskin üzerine yazabilir; yazma hatası da `let _ =` ile gizlenir. Ayrıca `import_file` içinde `ON CONFLICT DO UPDATE` sonrası `last_insert_rowid()` güncellenen satırın gerçek `id`'sini döndürmeyebilir ve sidecar silindiyse DB'deki puan/etiketi sıfırlar.
- **Önerilen çözüm:** `Sidecar::load_from_file` yalnızca `NotFound` durumunda yeni sidecar oluşturmalı; parse hatasında mevcut dosyayı ezmeden hata döndürmelidir. `import_file` içinde `RETURNING id` kullanın.
- **Tahmini efor:** S

#### 10. Bilinen Güvenlik Açığı: `libsqlite3-sys 0.31.0` (`RUSTSEC-2026-0459`)
- **Alan:** 9. Güvenlik ve dayanıklılık / 10. Dağıtım
- **Şiddet:** Yüksek
- **Konum:** `Cargo.toml:30` (`rusqlite = "0.33"`), `Cargo.lock`
- **Açıklama ve neden sorun olduğu:** `cargo audit`, gömülü C SQLite içeren `libsqlite3-sys 0.31.0` için orta şiddette (`CVSS 6.8`) `CVE-2026-119375` güvenlik açığı bildirmektedir (`>= 0.39.0` öneriliyor).
- **Önerilen çözüm:** `rusqlite` sürümünü `libsqlite3-sys >= 0.39.0` kullanan güncel sürüme yükseltip `cargo update -p rusqlite` ve `cargo audit` çalıştırın.
- **Tahmini efor:** S

#### 11. Düzeltme (Straighten) Döndürmesinde En-Boy Oranı Çarpılması
- **Alan:** 1. Doğruluk ve hatalar / 2. Renk ve görüntü pipeline'ı
- **Şiddet:** Yüksek
- **Konum:** `crates/beam-core/src/geometry.rs:50-67`, `shaders/display_transform.wgsl:85-101`
- **Açıklama ve neden sorun olduğu:** `straighten_deg` döndürmesi piksel uzayı yerine `[0, 1]` normalize UV uzayında en-boy oranı (`aspect_ratio`) düzeltmesi olmadan uygulanır. Kare olmayan (ör. 3:2) fotoğraflarda düzeltme açısı verildiğinde görüntü yamulur (skew).
- **Önerilen çözüm:** Döndürme öncesi UV'yi `x * aspect` ile izotropik uzaya alıp döndürme sonrası `x / aspect` ile geri çevirin (hem `geometry.rs` hem `display_transform.wgsl` içinde).
- **Tahmini efor:** S

#### 12. Global Kısayolların `TextEdit` Odaklıyken Tetiklenmesi ve Ayar Bağlama Hatası
- **Alan:** 8. UX ve erişilebilirlik / 1. Doğruluk
- **Şiddet:** Yüksek
- **Konum:** `crates/beam-app/src/app.rs:1835-2046, 6431-6470`, `crates/beam-app/src/views/library.rs:269-284, 894`
- **Açıklama ve neden sorun olduğu:** `app.rs` içindeki tek tuşluk kısayollar (`G`, `D`, `C`, `J`, `Z`, `F`, `0-9`) `!ctx.wants_keyboard_input()` kontrolü yapmaz. Arama kutusuna veya filigran metnine yazı yazarken görünüm değişebilir veya puan verilebilir. Ayrıca `library.rs:269-284` rakam tuşlarını bağımsız işleyerek çakışmaya yol açar; `app.rs:6453-6470` içindeki iki ayrı kısayol onay kutusu da aynı `number_keys_rate` alanına bağlıdır ve `ShortcutProfile` (`Default` vs `Lightroom`) hiçbir yerde okunmaz.
- **Önerilen çözüm:** Tek tuşluk kısayolları `if !ctx.wants_keyboard_input()` içine alın, `library.rs`'deki yinelenen `Num0..=Num5` işleyicisini kaldırın ve ayar penceresindeki bağlamayı düzeltin.
- **Tahmini efor:** S

#### 13. UI Thread'ini Bloklayan `rfd` Diyalogları ve Kare Başına Senkron I/O
- **Alan:** 3. Performans / 8. UX ve erişilebilirlik
- **Şiddet:** Yüksek
- **Konum:** `crates/beam-app/src/app.rs:955-998, 1248, 1448-1450, 2118-2212, 5914-5975`, `crates/beam-app/src/views/library.rs:181-199, 1118, 1282-1288`
- **Açıklama ve neden sorun olduğu:** Proje aç/kaydet, klasör seçimi ve export hedef diyaloglarının bir kısmı `rfd::FileDialog`u doğrudan UI thread'inde bloklayarak çağırır. Kütüphane ekranında her karede `state.photos.clone()` yapılır, `refresh()` her dosya için senkron `Path::exists()` çalıştırır ve kare başına 8 adede kadar thumbnail diskten senkron decode edilir.
- **Önerilen çözüm:** Tüm `rfd` diyaloglarını `thread::spawn` + `BackgroundMsg` üzerinden asenkronlaştırın; disk thumbnail yüklemesini arka plan `thumb_queue` worker'ına taşıyın ve `state.photos` klonlamasını indeks/slice tabanlı çizimle kaldırın.
- **Tahmini efor:** M

#### 14. Her Slider Hareketinde Gereksiz GPU BindGroup, Tüm 12 Aşama ve 1:1 Doku Güncellemesi
- **Alan:** 3. Performans
- **Şiddet:** Yüksek
- **Konum:** `crates/beam-gpu/src/pipeline.rs:368, 551, 725, 1055, 1376-1682`, `crates/beam-app/src/app.rs:1323-1382`
- **Açıklama ve neden sorun olduğu:** Her render çağrısında 13+ kez `create_bind_group` çağrılır, 4 adet 256'lık Tone Curve LUT'u CPU'da `Vec` tahsisleriyle yeniden hesaplanır, 12 uniform tamponu yazılır ve kapalı/etkisiz olanlar dahil tüm 12 WGSL geçişi çalıştırılır. `1:1` yakınlaştırmada tam çözünürlüklü doku sürükleme bitmeden her karede güncellenir.
- **Önerilen çözüm:** `BindGroup`'ları texture view nesneleriyle birlikte önbelleğe alın; `EditStack` alt gruplarında dirty-check ile yalnızca değişen uniform/LUT'ları güncelleyin ve etkisiz geçişleri `ping/pong` swap etmeden atlayın. `1:1` modunda sürükleme sırasında proxy veya düşük gecikmeli önizleme sunup bırakıldığında tam çözünürlüğü işleyin.
- **Tahmini efor:** M

#### 15. Büyük Kütüphanelerde SQLite Ölçekleme Sorunları (Transaction Eksikliği, Tam Tarama, Sayfalama Yok)
- **Alan:** 5. Kütüphane ve veri / 3. Performans
- **Şiddet:** Yüksek
- **Konum:** `crates/beam-library/src/lib.rs:187-188, 412-603, 755-795`
- **Açıklama ve neden sorun olduğu:** `scan_and_import_folder` her dosya için ayrı auto-commit `INSERT` çalıştırır (10.000 dosyada ciddi disk I/O darboğazı). `query_photos` `LIKE '%...%'` ve indekssiz `ORDER BY created_at DESC` ile tüm eşleşmeleri `LIMIT`/`OFFSET` olmadan belleğe çeker. Ayrıca `LibraryDb` üzerinde `busy_timeout` PRAGMA yoktur.
- **Önerilen çözüm:** Toplu içe aktarma/silme işlemlerini tek `BEGIN IMMEDIATE ... COMMIT` transaction'ına alın; `created_at` ve `file_ext` sütunları için indeks ekleyin, `query_photos`'a `LIMIT/OFFSET` sayfalama ve `PRAGMA busy_timeout = 5000` ekleyin.
- **Tahmini efor:** M

#### 16. WGSL Shader Matematik/Sıra Altın Testlerinin ve CI Linux Paket Doğrulamasının Eksikliği
- **Alan:** 7. Test ve güvenilirlik / 10. Dağıtım
- **Şiddet:** Yüksek
- **Konum:** `crates/beam-gpu/src/pipeline.rs:2069-2539`, `packaging/linux/build-linux.sh:42, 46`, `.github/workflows/release.yml:77`
- **Açıklama ve neden sorun olduğu:** 14 WGSL shader'dan yalnızca `exposure` için gerçek piksel golden testi vardır; `lut3d`, `color_grading`, `detail`, `vignette_grain`, `mask_adjust`, `display_transform` (kırpma/düzeltme) ve `export_transform` (P3/AdobeRGB) için doğrulama testi yoktur (mevcut `test_render_tiled_matches_monolithic` yalnızca noktasal aşamaları test eder). Ayrıca `build-linux.sh:42, 46` içindeki `|| true` RPM ve AppImage hatalarını CI'da gizleyebilir; `release.yml:77` `appimagetool`u sabit sürüm ve SHA256 doğrulaması olmadan indirir.
- **Önerilen çözüm:** Tüm WGSL aşamaları ve uzamsal tiled export için piksel doğruluk testleri ekleyin; `build-linux.sh` içinden `|| true` ifadelerini kaldırın ve `appimagetool` sürümünü SHA256 kontrolüyle sabitleyin.
- **Tahmini efor:** M

---

### Orta Şiddetli Sorunlar

#### 17. RAW Geliştirmede `WhiteBalance`/Renk Matrisi Adımlarının Eksikliği ve `None` EXIF Alanları
- **Alan:** 2. Renk ve görüntü pipeline'ı / 5. Kütüphane
- **Şiddet:** Orta
- **Konum:** `crates/beam-media/src/decoder.rs:914-925, 1005-1008`
- **Açıklama ve neden sorun olduğu:** `RawDevelop` adımları `[Rescale, Demosaic, FujiRotate, CropActiveArea]` olarak yapılandırılmıştır; kamera renk matrisi veya kamera "As Shot" beyaz dengesi çarpanlarının çalışma uzayı Rec.2020'ye nasıl eşlendiği burada açık değildir. Ayrıca `decode_raw_with_rawler` içinde `iso`, `shutter_speed`, `aperture`, `focal_length` sabit olarak `None` döndürülür.
- **Önerilen çözüm:** `rawler` `RawImage` içindeki `wb_coeffs` ve kamera renk matrisinden Rec.2020'ye dönüşüm matrisini doğrulayın/uygulayın ve `rawler` metadata veya `kamadak-exif` üzerinden ISO/enstantane/diyafram/odak uzaklığını doldurun.
- **Tahmini efor:** M

#### 18. Düzenleme Sırasında Histogramın Güncellenmemesi
- **Alan:** 1. Doğruluk ve hatalar / 8. UX
- **Şiddet:** Orta
- **Konum:** `crates/beam-app/src/app.rs:1232, 1338-1382`
- **Açıklama ve neden sorun olduğu:** `histogram_data` yalnızca ilk proxy yüklemesinde hesaplanır; `re_render_edits` sırasında güncellenmez. Kullanıcı pozlama veya kontrastı değiştirdiğinde histogram eski fotoğrafı gösterir.
- **Önerilen çözüm:** Hafif bir GPU compute histogram geçişi veya küçük (ör. 256 px) küçültülmüş çıktı üzerinden canlı histogram güncellemesi ekleyin (slider sürüklerken throttled).
- **Tahmini efor:** M

#### 19. `Preset` / `PresetGroupMask` Yapısının Yeni Ayarları Desteklememesi
- **Alan:** 4. Mimari ve kod kalitesi
- **Şiddet:** Orta
- **Konum:** `crates/beam-core/src/preset.rs:19-31, 97-124`
- **Açıklama ve neden sorun olduğu:** `Preset` ve `PresetGroupMask` içine `color_grading`, `detail`, `lut` ve `masks` dahil edilmemiştir; bu nedenle kullanıcı preset'leri bu ayarları kaydedip uygulayamaz.
- **Önerilen çözüm:** `Preset` ve `PresetGroupMask` alanlarını geriye dönük `#[serde(default)]` ile genişletin.
- **Tahmini efor:** S

#### 20. Mask Overlay Başlangıç Tutamacında `drag_stopped` Kaybı
- **Alan:** 1. Doğruluk ve hatalar
- **Şiddet:** Orta
- **Konum:** `crates/beam-app/src/widgets/mask_overlay.rs:73-79`, `crates/beam-app/src/app.rs:5382-5384`
- **Açıklama ve neden sorun olduğu:** `mask_lin_start` tutamacı `stopped |= s` atamasını `if let (Some(d), s)` bloğunun içinde yapar. Sürükleme sonu karesinde `drag_delta == None` olursa `stopped` kaçırılır ve geçmiş adımı (`push_history_step`) kaydedilmez.
- **Önerilen çözüm:** `let (delta, stopped_start) = ...; stopped |= stopped_start;` şeklinde ayırın.
- **Tahmini efor:** S

#### 21. Kütüphane Görünümünde Hata Mesajlarının Gösterilmemesi
- **Alan:** 8. UX ve erişilebilirlik
- **Şiddet:** Orta
- **Konum:** `crates/beam-app/src/app.rs:5006-5019`
- **Açıklama ve neden sorun olduğu:** `self.error_message` banner'ı yalnızca Develop `CentralPanel` içinde çizilir; Kütüphane ekranında oluşan içe aktarma veya proje yükleme hataları kullanıcı Geliştir moduna geçene kadar görünmez.
- **Önerilen çözüm:** Hata/bildirim banner'ını görünüm bağımsız üst/alt toast veya `TopBottomPanel` katmanına taşıyın.
- **Tahmini efor:** S

#### 22. Üretim Kodunda `Mutex::lock().unwrap()` ve `pipeline.rs` `.unwrap()` Zincirleri
- **Alan:** 9. Güvenlik ve dayanıklılık
- **Şiddet:** Orta
- **Konum:** `crates/beam-app/src/app.rs:305, 307, 1516, 1533`, `crates/beam-gpu/src/pipeline.rs:1631, 1632, 1737, 1739, 1827, 1829, 1838`
- **Açıklama ve neden sorun olduğu:** Thumbnail worker thread'i mutex'i zehirlerse UI thread'i `lock().unwrap()` noktasında panikler. `pipeline.rs` içinde `ok_or_else(...)` yerine `.unwrap()` kullanılması olası durum uyumsuzluğunda kütüphane kullanıcılarını düşürür.
- **Önerilen çözüm:** `unwrap_or_else(|e| e.into_inner())` veya hata dönüşü kullanın; `pipeline.rs` içinde `Result` döndürün.
- **Tahmini efor:** S

#### 23. CPU Tarafında Paralelleştirme (`rayon`) Eksikliği ve Çoklu Tam Çözünürlük Kopyaları
- **Alan:** 3. Performans
- **Şiddet:** Orta
- **Konum:** `crates/beam-media/src/decoder.rs:1158-1272, 1279-1300, 1579-1607`, `crates/beam-media/src/export.rs:109-198, 1101-1182`
- **Açıklama ve neden sorun olduğu:** `beam-media` içinde `rayon` kullanılmaz. `decode_raster_image` aynı dosyayı 3 kez açar; ICC ve yönlendirme dönüşümleri yeni tam çözünürlüklü `Vec<f32>` tamponları tahsis eder; `generate_thumbnail_with_edits` zaten yönlendirilmiş görüntüye tekrar `apply_orientation` çağırır (varsayılan `orientation == 1` olsa da mantıksal fazlalık). Büyük JPEG/TIFF veya toplu export tek çekirdekte kalır.
- **Önerilen çözüm:** `decode_raster_image`'da dosyayı tek açılışta bellek/reader üzerinden okuyun; proxy/unsharp/ICC döngülerini `rayon` ile paralelleştirin ve `DecodedImage` üzerinde isteğe bağlı "sadece proxy decode" modu ekleyin.
- **Tahmini efor:** M

#### 24. `DefaultHasher` ile Thumbnail Önbellek Anahtarı ve Disk Temizliği Eksikliği
- **Alan:** 5. Kütüphane ve veri
- **Şiddet:** Orta
- **Konum:** `crates/beam-media/src/decoder.rs:1625-1663`, `crates/beam-library/src/lib.rs:714-719`
- **Açıklama ve neden sorun olduğu:** `DefaultHasher` Rust sürümleri arasında kararlı değildir; derleyici güncellemesinde tüm thumbnail önbelleği geçersizleşebilir. `clear_all_thumbnails` ise yalnızca SQLite sütununu `NULL` yapar, diskteki önbellek dosyalarını silmez.
- **Önerilen çözüm:** Sabit bir hash (örn. `FNV-1a` 64-bit veya `SHA-256` ilk 16 byte) kullanın ve `clear_all_thumbnails` ile disk temizliği / LRU boyutu sınırı ekleyin.
- **Tahmini efor:** S

#### 25. `BeamApp` Monolitik Yapısı ve Hardcoded Modüler Olmayan WGSL Stage Listesi
- **Alan:** 4. Mimari ve kod kalitesi
- **Şiddet:** Orta
- **Konum:** `crates/beam-app/src/app.rs:83-211` (7.000+ satır), `crates/beam-gpu/src/pipeline.rs:1638-1678`
- **Açıklama ve neden sorun olduğu:** `app.rs` tüm uygulama durumunu, UI çizimini, diyalogları ve arka plan mesajlarını tek dosyada toplar; yeni araç eklemek 4+ yeri (`sidecar.rs`, `pipeline.rs`, `shaders/`, `app.rs`, `preset.rs`) manuel güncellemeyi gerektirir ve Rust/WGSL uniform struct'ları elle senkron tutulur.
- **Önerilen çözüm:** `app.rs` içindeki Develop panelini, Export penceresini, Settings penceresini ve arka plan iş yöneticisini ayrı modüllere bölün; `encase` veya ortak uniform trait'leri ile shader kayıt yapısını sadeleştirin.
- **Tahmini efor:** L

#### 26. Windows Yükleyici (`.iss`) ve RPM `.spec` Sürüm/Uzantı Uyumsuzlukları
- **Alan:** 10. Dağıtım
- **Şiddet:** Orta
- **Konum:** `packaging/windows/beam-installer.iss:3, 56-64`, `packaging/linux/beam.spec:2, 9`
- **Açıklama ve neden sorun olduğu:** `beam-installer.iss:3` sürümü `"1.0.0"` olarak sabit kodlar (`build-windows.ps1`'den `-Version` almaz) ve `.raf`, `.rw2`, `.orf`, `.pef` uzantılarını ilişkilendirmez. `beam.spec:2` de sürümü sabitler ve `rust >= 1.75` ister (`README.md:144` 1.89+ belirtir).
- **Önerilen çözüm:** Inno Setup'a `/DMyAppVersion=$Version` geçin, eksik RAW uzantılarını ekleyin ve `beam.spec` için dinamik sürüm/Rust gereksinimini güncelleyin.
- **Tahmini efor:** S

#### 27. Fluent Çevirisi Yerine Sabit İngilizce Metinler (i18n Eksikleri)
- **Alan:** 8. UX ve erişilebilirlik
- **Şiddet:** Orta
- **Konum:** `crates/beam-app/src/app.rs:359, 468, 522, 628, 649, 668, 672, 693, 713, 717, 875, 980, 1130, 1154, 2055, 6856, 6881, 6921`, `crates/beam-app/src/widgets/histogram.rs:222-226`
- **Açıklama ve neden sorun olduğu:** Uygulama kapsamlı Fluent (`en.ftl`, `tr.ftl`) altyapısına sahip olmasına rağmen hata mesajları, `"Hide Filmstrip"`, `"No image"`, `"Initial"` ve `"Rating: ..."` gibi dizeler doğrudan İngilizce bırakılmıştır.
- **Önerilen çözüm:** Tüm kalan sabit dizeleri `locales/en.ftl` ve `locales/tr.ftl` anahtarlarına taşıyıp `t!(...)` ile çağırın.
- **Tahmini efor:** S

#### 28. `README.md` / `README.tr.md` Özellik Listesinin v1.0.0 Gerçeğinin Gerisinde Kalması
- **Alan:** 10. Dağıtım
- **Şiddet:** Orta
- **Konum:** `README.md:44-48, 70`, `README.tr.md:44-48, 70`
- **Açıklama ve neden sorun olduğu:** `README` export için yalnızca JPEG/PNG'den söz eder ve `16-bit TIFF` / `WebP` ile `Color Grading`, `3D LUT`, `Masking`, `Clarity/Dehaze`, `Virtual Copies` gibi mevcut `1.0.0` yeteneklerini listelemez.
- **Önerilen çözüm:** `README.md` ve `README.tr.md` özellik ve export bölümlerini v1.0.0 ile hizalayın.
- **Tahmini efor:** S

#### 29. `generate_thumbnail_with_edits` Hızlı Yolunda `image::Limits` Olmaması
- **Alan:** 9. Güvenlik ve dayanıklılık
- **Şiddet:** Orta
- **Konum:** `crates/beam-media/src/decoder.rs:1570-1577`
- **Açıklama ve neden sorun olduğu:** `decode_raster_image` piksel bombalarına karşı `image::Limits` uygularken, düzenlenmemiş raster thumbnail üretimi doğrudan `image::open(path)` çağırır ve boyut/tahsis sınırı uygulamaz.
- **Önerilen çözüm:** `image::ImageReader::open(path)` + `reader.limits(limits)` kullanarak aynı `150 MP` / `1 GB` korumasını uygulayın.
- **Tahmini efor:** S

---

### Düşük Şiddetli Sorunlar

#### 30. Kullanılmayan Workspace Bağımlılıkları ve Yinelenen Kasa Sürümleri
- **Alan:** 4. Mimari ve kod kalitesi / 10. Dağıtım
- **Şiddet:** Düşük
- **Konum:** `crates/beam-app/Cargo.toml:19, 24`, `Cargo.lock` (`moxcms 0.8.1` vs `0.9.1`, `pollster 0.4.0` vs `1.0.1`)
- **Açıklama ve neden sorun olduğu:** `egui-wgpu` ve `pollster` `beam-app` içinde hiç kullanılmamasına rağmen bağımlılıklarda duruyor ve gereksiz derleme/ağaç karmaşası yaratıyor.
- **Önerilen çözüm:** `beam-app/Cargo.toml` içinden `egui-wgpu` ve `pollster` satırlarını kaldırın.
- **Tahmini efor:** S

#### 31. `parse_cube_file` Ters/Geçersiz `DOMAIN_MIN >= DOMAIN_MAX` Kontrolü Eksikliği
- **Alan:** 1. Doğruluk ve hatalar
- **Şiddet:** Düşük
- **Konum:** `crates/beam-core/src/lut.rs:81-91`, `shaders/lut3d.wgsl:72`
- **Açıklama ve neden sorun olduğu:** Bozuk bir `.cube` dosyası `DOMAIN_MIN >= DOMAIN_MAX` verirse shader tarafında aralık `1e-5`'e çöküp tüm renkleri doyuma götürür.
- **Önerilen çözüm:** `parse_cube_file` içinde `domain_min[i] < domain_max[i]` doğrulaması yapıp geçersizse anlamlı hata döndürün.
- **Tahmini efor:** S

#### 32. `save_user_preset` Dosya Adı Çakışma Riski
- **Alan:** 1. Doğruluk ve hatalar
- **Şiddet:** Düşük
- **Konum:** `crates/beam-core/src/preset.rs:660-672`
- **Açıklama ve neden sorun olduğu:** Alfanümerik olmayan karakterler `_` ile değiştirildiği için farklı Unicode/özel karakterli iki preset adı aynı dosya adına çarpışabilir.
- **Önerilen çözüm:** Temizlenen ada kısa bir kararlı hash son eki ekleyin veya benzersiz dosya adı çözümleyicisi kullanın.
- **Tahmini efor:** S

#### 33. `beam_core::i18n::t` Doc-Test'inin `ignored` Olması
- **Alan:** 7. Test ve güvenilirlik
- **Şiddet:** Düşük
- **Konum:** `crates/beam-core/src/i18n.rs:321`
- **Açıklama ve neden sorun olduğu:** Mevcut tek doc-test `ignored` olarak işaretlenmiş, derleme/çalışma doğrulamasından geçmiyor.
- **Önerilen çözüm:** Çalıştırılabilir bir örnek veya `compile_fail`/`no_run` yerine geçerli bir `i18n::t` testi haline getirin.
- **Tahmini efor:** S

#### 34. `beam-gpu` Testlerinin GPU Yoksa Sessizce Atlanması
- **Alan:** 7. Test ve güvenilirlik
- **Şiddet:** Düşük
- **Konum:** `crates/beam-gpu/src/pipeline.rs:2069-2539`
- **Açıklama ve neden sorun olduğu:** CI veya headless konteynerlerde `HeadlessGpu::new()` `None` dönerse GPU testleri `return;` ile sessizce başarılı sayılır; regresyonlar gözden kaçabilir.
- **Önerilen çözüm:** CI ortamında `lavapipe`/`llvmpipe` Vulkan yazılım sürücüsü ile koşup `BEAM_REQUIRE_GPU_TESTS=1` bayrağıyla `None` durumunda fail ettirin.
- **Tahmini efor:** S

#### 35. Flatpak Manifesti Eksikliği
- **Alan:** 10. Dağıtım
- **Şiddet:** Düşük
- **Konum:** `packaging/linux/` (`com.github.mozcelik14.beam.metainfo.xml` mevcut, manifest yok)
- **Açıklama ve neden sorun olduğu:** AppStream metainfo hazır olmasına rağmen Flathub/Flatpak dağıtımı için `.yml`/`.json` manifest dosyası bulunmuyor.
- **Önerilen çözüm:** `packaging/linux/com.github.mozcelik14.beam.yml` Flatpak manifesti ekleyin.
- **Tahmini efor:** M

---

### Doğrulanamayan Alanlar

1. **Gerçek Kamera RAW Dosyalarında (`ARW`/`CR3`/`NEF`/`RAF`) Renk Matrisi & Beyaz Dengesi Doğruluğu**: *Doğrulanamadı.* `cargo test` içinde gerçek kamera RAW dosyaları yerine sentetik testler kullanılmıştır ve `RawDevelop` adımlarında `WhiteBalance` / kamera profili dönüşümünün çıktısı gerçek bir D65 ColorChecker referansı ile kıyaslanmamıştır. *Nasıl doğrulanır:* `crates/beam-media` içine örnek bir DNG/ARW ColorChecker dosyası ile Delta-E (`ΔE2000`) referans testi eklenerek doğrulanabilir.
2. **HiDPI Kesirli Ölçekleme ve 10.000+ Fotoğraflı Kütüphane UI Akıcılığı**: *Doğrulanamadı.* Denetim salt okunur statik inceleme ve birim testleriyle sınırlıdır; kesirli HiDPI (`1.25x`/`1.5x`) ve büyük katalog benchmark'ı canlı GUI üzerinde ölçülmemiştir. *Nasıl doğrulanır:* `WINIT_X11_SCALE_FACTOR=1.5` altında görsel kontrol ve 10.000 sentetik kayıt içeren SQLite benchmark testi ile doğrulanabilir.

---

## Aşama 3: Yeni Özellik Önerileri

*(Not: Mevcut kod tabanında zaten bulunan Tone Curve, 8-bant HSL, 3-Yönlü Color Grading, 3D `.cube` LUT, Clarity/Dehaze/NR/Sharpening, Vignette/Grain, Crop/Rotate/Flip/Straighten, Gradyan/Radyal Parametrik Maskeler, Histogram, Virtual Copies, Filigran ve Toplu Export bu listede tekrar edilmemiştir; yalnızca profesyonel bir RAW stüdyosunda henüz eksik olan veya yarım kalan yetenekler önerilmiştir.)*

1. **Gerçek Highlight Recovery (Kanal Yeniden Yapılandırma) ve Film/AgX Ton Eşleme**
   - **Kullanıcıya değeri:** Kısmen patlamış (1 veya 2 kanalı doyuma ulaşmış) gökyüzü ve gelinlik/yansımalarda renk kayması olmadan doğal detay kurtarma ve yumuşak vurgu geçişi sağlar.
   - **Mevcut mimariye uyumu:** `beam-gpu` içinde `exposure` ve `highlights_shadows` arasına veya `display_transform.wgsl`/`export_transform.wgsl` sonuna eklenebilir; veri modeli `EditStack`'e `highlight_recovery: f32` ve `tone_mapper: enum` alanı eklenerek genişletilir.
   - **Tahmini efor:** M | **Bağımlılıklar:** Öncelikle Bulgu #2 (HDR kırpılmasının giderilmesi).

2. **Fırça (Brush) ve Renk/Parlaklık Aralığı (Color/Luminance Range) Maskeleri**
   - **Kullanıcıya değeri:** Mevcut gradyan ve radyal maskelere ek olarak fotoğrafın istenen bölgesini serbest fırça ile boyama veya sadece gökyüzü/ten tonu aralığını seçme imkanı verir.
   - **Mevcut mimariye uyumu:** `sidecar.rs` içindeki `MaskAdjustment` listesine `Brush` (stroke noktaları veya RLE/alpha maske tamponu) ve `Range` parametreleri eklenir; `mask_adjust.wgsl` bu katmanı okur.
   - **Tahmini efor:** L | **Bağımlılıklar:** Bulgu #3 (tile-local UV düzeltmesi).

3. **Leke ve Toz Giderme (Spot / Healing Brush)**
   - **Kullanıcıya değeri:** Sensör tozlarını, cilt kusurlarını veya küçük istenmeyen objeleri tahribatsız biçimde temizler.
   - **Mevcut mimariye uyumu:** `EditStack`'e `spots: Vec<SpotEdit>` (`center`, `source`, `radius`, `feather`, `mode: Clone | Heal`) eklenir; kırpma öncesi yeni bir WGSL geçişi olarak çalışır ve `mask_overlay.rs` içine etkileşimli nokta tutamacı eklenir.
   - **Tahmini efor:** L | **Bağımlılıklar:** Yok.

4. **Lens Profili Düzeltme (Distorsiyon, Kromatik Aberasyon, Lens Vinyet) ve Perspektif / Upright**
   - **Kullanıcıya değeri:** Geniş açı bükülmesini, mor saçaklanmayı (CA) ve mimari çekimlerdeki düşey perspektif eğikliğini (keystone) tek tıkla veya sürgüyle düzeltir.
   - **Mevcut mimariye uyumu:** `geometry.rs` ve `display_transform.wgsl` + `export_transform.wgsl` birleştirilip 3x3 homografi matrisi (`perspective_v`, `perspective_h`) ve kahverengi-Conrady (`k1`, `k2`, `ca_rb`) UV örneklemesi eklenir; kamera/lens EXIF bilgisiyle eşleştirilebilir.
   - **Tahmini efor:** L | **Bağımlılıklar:** Bulgu #3 ve #11 (geometri/straighten birleşimi).

5. **Soft Proofing (Ekran/Baskı Simülasyonu + Gamut Dışı Uyarısı)**
   - **Kullanıcıya değeri:** Baskı veya web çıktısı almadan önce seçili ICC profiline (sRGB, Display P3, Adobe RGB veya özel yazıcı ICC'si) göre renklerin nasıl görüneceğini ve gamut dışı kalan bölgeleri canlı gösterir.
   - **Mevcut mimariye uyumu:** `moxcms` zaten projede mevcuttur; `display_transform.wgsl` içine 3D proof LUT veya matris simülasyonu ve clipping overlay benzeri gamut uyarısı bayrağı eklenir.
   - **Tahmini efor:** M | **Bağımlılıklar:** Bulgu #6 (`Rgba16Float` 3D LUT desteği).

6. **Anlık Görüntüler (Snapshots) ve Çift Fotoğraf Karşılaştırma Modu (Compare View)**
   - **Kullanıcıya değeri:** Aynı fotoğraf üzerinde farklı düzenleme denemelerini ("Sıcak Film", "Yüksek Kontrast S&B") adlandırıp saklamayı ve çekim seçerken iki benzer fotoğrafı yan yana `%100` yakınlaştırarak kıyaslamayı sağlar.
   - **Mevcut mimariye uyumu:** `Sidecar` içine `snapshots: Vec<NamedSnapshot>` eklenir; Geliştir görünümünde ikinci bir `GpuImage` yuvası ile senkronize pan/zoom uygulanır.
   - **Tahmini efor:** M | **Bağımlılıklar:** Bulgu #8 (sidecar senkronizasyonu).

7. **Akıllı Koleksiyonlar (Smart Collections), Anahtar Kelime/Etiketleme ve Harita/Metadata Filtreleme**
   - **Kullanıcıya değeri:** Büyük arşivlerde "5 yıldız + Sony A7IV + 35mm + son 30 gün" gibi dinamik kurallarla otomatik koleksiyonlar oluşturur.
   - **Mevcut mimariye uyumu:** `beam-library` SQLite şemasına `tags`, `photo_tags`, `smart_collections` tabloları ve EXIF sütun indeksleri eklenir (Schema v2 migration).
   - **Tahmini efor:** M | **Bağımlılıklar:** Bulgu #15 (SQLite ölçekleme ve migration).

8. **Çoklu Çekim Araçları (HDR Birleştirme & Panorama) ve Tethered Çekim / Eklenti Sistemi**
   - **Kullanıcıya değeri:** Pozlama basamaklı (bracketing) çekimleri 32-bit DNG/HDR olarak birleştirir, panoramaları diker ve stüdyoda kameradan anlık aktarım yapar.
   - **Mevcut mimariye uyumu:** `beam-media` içine hizalama/birleştirme modülü, DNG yazıcı ve harici `gphoto2`/PTP köprüsü gerekir; eklenti sistemi için WASM (`wasmtime`) veya Lua sandboxing düşünülebilir.
   - **Tahmini efor:** L | **Bağımlılıklar:** Bulgu #17 (RAW renk hattı doğrulaması).

### Öncelik Sıralaması (Değer / Efor Oranına Göre)

| Sıra | Özellik | Değer | Efor | Öncelik Gerekçesi |
| :---: | :--- | :---: | :---: | :--- |
| 1 | **Gerçek Highlight Recovery & Ton Eşleme** | Çok Yüksek | M | RAW kalitesini doğrudan belirler; mevcut HDR kırpılma düzeltmesinin doğal devamıdır. |
| 2 | **Snapshots & Çift Fotoğraf Karşılaştırma** | Yüksek | M | Mevcut `Sidecar` ve `GpuImage` altyapısına düşük sürtünmeyle oturur. |
| 3 | **Soft Proofing & Gamut Dışı Uyarısı** | Yüksek | M | `moxcms` ve clipping overlay altyapısı zaten hazırdır. |
| 4 | **Akıllı Koleksiyonlar & Etiketleme** | Yüksek | M | SQLite katalog değerini katlar; kütüphane ölçeklemesiyle birlikte gelir. |
| 5 | **Lens Profili & Perspektif / Upright** | Çok Yüksek | L | Mimari ve manzara fotoğrafçılarının temel ihtiyacıdır. |
| 6 | **Leke / Toz Giderme (Spot Removal)** | Çok Yüksek | L | Sensör tozu temizliği için dış yazılıma çıkma ihtiyacını bitirir. |
| 7 | **Fırça & Renk/Parlaklık Aralığı Maskeleri** | Yüksek | L | Mevcut parametrik maskeleri tam yerel düzenleme motoruna dönüştürür. |
| 8 | **HDR / Panorama / Tethered / Eklenti** | Orta-Yüksek | L | İleri aşama stüdyo ve ekosistem genişlemesi. |

---

## Önerilen Yol Haritası (Milestones)

### Milestone 6: Stabilizasyon, Renk Doğruluğu & Kritik Hata Düzeltmeleri
- `Lut3DPass` `as_chunks::<4>()` ve `Rgba16Float` 3D LUT düzeltmesi (Bulgu #1, #6).
- Varsayılan Tone Curve / HSL HDR kırpılmasının ve `hsl.wgsl` sıfıra bölme riskinin giderilmesi (Bulgu #2).
- `render_tiled_to_rgba8` halo taşması + tam görüntü UV offset'i + `vignette` kırpma sırası + `straighten` aspect düzeltmesi (Bulgu #3, #11).
- `ColorGradingAdjustment` / `DetailAdjustment` ölçek birliği ve `Tiff16` gerçek 16-bit + ICC gömme (Bulgu #5, #7).
- Asenkron `decode_generation` ID'si, export/import çakışma kilidi, Undo/Redo `save_sidecar()`, `update_rating` sidecar koruması ve `libsqlite3-sys` güncellemesi (Bulgu #4, #8, #9, #10).

### Milestone 7: Performans, Büyük Kütüphane Ölçekleme & Test Güvencesi
- `BindGroup` ve dirty-stage önbelleklemesi, `1:1` drag throttling, canlı histogram güncellemesi (Bulgu #14, #18).
- Asenkron `rfd` diyalogları, disk thumbnail yüklemesinin worker'a alınması, `rayon` ile paralelleştirilmiş decode/export ve `image::Limits` koruması (Bulgu #13, #23, #29).
- SQLite toplu transaction'lar, `LIMIT/OFFSET` sayfalama, `busy_timeout`, kararlı thumbnail hash'i ve disk cache temizliği (Bulgu #15, #24).
- Global kısayol `wants_keyboard_input()` düzeltmesi, i18n eksikleri, `BeamApp` modülerleştirme ve tüm WGSL shader'lar için golden/CI testleri (Bulgu #12, #16, #25, #27).

### Milestone 8: Profesyonel Fotoğrafçılık Araçları (Renk, Detay, Geometri & İş Akışı)
- Gerçek Highlight Recovery & AgX/Filmic ton eşleme.
- Soft Proofing & Gamut dışı uyarısı.
- Snapshots & Çift Fotoğraf Karşılaştırma (Compare View).
- Lens düzeltme (distorsiyon, CA, vinyet) & Perspektif/Upright.
- Leke/Toz giderme (Spot/Healing Brush) ve `Preset` maskesinin tüm ayarları kapsaması (Bulgu #19).

### Milestone 9: İleri Düzey Kütüphane, Çoklu Çekim & Dağıtım Ekosistemi
- Fırça & Renk/Parlaklık Aralığı maskeleri.
- Akıllı Koleksiyonlar, Anahtar Kelime/IPTC/XMP ve EXIF arama.
- HDR Birleştirme, Panorama ve Flatpak / imzalı CI paketleme iyileştirmeleri (Bulgu #26, #35).
