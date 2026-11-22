# beaM. Kapsamlı Teknik Denetim Raporu

**Denetim Tarihi:** 2026-11-22  
**İncelenen Sürüm:** `v1.2.7` (`Cargo.toml:13`, 5 crate'li Cargo workspace + 15 WGSL shader + `fuzz/` + `packaging/` + `.github/workflows/`)  
**Kapsam:** Salt okunur kaynak kod, shader, test, bağımlılık ve dağıtım denetimi (hiçbir kod/yapılandırma değiştirilmemiştir).

---

## 1. Yönetici Özeti

`beaM.`, saf Rust (`rawler`, `wgpu`, `eframe`/`egui`, `rusqlite`, `moxcms`) ile geliştirilmiş, üretim kodunda **0 `unsafe` bloğu** barındıran, 109/109 geçen test paketine sahip temiz bir mimari temel üzerine kuruludur. Buna karşın, derin kaynak kod incelemesi **gerçek RAW fotoğraf iş akışını, renk doğruluğunu, güvenliği ve büyük kütüphane ölçeklenebilirliğini doğrudan etkileyen** kritik kusurlar ortaya çıkarmıştır.

### En Kritik 5 Bulgu

1. **[F-01 — KRİTİK / RAW & Renk] RAW Çözümlemede Beyaz Dengesi ve Kamera Renk Matrisi (Linear Rec.2020 Dönüşümü) Eksik (`crates/beam-media/src/decoder.rs:914-925`):** `rawler::RawDevelop` yalnızca `Rescale`, `Demosaic`, `FujiRotate` ve `CropActiveArea` çalıştırıyor; `WhiteBalance` ve `Calibrate` (kamera sensör RGB → CIE XYZ → Linear Rec.2020) uygulanmıyor. Tüm RAW dosyaları ham sensör RGB uzayında kalarak ağır yeşil/renk kaymasıyla işleniyor.
2. **[F-02 — KRİTİK / RAW] 64 KB Konteyner Doğrulaması Gerçek >64 KB RAW Dosyalarını Reddediyor (`crates/beam-media/src/decoder.rs:863-880`, `573-602`):** `validate_raw_container(&head[..n])` ilk 64 KB'lık tamponun uzunluğunu (`bytes.len()`) gerçek dosya boyutu varsayıyor. 64 KB sınırını aşan IFD/SubIFD/Strip offset'li TIFF/ARW/CR2/NEF/DNG dosyaları ve tüm gerçek Canon CR3 (`mdat`/`moov`) dosyaları hatalı biçimde "Corrupted" olarak reddediliyor.
3. **[F-03 — KRİTİK / Renk Bilimi] Hatalı ve Birbiriyle Çelişen `REC2020_TO_ADOBE_RGB` Matrisleri ve Geniş Gam Kırpması (`crates/beam-core/src/color.rs:72-75`, `shaders/display_transform.wgsl:58-62`, `shaders/export_transform.wgsl:44-48`, `crates/beam-media/src/decoder.rs:1148-1170`):** Adobe RGB (1998) matrisi hem matematiksel olarak yanlış hem de `display_transform.wgsl` ile `export_transform.wgsl`/`color.rs` arasında tamamen farklı. Ayrıca raster ICC dönüşümü tüm geniş gamlı (Display P3, Adobe RGB, ProPhoto) görselleri önce sRGB `[0, 1]` aralığına kırparak gamı kalıcı olarak yok ediyor.
4. **[F-04 — KRİTİK / Güvenlik] CLI Eklenti Yürütmesinde Keyfi Komut Çalıştırma, Windows Komut Enjeksiyonu ve Zaman Aşımı Eksikliği (`crates/beam-core/src/plugin.rs:80-148`):** Eklenti manifest doğrulaması yalnızca `".."` ve `'\0'` engelliyor; kullanıcı onayı/imza olmadan herhangi bir ikili (`sh`, `cmd`, `powershell`) çalıştırılabiliyor. Windows örnek eklentisi (`cmd /C "echo %BEAM_IMAGE_PATH%"`) dosya adından kabuk enjeksiyonuna açık. Ayrıca `cmd.output()` UI thread'inde (`crates/beam-app/src/app.rs:7247-7265`) zaman aşımı ve çıktı sınırı olmadan senkron çalışıyor.
5. **[F-05 — KRİTİK / Export & GPU] TIFF16 Export'ta Straighten/Sharpening/Watermark/EXIF Kaybı, 10-Bit `Rgba16Float` Kuantizasyonu ve `lens.wgsl` Karo (Tile) Dikiş Hatası (`crates/beam-media/src/export.rs:861-1004`, `crates/beam-gpu/src/pipeline.rs:2237,2353,2480`):** 16-bit TIFF export yolu `straighten`, `sharpening`, `watermark` ve `EXIF` verisini tamamen düşürüyor; `Rgba16Float` kullanımı `[0.5, 1.0]` aralığında gerçek 16-bit yerine 10-bit hassasiyet üretiyor; yüksek çözünürlüklü karolanmış (tiled) export'ta 8 px halo `lens.wgsl` bozulma/perspektif kaymalarına yetmediği için her karo kenarında ağır dikiş/sürüklenme (smearing) oluşuyor.

---

## 2. Otomatik Araç ve Komut Çıktıları

Aşağıdaki komutlar 2026-11-22 tarihinde depo kök dizininde (`rustc 1.99.0`) çalıştırılmıştır:

### `cargo check --workspace`
- **Durum:** Başarılı (exit code `0`, 0.48s)
- **Uyarı (1 adet):**
  - `crates/beam-app/src/widgets/histogram.rs:56:12` — `dead_code`: `HistogramData::compute_from_rgba32f` hiç kullanılmıyor.

### `cargo clippy --all-targets`
- **Durum:** Başarılı (exit code `0`, 4.49s)
- **Uyarılar (2 adet, `beam-app`):**
  1. `crates/beam-app/src/widgets/histogram.rs:56:12` — `dead_code`: `compute_from_rgba32f` kullanılmıyor.
  2. `crates/beam-app/src/widgets/histogram.rs:494:31` — `clippy::type_complexity`: `[(&[u16], f32, (u8, u8, u8)); 3]` için tip takma adı (`type`) öneriliyor.

### `cargo test --workspace`
- **Durum:** Başarılı (exit code `0`, toplam **109 geçti, 0 başarısız**, 1 doc-test `ignored`)
  - `beam-app`: 5/5 geçti (`0.00s`)
  - `beam-core`: 39/39 geçti (`0.04s`), 1 doc-test ignored (`crates/beam-core/src/i18n.rs:321`)
  - `beam-gpu`: 20/20 geçti (`4.06s`)
  - `beam-library`: 15/15 geçti (`5.18s`)
  - `beam-media`: 30/30 geçti (`1.09s`)

### `cargo audit`
- **Durum:** Başarılı (exit code `0`, 459 bağımlılık tarandı, 1555 RustSec kaydı yüklendi)
- **Bulgu (1 uyarı):**
  - `RUSTSEC-2026-0543` (`unmaintained`, 2026-09-14): `chrono 0.4.45` — *chrono is unmaintained* (`https://rustsec.org/advisories/RUSTSEC-2026-0543`).

---

## 3. Kapsamlı Bulgu Tablosu

| ID | Önem | Kategori | Dosya:Satır | Açıklama | Önerilen Çözüm | Efor |
| :--- | :--- | :--- | :--- | :--- | :--- | :---: |
| F-01 | Kritik | RAW / Renk | `crates/beam-media/src/decoder.rs:914-925` | `RawDevelop` içinde `WhiteBalance` ve kamera renk matrisi → Linear Rec.2020 dönüşümü (`Calibrate`) eksik. | `wb_coeffs` + `cam_to_xyz * XYZ_TO_REC2020` dönüşümü ve highlight recovery ekle. | L |
| F-02 | Kritik | RAW | `crates/beam-media/src/decoder.rs:863-880,573-602` | `validate_raw_container` 64 KB ön tamponun uzunluğunu toplam dosya boyutu sanarak >64 KB gerçek RAW/CR3 dosyalarını reddediyor. | Gerçek `total_file_size` parametresi geçir; 64 KB üstü offset'leri buna göre doğrula. | S |
| F-03 | Kritik | Renk Bilimi | `crates/beam-core/src/color.rs:72-75`, `shaders/display_transform.wgsl:58-62`, `shaders/export_transform.wgsl:44-48` | `REC2020_TO_ADOBE_RGB` matrisleri matematiksel olarak hatalı ve `display_transform` ile `export_transform` arasında tutarsız. | CIE xy birincillerinden `f64` ile tek kaynaktan türet, 3 dosyayı eşitle ve matris doğrulama testi ekle. | S |
| F-04 | Kritik | Renk Bilimi | `crates/beam-media/src/decoder.rs:1148-1170,1017-1022` | `transform_icc_to_linear_rec2020` önce sRGB'ye çevirip `[0,1]` kırptığı için Display P3 / Adobe RGB / ProPhoto geniş gamını yok ediyor. | `moxcms` hedef profilini doğrudan Linear Rec.2020 (gamma 1.0) tanımlayıp tek geçişte dönüştür. | M |
| F-05 | Kritik | Export | `crates/beam-media/src/export.rs:861-1004,1125-1145` | `export_tiff16_decoded_image` `straighten`, `sharpening`, `watermark` ve `EXIF` verisini tamamen düşürüyor; erişilemeyen ölü kod var. | 16-bit crop/straighten/unsharp/watermark + EXIF gömme desteği ekle; ölü dalı sil. | M |
| F-06 | Kritik | Düzenleme Hattı | `crates/beam-gpu/src/pipeline.rs:2237,2480`, `shaders/lens.wgsl:35-49,82-109` | Karolanmış (tiled) export'ta `TILE_HALO = 8` px `lens.wgsl` bozulma/CA/perspektif kaymasına yetmiyor; karo kenarlarında ağır dikiş/smearing oluşuyor. | `lens` aktifken karo başına ters-warp kaynak sınırlayıcı kutu hesapla veya tam kare lens geçişi uygula. | M |
| F-07 | Kritik | Güvenlik | `crates/beam-core/src/plugin.rs:80-148` | CLI eklentileri onaysız keyfi komut çalıştırıyor, Windows örnek eklentisinde kabuk enjeksiyonu var, zaman aşımı ve çıktı sınırı yok. | SHA-256 + kullanıcı onay listesi ekle, `cmd /C` sarmalayıcısını kaldır, 30s timeout ve 1 MB çıktı sınırı koy. | M |
| F-08 | Kritik | Performans | `crates/beam-app/src/app.rs:7247-7265,5198-5232,1176,1314,1571-1608,1638-1648`, `crates/beam-app/src/views/library.rs:217-221,1584-1606` | Eklenti çalıştırma, Auto Sky/Subject segmentasyonu, 10 `rfd` diyaloğu, disk thumbnail decode, tüm kütüphanede `exists()` ve SQLite/sidecar yazımları UI thread'ini blokluyor. | Tüm disk/SQLite/eklenti/segmentasyon/diyalog işlemlerini arka plan iş parçacıklarına ve `BackgroundMsg` kanalına taşı. | L |
| F-09 | Yüksek | Renk Bilimi | `crates/beam-core/src/color.rs:170-179`, `shaders/white_balance.wgsl:40` | Beyaz dengesi Bradford CAT yerine Rec.2020'de basit diyagonal RGB kazancı kullanıyor; parlaklık ve ton kayması yaratıyor. | Bradford CAT `mat3x3<f32>` dönüşüm matrisi hesaplayıp `white_balance.wgsl`'e uniform olarak geç. | M |
| F-10 | Yüksek | Renk Bilimi | `shaders/tone_curve.wgsl:41-76`, `crates/beam-core/src/preset.rs:500` | `if (val <= 0.0) { return 0.0; }` kaldırılmış siyah noktayı (`builtin_matte`) bozuyor; 256'lık lineer LUT gölgelerde yalnızca ~3 girişe düşüp banding yapıyor. | `val <= 0.0` erken dönüşünü kaldır; LUT'u algısal (log/gamma) shaper ile 1024 girişe çıkar. | M |
| F-11 | Yüksek | Renk Bilimi | `shaders/hsl.wgsl:127-153` | `d_hue / total_weight` normalizasyonu tek bantlı kenar bölgelerde `w=0.001` iken %100 etki uygulayıp `1e-4`'te sıfıra düşerek sert ton kırılması yaratıyor. | `/ total_weight` yerine `/ max(total_weight, 1.0)` veya toplamı 1 olan örtüşen bant ağırlıkları kullan. | S |
| F-12 | Yüksek | Düzenleme Hattı | `shaders/detail.wgsl:55-161` | `blur3`, `dehaze`, `clarity` ve `sharpening` doğrudan ham `t_input` okuyarak `ai_denoise` çıktısını yok sayıp gürültüyü geri enjekte ediyor. | `ai_denoise`/NR aşamasını `dehaze`/`clarity`/`sharpening` öncesi ayrı ping-pong geçişine ayır. | M |
| F-13 | Yüksek | Export / GPU | `crates/beam-gpu/src/pipeline.rs:2353,2456-2459` | 16-bit export `Rgba16Float` üzerinden yapıldığı için `[0.5, 1.0]` aralığında yalnızca 10-bit (1024 adım) hassasiyet üretiyor. | 16-bit export hedefini `Rgba32Float` yapıp doğrudan `f32 -> u16` dönüştür. | S |
| F-14 | Yüksek | Düzenleme Hattı | `crates/beam-gpu/src/pipeline.rs:1882-1894,2001-2028`, `shaders/display_transform.wgsl:100-146`, `shaders/export_transform.wgsl:73-107` | Aşama sırası hatalı (maskeler grain/vignette'ten sonra, vignette kırpmadan önce, denoise pozlama/kontrasttan sonra); `export_transform.wgsl` kırpma/döndürme içermiyor. | Pipeline sırasını fiziksel/fotoğrafik doğru sıraya al; kırpma sonrası vignette ve ortak transform bloğu uygula. | L |
| F-15 | Yüksek | Güvenlik | `crates/beam-core/src/sidecar.rs:746-761,802-803,963,1125-1146`, `crates/beam-gpu/src/pipeline.rs:1055`, `crates/beam-core/src/preset.rs:745`, `crates/beam-core/src/i18n.rs:171` | `.beam` içindeki `lut.path` doğrulanmadan açılıyor (keyfi dosya/UNC okuma); `snapshots` ve `tone_curve` noktaları sınırsız; RON özyineleme sınırı ve preset/config dosya boyutu sınırı yok. | `lut.path`'i `.cube` ve izinli dizinlerle kısıtla; `snapshots` ve eğri noktalarını sınırla; `ron::Options::with_recursion_limit(Some(32))` ve boyut kontrolü ekle. | M |
| F-16 | Yüksek | Güvenlik / Export | `crates/beam-media/src/export.rs:1261-1271,1208-1229` | `custom_suffix` ve `generate_output_path` yol geçişi (path traversal) sanitizasyonunu atlıyor. | Tüm çıktı adlarını ortak `sanitize_filename_component` fonksiyonundan geçir. | S |
| F-17 | Yüksek | Export | `crates/beam-media/src/export.rs:314-377,952-954,1086-1155` | `extract_raw_exif` iki yolda tutarsız `"Exif\0\0"` öneki dönüyor ve TIFF tabanlı RAW EXIF'ini kaçırıyor; `sanitize_exif_orientation` ham byte yaması yapıyor; ICC/EXIF yazım hataları sessizce yutuluyor. | 6-byte öneki normalize et, RAW EXIF'i `rawler`/`kamadak-exif` ile çıkar, IFD0 parse ederek orientation sıfırla, hataları logla/ilet. | M |
| F-18 | Yüksek | RAW | `crates/beam-media/src/decoder.rs:1005-1008,1037-1043,1562-1592` | RAW EXIF alanları (`iso`, `shutter_speed`, `aperture`, `focal_length`) hep `None`; `extract_image_dimensions` RAW için `None` (0×0); thumbnail üretiminde gömülü RAW önizlemesi kullanılmıyor. | RAW metadata ve başlık boyutlarını oku; düzenlenmemiş RAW thumbnail'leri için gömülü JPEG önizlemesi kullan. | M |
| F-19 | Yüksek | Kütüphane | `crates/beam-library/src/lib.rs:325-467,978-1013,1164-1188` | IPTC/XMP ve etiketler diske (`.xmp`/`.beam`) yazılmıyor; `created_at` EXIF `DateTimeOriginal` yerine dosya `mtime` Unix string'i kaydediyor. | XMP/sidecar senkronizasyonu ekle; `created_at` için EXIF `DateTimeOriginal` (ISO-8601) veya `INTEGER` epoch kullan. | M |
| F-20 | Yüksek | Kütüphane / Performans | `crates/beam-library/src/lib.rs:365-385,487-549,555-677,821-861,1191-1258` | 50k+ fotoğrafta sayfalama yok, indekssiz `LIKE` tam tablo taramaları, `find_duplicates`'ta N+1 + tam dosya `fs::read` + FNV-1a, `BEGIN IMMEDIATE` içinde senkron thumbnail üretimi ve transaction'sız toplu silme. | Sayfalama, `file_ext`+`file_size`+FTS5 indeksleri, streaming SHA-256/BLAKE3 hash önbelleği, transaction dışı thumbnail işçisi ekle. | L |
| F-21 | Yüksek | Performans / Bellek | `crates/beam-media/src/decoder.rs:923-981,1158-1170`, `crates/beam-media/src/export.rs:143-275`, `crates/beam-app/src/app.rs:100,1304,1335-1368` | 100 MP RAW'da 3.2–4.8 GB pik RAM (çoklu `Vec<f32>` kopyası), `current_image` tüm oturum boyunca RAM'de tutuluyor; CPU döngülerinde `rayon` yok; `generate_proxy` yüksek küçültmede bilinear aliasing yapıyor. | Chunk tabanlı in-place ICC/orientation, `rayon` paralelleştirme, area/Lanczos küçültme ve isteğe bağlı tam çözünürlük yükleme uygula. | L |
| F-22 | Yüksek | Performans / GPU | `crates/beam-gpu/src/pipeline.rs:390-403,1485-1785,1896,2169,2433` | Her karede 14'e kadar yeni `BindGroup`, koşulsuz 4×256 spline LUT yeniden hesabı ve 2–3 ayrı `CommandEncoder`/`queue.submit` yapılıyor. | `BindGroup` önbellekle, dirty-flag ile sadece değişen uniform'ları güncelle, geçişleri tek `CommandEncoder`'da birleştir. | M |
| F-23 | Yüksek | Güvenilirlik / Bellek | `crates/beam-app/src/app.rs:767-789,1305-1307`, `crates/beam-app/src/views/library.rs:190-204` | `close_project()` ve `upload_proxy_and_render()` `renderer.free_texture` çağırmadan `egui::TextureId` sızdırıyor ve tam çözünürlüklü GPU kaynaklarını bırakmıyor; thumbnail önbelleği rastgele `HashMap` silmesiyle thrashing yapıyor. | Tüm doku çıkışlarında `free_texture` çağır, oturum alanlarını tam temizle, thumbnail önbelleğini LRU'ya çevir. | S |
| F-24 | Yüksek | Düzenleme / Önizleme | `crates/beam-app/src/app.rs:1240,1294-1298,1451-1477,5953-5990` | İlk histogram ham lineer proxy'den hesaplanıyor (Auto Tone'u saptırıyor); sürgü sürüklerken her karede tam çözünürlük GPU + CPU histogram çalışıyor; WB damlalığı kırpma/döndürmeyi yok sayıyor. | İlk yüklemede `render_with_proof_options` + sRGB çıktısından skop hesapla; sürüklemede proxy/debounce kullan; WB damlalığına ters geometri dönüşümü ekle. | M |
| F-25 | Yüksek | CI/CD & Paketleme | `.github/workflows/release.yml:77,139`, `packaging/linux/build-linux.sh:21,28-33,42,46`, `packaging/windows/beam-installer.iss:3` | PR/push CI workflow'u yok; release test çalıştırmadan yayınlıyor; `appimagetool` SHA256 doğrulamasız `continuous` tag'inden indiriliyor; Windows `.iss` `1.2.6` sabit (1.2.7 değil); Linux `.tar.gz` içinde GPL-3.0 `LICENSE` dosyası eksik; `|| true` hataları gizliyor. | `ci.yml` ekle, release öncesi test zorunlu kıl, `appimagetool` SHA256 sabitle, `.tar.gz`'ye `LICENSE` ekle, `/DMyAppVersion` geçir, `--locked` kullan. | M |
| F-26 | Orta | Renk Bilimi | `crates/beam-gpu/src/pipeline.rs:1634`, `shaders/color_grading.wgsl:41,52-54`, `shaders/vibrance_saturation.wgsl:36-40,86-89`, `shaders/lut3d.wgsl:32,71`, `crates/beam-core/src/color.rs:131-158` | `wheel_to_vec4` Rec.709 luma kullanırken shader Rec.2020 kullanıyor; color grading ağırlıkları >1.0 olabiliyor; Neutral profil siyahı kaldırıyor; cilt koruması sert dallanmalı; 3D LUT HDR/geniş gamı kırpıyor; CPU `color.rs`'te highlight rolloff eksik. | Rec.2020 luma eşitle, ağırlıkları normalize et, `smoothstep` cilt koruması ekle, CPU/GPU rolloff'u hizala. | M |
| F-27 | Orta | Düzenleme Hattı | `crates/beam-core/src/sidecar.rs:833`, `crates/beam-gpu/src/pipeline.rs:1704-1748`, `shaders/mask_adjust.wgsl:41,112-132`, `shaders/vignette_grain.wgsl:58-59` | Tüm fırça maskeleri tek 128 noktalık havuzu paylaşıyor (2. maske sessizce kesiliyor); UV uzayında en-boy oranı düzeltilmediği için fırça ve vinyet eliptik çıkıyor; piksel başına O(128) döngü. | Fırça darbelerini `R8Unorm` maske dokusuna rasterize et ve aspect-ratio düzeltmesi ekle. | M |
| F-28 | Orta | Güvenilirlik | `crates/beam-media/src/decoder.rs:882-925,1109-1131,1323-1378`, `crates/beam-gpu/src/pipeline.rs:1797,1984`, `crates/beam-core/src/segmentation.rs:19,131-138`, `crates/beam-core/src/lut.rs:52-99`, `crates/beam-app/src/app.rs:300,3050` | `develop_intermediate` `catch_unwind` dışında; RAW'da `MAX_IMAGE_DIMENSION` eksik; 32F TIFF 8-bit'e düşüyor; `pipeline.rs` ve `app.rs`'te birkaç üretim `.unwrap()`/`.expect()` var; `segmentation.rs`'te `NaN` yayılımı; `lut.rs` yinelenen başlığı reddetmiyor. | `catch_unwind` ve boyut/NaN/checked_mul korumalarını tamamla; üretim `unwrap`/`expect` çağrılarını kaldır. | M |
| F-29 | Orta | Kütüphane | `crates/beam-library/src/lib.rs:187-192,205-317,610-625,788-905,1061-1090,1262-1282` | `save_smart_collection` conflict update'te yanlış ID (`last_insert_rowid`) dönüyor; `LIKE` wildcard kaçışı yok ve Windows `\` yol filtresi bozuk; `SQLITE_OPEN_URI` riski; `relink_folder` virtual copy yollarını güncellemiyor; silinen fotoğrafın diskteki sanal kopyaları yetim kalıyor. | `RETURNING id`, `LIKE ... ESCAPE '\'` (veya aralık sorgusu), platform bağımsız ayırıcı, virtual copy relink/temizlik ekle. | M |
| F-30 | Orta | Güvenilirlik / Export | `crates/beam-media/src/export.rs:280-291,478-509,731,968,1173,1249-1310`, `crates/beam-app/src/views/library.rs:324-336` | 8-bit `crop.x==1.0` sınır hatası; batch export seri ve `date`/`rating` hardcoded; rename TOCTOU ve `sync_all` eksik; watermark logosu `Limits` olmadan açılıyor; Tethered Watch her 2 saniyede `config.ron` diske yazıyor ve kısmi yazılmış RAW'ı okuyabiliyor. | 8-bit crop clamp, `notify` watcher + debounce, `OpenOptions::create_new` + `sync_all`, logo `Limits` ve gerçek tarih/puan aktarımı ekle. | M |
| F-31 | Orta | Fuzzing & Güvenlik | `fuzz/fuzz_targets/fuzz_raw_decode.rs:17-39`, `SECURITY_AUDIT.md:3-4,95,192`, `deny.toml:103,113,230-233` | `fuzz_raw_decode` `catch_unwind` ile panic'leri yutuyor (libFuzzer çökme yakalayamaz); LUT/plugin/XMP/template fuzz hedefi yok; `SECURITY_AUDIT.md` v0.1.1'de kalmış; `deny.toml` GPL uyumsuz `Ubuntu-font-1.0` izinli ve `unknown-registry = "warn"`. | `catch_unwind`'ı fuzz hedefinden kaldır, yeni fuzz hedefleri ekle, `deny.toml` ve `SECURITY_AUDIT.md`'yi güncelle. | S |
| F-32 | Orta | Kod Kalitesi | `crates/beam-app/src/app.rs:1-8148`, `crates/beam-app/src/widgets/histogram.rs:56,494` | `app.rs` 8.148 satırlık monolit (`ui()` tek başına 4.728 satır); 2 clippy/dead_code uyarısı; `chrono 0.4.45` `RUSTSEC-2026-0543` (unmaintained) uyarısı. | `app.rs`'i `state`, `worker`, `views/develop`, `panels/*`, `dialogs/*` modüllerine böl; clippy uyarılarını temizle; `chrono` bağımlılığını (transitive) izle. | L |
| F-33 | Orta | Yerelleştirme & Erişilebilirlik | `Cargo.toml:25`, `locales/en.ftl:369`, `locales/tr.ftl:369`, `crates/beam-app/src/app.rs:384-431,483,542,616-749,7214`, `crates/beam-app/src/widgets/histogram.rs:283-290` | `eframe` içinde `accesskit` kapalı (ekran okuyucu yok); Hakkında diyaloğu sabit `1.0.0` yazıyor; çok sayıda sabit İngilizce hata/etiket metni; açık/yüksek kontrastlı tema ve UI ölçek ayarı yok. | `accesskit` özelliğini etkinleştir, `CARGO_PKG_VERSION` kullan, sabit metinleri `.ftl`'ye taşı, ikon düğmelere tooltip/erişilebilir etiket ekle. | M |
| F-34 | Düşük | Paketleme | `packaging/linux/build-deb.sh:84`, `packaging/beam.desktop:10`, `crates/beam-app/src/main.rs:18`, `packaging/windows/beam-installer.iss:57-61`, `packaging/com.github.mozcelik14.beam.metainfo.xml:53` | `.deb` yalnızca `libc6` bağımlı (Vulkan/Wayland/X11/GTK3 yok); `beam.desktop` `%F` geçerken `main.rs` yalnızca `args().nth(1)` okuyor; Windows kurucu `.raf/.rw2/.orf/.pef` kaydetmiyor; `metainfo.xml` 1.2.6'da kalmış. | `dpkg-shlibdeps`/eksik bağımlılıkları ekle, `main.rs`'te çoklu dosya argümanı destekle, Windows uzantılarını ve `metainfo.xml`'i güncelle. | S |

---

## 4. Kritik ve Yüksek Bulguların Ayrıntılı İncelemesi ve Somut Düzeltme Önerileri

### F-01 [Kritik] — RAW Çözümlemede White Balance ve Kamera → Linear Rec.2020 Renk Matrisi Eksik
- **Konum:** `crates/beam-media/src/decoder.rs:914-925`
- **Teknik Analiz:** `RawDevelop::steps` yalnızca `[Rescale, Demosaic, FujiRotate, CropActiveArea]` adımlarını içeriyor. `ProcessingStep::WhiteBalance` ve `ProcessingStep::Calibrate` çağrılmadığı için `Intermediate::ThreeColor` tamponu ham kamera sensörü RGB uzayında kalıyor. Oysa `beam-gpu` tüm shader'larda (Rec.2020 luma ağırlıkları `0.2627, 0.6780, 0.0593` ve `REC2020_TO_SRGB` matrisi) girdinin **D65 Linear Rec.2020** olduğunu varsayar.
- **Somut Düzeltme:**
  1. `rawimage.wb_coeffs` (geçerli değilse `rawimage.neutralwb` / D65) ile kanal başına beyaz dengesi normalizasyonu uygula; highlight clipping öncesi `> 1.0` değerlerini koruyarak opsiyonel highlight reconstruction (en azından kanal oran korumalı clip/inpaint) ekle.
  2. `rawler`'ın `rawimage.color_matrix` (`XYZ_D65 -> CamRGB`) matrisinin 3×3 tersini (`CamRGB -> XYZ_D65`) hesaplayıp `beam_core::color::XYZ_TO_REC2020_D65` ile çarp (`M = XYZ_TO_REC2020_D65 * cam_to_xyz`) ve her piksele uygula.
  3. Gerçek bir DNG/ARW/CR2 referans örneğiyle ColorChecker / gri kart golden-image testi ekle.

### F-02 [Kritik] — `validate_raw_container` 64 KB Tamponu Gerçek Dosya Boyutu Sanıyor
- **Konum:** `crates/beam-media/src/decoder.rs:863-880`, `573-602`, `354-383`, `562-566`, `684-705`, `742-754`
- **Teknik Analiz:** `decode_raw_with_rawler` dosyanın yalnızca ilk 64 KB'ını (`head`) okuyup `validate_raw_container(&head[..n])` çağırıyor (`877`). `validate_raw_container` ise `bytes.len()` değerini `total_file_size` olarak geçiriyor (`598`). Gerçek kamera RAW dosyalarının tamamı >64 KB olduğundan:
  - TIFF/ARW/CR2/NEF/DNG/RW2/ORF/PEF dosyalarında `val_offset + data_size > 65536` olan IFD/SubIFD/StripOffsets girişleri (`354-358`, `377-382`) doğrudan reddedilir.
  - Canon CR3 (BMFF) dosyalarında `mdat`/`moov` kutusu >64 KB olduğundan `562-566` tüm gerçek CR3 dosyalarını reddeder.
- **Somut Düzeltme:** `validate_raw_container(head: &[u8], total_file_size: u64)` imzasına geç. `head` sınırları içinde kalan yapıları `head.len()` ile, dosya gövdesine işaret eden offset/box uzunluklarını ise `total_file_size` ile doğrula. Testlere 128 KB+ sentetik TIFF/CR3 geçerli konteyner testleri ekle.

### F-03 [Kritik] — Hatalı ve Çelişkili `REC2020_TO_ADOBE_RGB` Matrisleri
- **Konum:** `crates/beam-core/src/color.rs:72-75`, `shaders/export_transform.wgsl:44-48`, `shaders/display_transform.wgsl:58-62`
- **Teknik Analiz:** Adobe RGB (1998) ve sRGB aynı Kırmızı `(0.64, 0.33)`, Mavi `(0.15, 0.06)` ve D65 beyaz noktasına sahiptir; bu nedenle `REC2020_TO_ADOBE_RGB`'nin 2. satırı (`G` kanalı) `REC2020_TO_SRGB`'nin 2. satırına (`[-0.1245505, 1.1328999, -0.0083494]`) eşit olmak zorundadır. Mevcut her iki matris de bunu sağlamıyor ve üstelik `display_transform.wgsl` ile `export_transform.wgsl` birbirinden tamamen farklı katsayılar kullanıyor.
- **Somut Düzeltme:** Tüm renk matrislerini CIE xy birincillerinden `f64` hassasiyetinde hesaplayıp tek doğru kaynak olarak sabitle (D65 satır-bazlı `REC2020_TO_ADOBE_RGB`: `[[1.15198, -0.09751, -0.05448], [-0.12455, 1.13290, -0.00835], [-0.02253, -0.04981, 1.07234]]`). `color.rs`, `display_transform.wgsl` ve `export_transform.wgsl`'i eşitle; `M_ADOBE_TO_SRGB * M_REC2020_TO_ADOBE == M_REC2020_TO_SRGB` birim testi ve Display-vs-Export eşdeğerlik GPU testi ekle.

### F-04 [Kritik] — Raster ICC Dönüşümünde Geniş Gamın sRGB'ye Kırpılması
- **Konum:** `crates/beam-media/src/decoder.rs:1148-1170`, `1017-1022`
- **Teknik Analiz:** `transform_icc_to_linear_rec2020` önce `ColorProfile::new_srgb()` hedefine dönüştürüp ardından `srgb_to_rec2020_linear` içinde `clamp(0.0, 1.0)` uyguluyor. Display P3, Adobe RGB veya ProPhoto ICC profiline sahip bir JPEG/PNG/TIFF dosyası açıldığında sRGB dışındaki tüm doygun renkler girişte kırpılır.
- **Somut Düzeltme:** `moxcms::ColorProfile` ile doğrudan BT.2020 birincilleri ve lineer TRC (`gamma = 1.0`) içeren hedef profil oluştur; tek geçişte `src_profile -> linear_rec2020` dönüşümünü satır/chunk bazında in-place çalıştır.

### F-05 [Kritik] — TIFF16 Export Yolunda Straighten, Sharpening, Watermark ve EXIF Kaybı
- **Konum:** `crates/beam-media/src/export.rs:861-979`, `995-1004`, `1125-1145`
- **Teknik Analiz:** `export_tiff16_decoded_image` yalnızca 90/180/270 döndürme, çevirme ve dikdörtgen kırpma uygular; `straighten`, `config.sharpening`, `config.watermark` ve `raw_exif` tamamen atlanır. Ayrıca `1125-1145` arasında erişilemeyen ölü bir `ExportFormat::Tiff16` dalı bulunmaktadır.
- **Somut Düzeltme:** 16-bit (`Rgba<u16>` veya `f32`) destekleyen `apply_crop_and_orientation_16`, `apply_unsharp_mask_16` ve `apply_watermark_16` fonksiyonlarını uygula; `raw_exif` parametresini geçirip `sanitize_exif_orientation` sonrası TIFF encoder'a yaz; `1125-1145` ölü kodunu temizle.

### F-06 [Kritik] — `lens.wgsl` Karolanmış (Tiled) Render Dikiş/Smearing Hatası
- **Konum:** `crates/beam-gpu/src/pipeline.rs:2237,2480`, `shaders/lens.wgsl:35-49,82-109`
- **Teknik Analiz:** `TILE_HALO = 8` piksel yalnızca 5×5 çekirdekli `detail.wgsl` için yeterlidir. `lens.wgsl` radyal bozulma (`k1 = ±0.35`), yanal CA (`±1.5%`) ve perspektif (`±0.45`) uyguladığında kaynak koordinatları yüzlerce piksel kayar; karo sınırını aşan örnekler kenara yapışır (`clamp`) ve her karo birleşiminde ağır çizgiler oluşur.
- **Somut Düzeltme:** Karo döngüsünde `lens` aktifse her hedef karonun 4 köşesini (ve kenar orta noktalarını) `lens.wgsl` ters/ileri formülüyle eşleyerek gereken kaynak dikdörtgenini dinamik hesapla (görüntü sınırına clamp ederek) ve o kaynak bölgeyi `uv_offset`/`uv_scale` ile yükle. `test_render_tiled_matches_monolithic_with_lens` testi ekle.

### F-07 [Kritik] — CLI Eklenti Yürütme Güvenliği
- **Konum:** `crates/beam-core/src/plugin.rs:80-148`, `crates/beam-app/src/app.rs:7247-7265`
- **Teknik Analiz:**
  1. `validate_manifest` herhangi bir sistem komutunun (`sh`, `powershell`, `rm`, `curl`) çalıştırılmasına izin verir.
  2. Windows örnek eklentisi `cmd /C "echo %BEAM_IMAGE_PATH%"` (`106-107`) dosya adında `&` veya `|` karakteri olduğunda kabuk komut enjeksiyonuna açıktır.
  3. `cmd.output()` zaman aşımı ve çıktı boyutu sınırı olmadan UI thread'inde bloklar.
- **Somut Düzeltme:** Eklenti manifestlerine SHA-256 doğrulamalı açık kullanıcı onay akışı ekle; örnek eklentiyi kabuk sarmalayıcısız güvenli bir yardımcıya çevir; çalıştırmayı arka plan thread'inde `Command::spawn` + 30s timeout (`wait_timeout` / polling + `kill`) ve 1 MB `take()` çıktı sınırı ile yürüt.

### F-08 [Kritik] — UI Thread'ini Bloklayan Senkron İşlemler
- **Konum:** `crates/beam-app/src/app.rs:995-1004,1034-1038,1176,1314,1547-1556,1571-1608,1638-1648,2253-2347,2781-2789,5016-5019,5198-5232,6591,6650,7247-7265`, `crates/beam-app/src/views/library.rs:217-221,1499,1584-1606`
- **Teknik Analiz:** Kütüphane yenilemede tüm fotoğraflar için `Path::exists()`, kare başına 8 adede kadar senkron `image::open` thumbnail çözümü, 10 adet senkron `rfd::FileDialog`, senkron `detect_sky_mask`/`detect_subject_mask`, senkron `run_plugin`, her undo/redo adımında `fsync`'li `save_sidecar()` ve her `ThumbnailReady` mesajında SQLite yazımı arayüzü donduruyor.
- **Somut Düzeltme:** Dedicated IO/DB worker thread'i kur: (a) eksik dosya taraması ve disk thumbnail decode arka planda `ColorImage` üretsin; (b) `rfd` çağrıları arka plan thread'inde çalışsın; (c) sidecar yazımları 300 ms debounce ile asenkron yapılsın; (d) thumbnail DB güncellemeleri 20–50'lik batch transaction'larla işlensin; (e) segmentasyon ve eklenti çalıştırma arka planda yürütülsün.

### F-09 [Yüksek] — Diyagonal Beyaz Dengesi Yerine Bradford CAT
- **Konum:** `crates/beam-core/src/color.rs:170-179`, `shaders/white_balance.wgsl:40`
- **Düzeltme:** Sıcaklık/Ton değerlerinden hedef beyaz nokta `(x, y)` → XYZ hesaplayıp Bradford koni adaptasyon matrisini (`M_REC2020_TO_XYZ -> M_BRADFORD -> scale -> M_BRADFORD_INV -> M_XYZ_TO_REC2020`) tek bir `mat3x3<f32>` olarak `white_balance.wgsl`'e aktar.

### F-10 [Yüksek] — Ton Eğrisi Siyah Nokta Hatası ve Gölge Kuantizasyonu
- **Konum:** `shaders/tone_curve.wgsl:41-76`, `crates/beam-core/src/color.rs:204-317`
- **Düzeltme:** `tone_curve.wgsl:42-44`'teki `if (val <= 0.0) { return 0.0; }` bloğunu kaldırarak `0.0` değerinin `lut[0]` üretmesini sağla. LUT'u algısal bir shaper (`pow(x, 1/2.2)` veya log2) üzerinden 1024 girişle örnekle ve `val > 1.0` için uç türev (`slope_at_1`) ile ekstrapolasyon yap.

### F-11 [Yüksek] — HSL Bant Kenarı Süreksizliği
- **Konum:** `shaders/hsl.wgsl:127-153`
- **Düzeltme:** `hsl.wgsl:135-153` içinde `/ total_weight` yerine `/ max(total_weight, 1.0)` kullan (veya 8 bandın tüm 360° dairede toplamı 1.0 olacak şekilde örtüşen kosinüs pencere ağırlıkları tanımla).

### F-12 [Yüksek] — `detail.wgsl` İçinde `ai_denoise` Çıktısının Yok Sayılması
- **Konum:** `shaders/detail.wgsl:55-161`
- **Düzeltme:** `detail.wgsl`'i iki ayrı ping-pong aşamasına böl: (1) `denoise.wgsl` (`ai_denoise`, `noise_reduction_luma`, `noise_reduction_color`), (2) `detail.wgsl` (`dehaze`, `clarity`, `sharpening`). Böylece keskinleştirme ve netlik, gürültüsü giderilmiş dokuyu okur.

### F-13 [Yüksek] — 16-Bit Export'ta `Rgba16Float` 10-Bit Kuantizasyonu
- **Konum:** `crates/beam-gpu/src/pipeline.rs:2353,2456-2459`
- **Düzeltme:** `export_display_pipeline_16f` hedef dokusunu `wgpu::TextureFormat::Rgba32Float` olarak değiştir ve readback'te `(val.clamp(0.0, 1.0) * 65535.0).round() as u16` uygula. 16-bit gradient testini `unique_vals.len() > 16` yerine tam 16-bit monotonic adım doğrulamasına yükselt.

### F-14 [Yüksek] — Pipeline Aşama Sırası ve Display-Export Ayrışması
- **Konum:** `crates/beam-gpu/src/pipeline.rs:1882-1894`, `shaders/display_transform.wgsl:100-146`, `shaders/export_transform.wgsl:73-107`
- **Düzeltme:** Aşama sırasını `Lens -> Denoise -> WB -> Exposure -> Camera Profile -> Masks -> H/S -> Contrast -> Tone Curve -> HSL -> Vib/Sat -> Color Grading -> Clarity/Dehaze/Sharpen -> LUT3D -> Crop-aware Vignette/Grain -> Display/Export` düzenine çek. Kırpma ve ufuk düzeltme bilgisini `vignette_grain.wgsl` ve `export_transform.wgsl` ile paylaş.

### F-15 [Yüksek] — Sidecar `lut.path` Traversal, Sınırsız `snapshots`/Eğri ve RON Özyineleme
- **Konum:** `crates/beam-core/src/sidecar.rs:746-761,802-803,963,1125-1146`, `crates/beam-gpu/src/pipeline.rs:1055`, `crates/beam-core/src/preset.rs:745`, `crates/beam-core/src/i18n.rs:171`
- **Düzeltme:** `validate_and_sanitize` içinde `lut.path` için UNC yollarını (`\\`), `..` bileşenlerini ve `.cube` dışı uzantıları reddet; `snapshots` üst sınırını (ör. 50) ve her snapshot'ın sanitizasyonunu ekle; `tone_curve` noktalarını 64 ile sınırla; tüm `ron::from_str` çağrılarını `ron::Options::default().with_recursion_limit(Some(32))` ile değiştir; `preset.rs` ve `i18n.rs`'te `fs::metadata().len()` üst sınırı ekle.

### F-16 [Yüksek] — `custom_suffix` Path Traversal Bypass
- **Konum:** `crates/beam-media/src/export.rs:1261-1271,1208-1229`
- **Düzeltme:** `expand_filename_template` içindeki sanitizasyon mantığını ayrı bir `sanitize_filename_stem(&str) -> String` fonksiyonuna çıkar ve hem `custom_suffix` hem de `generate_output_path` yollarında zorunlu kıl.

### F-17 [Yüksek] — EXIF Çıkarım Tutarsızlığı, Ham Byte Yaması ve Sessiz ICC/EXIF Hataları
- **Konum:** `crates/beam-media/src/export.rs:314-377,952-954,1086-1155`
- **Düzeltme:** `export.rs:345`'te `"Exif\0\0"` (6 byte) önekini atla; RAW dosyalar için TIFF/EXIF başlığını `rawler`/`kamadak-exif` ile çıkar; `sanitize_exif_orientation`'ı IFD0 girişini parse ederek güvenli hale getir; `set_icc_profile` ve `set_exif_metadata` hatalarını `log::warn!` veya `Result` olarak raporla.

### F-18 [Yüksek] — RAW EXIF Metadata, Boyut Çıkarımı ve Gömülü Önizleme Eksikliği
- **Konum:** `crates/beam-media/src/decoder.rs:1005-1008,1037-1043,1562-1592`
- **Düzeltme:** `decode_raw_with_rawler` içinde `extract_exif_metadata(path)` ile ISO/enstantane/diyafram/odak uzaklığını doldur; `extract_image_dimensions` içinde RAW başlık/EXIF boyutlarını oku; düzenlemesiz RAW thumbnail'leri için `rawler` gömülü JPEG preview/thumbnail çıkarıcısını kullan.

### F-19 [Yüksek] — IPTC/XMP ve Etiketlerin Diske Yazılmaması & `created_at` Hatası
- **Konum:** `crates/beam-library/src/lib.rs:325-467,978-1013,1164-1188`
- **Düzeltme:** Standart `.xmp` sidecar (veya `.beam` içinde metadata/tags alanı + opsiyonel `.xmp` dışa aktarım) desteği ekle; `import_file` içinde `created_at` için EXIF `DateTimeOriginal` değerini ISO-8601 formatında sakla.

### F-20 [Yüksek] — 50k+ Kütüphane Ölçeklenme ve `find_duplicates` Darboğazları
- **Konum:** `crates/beam-library/src/lib.rs:365-385,487-549,555-677,821-861,1191-1258`
- **Düzeltme:**
  1. `query_photos`'a `limit`/`offset` (sayfalama) ekle; şemaya `file_ext TEXT`, `content_hash TEXT`, `idx_photos_file_ext`, `idx_photos_file_size`, `idx_photos_filename` ve FTS5 tablosu ekle.
  2. `find_duplicates`'ta N+1 sorguyu tek `JOIN` sorgusuna indir, dosyaları 64 KB streaming tamponla okuyup iki aşamalı (ilk+son 64 KB → tam SHA-256/BLAKE3) hash hesapla ve DB'de önbellekle.
  3. `scan_and_import_folder` içindeki thumbnail üretimini `BEGIN IMMEDIATE` transaction dışına (arka plan kuyruğuna) taşı; `delete_photos`/`relink_folder`'ı tek transaction içine sar.

### F-21 [Yüksek] — 50–100 MP Bellek Tüketimi ve `rayon` Paralellik Eksikliği
- **Konum:** `crates/beam-media/src/decoder.rs:923-981,1158-1170`, `crates/beam-media/src/export.rs:143-275`, `crates/beam-app/src/app.rs:100,1304,1335-1368`
- **Düzeltme:** `beam-media`'ya `rayon` ekle ve `generate_proxy`, `apply_orientation_f32`, `srgb_to_rec2020_linear`, `apply_unsharp_mask`, `straighten` döngülerini `par_chunks_mut` ile paralelleştir. `transform_icc_to_linear_rec2020`'yi chunk bazlı in-place yap, `generate_proxy`'de area/Lanczos filtre kullan, `current_image.data`'yı yalnızca 1:1 veya export sırasında talep üzerine yükle.

### F-22 [Yüksek] — GPU `BindGroup` ve `CommandEncoder` Yükü
- **Konum:** `crates/beam-gpu/src/pipeline.rs:390-403,1485-1785,1896,2169,2433`
- **Düzeltme:** Aşama başına `BindGroup` önbellekle (yalnızca doku değişiminde yenile), `EditStack` alanlarında dirty-tracking ile gereksiz 4×256 spline LUT ve uniform güncellemelerini atla, tüm geçişleri ve readback kopyasını tek `CommandEncoder` / `queue.submit` içinde topla.

### F-23 [Yüksek] — `egui::TextureId` / GPU Sızıntısı ve Thumbnail Cache Thrashing
- **Konum:** `crates/beam-app/src/app.rs:767-789,1305-1307`, `crates/beam-app/src/views/library.rs:190-204`
- **Düzeltme:** `close_project()` ve `upload_proxy_and_render()` içinde mevcut `egui_texture_id`, `egui_before_texture_id`, `egui_full_texture_id` için `render_state.renderer.write().free_texture(&id)` çağır ve `gpu_full_image`/`gpu_full_texture`/`snapshots`/`histogram_data` alanlarını sıfırla. `LibraryViewState::thumbnail_status` önbelleğini gerçek LRU yapısına geçir.

### F-24 [Yüksek] — İlk Histogram, Sürgü Tepkiselliği ve WB Damlalığı Geometri Uyumsuzluğu
- **Konum:** `crates/beam-app/src/app.rs:1240,1294-1298,1451-1477,5953-5990`
- **Düzeltme:** `upload_proxy_and_render`'da `render_with_proof_options` kullan ve histogramı sRGB-kodlu düzenlenmiş çıktıdan hesapla; sürgü sürüklenirken 1:1 tam çözünürlük ve skop yeniden hesabını throttle/debounce et; WB damlalığında `(u, v)` koordinatlarına aktif kırpma/döndürme/çevirme/ufuk düzeltmenin tersini uygula.

### F-25 [Yüksek] — PR CI Eksikliği, Doğrulanmamış `appimagetool` ve Paketleme Hataları
- **Konum:** `.github/workflows/release.yml:77,139`, `packaging/linux/build-linux.sh:21,28-33,42,46`, `packaging/windows/beam-installer.iss:3`, `packaging/windows/build-windows.ps1:66`
- **Düzeltme:** PR ve `master` push'larında `fmt`, `clippy -D warnings`, `test`, `cargo-deny`, `cargo-audit` çalıştıran `.github/workflows/ci.yml` ekle; `release.yml`'de test job'ını yayın ön koşulu yap; `appimagetool` indirmesinde sabit sürüm + SHA256 doğrula; `build-linux.sh`'tan `|| true` ifadelerini kaldır ve portable `.tar.gz`'ye `LICENSE` ekle; `build-windows.ps1`'de `ISCC`'ye `/DMyAppVersion=$Version` geçir; tüm derlemelere `--locked` ekle.

---

## 5. İnceleme Alanlarına Göre Özet Değerlendirme

1. **Renk Bilimi:** Linear Rec.2020 çalışma uzayı fikri doğru, ancak RAW kamera matrisi eksikliği (F-01), yanlış Adobe RGB matrisleri (F-03), raster geniş gam kırpması (F-04), diyagonal WB (F-09), ton eğrisi siyah nokta/kuantizasyon hatası (F-10) ve HSL kenar süreksizliği (F-11) nedeniyle renk doğruluğu şu an üretim seviyesinde değildir.
2. **RAW Desteği:** Saf Rust `rawler` yaklaşımı bağımlılık açısından avantajlıdır; ancak 64 KB doğrulama hatası (F-02), EXIF/boyut/önizleme eksikleri (F-18) ve `develop_intermediate` panic koruması eksikliği (F-28) gerçek dünya RAW dosyalarında ciddi engeller oluşturur. Demosaic algoritması seçimi (AMaZE/RCD/VNG4) ve highlight reconstruction henüz yoktur.
3. **Düzenleme Hattı:** 15 modüler WGSL aşaması ve `.beam` yan dosyası sağlam bir temel sunar; ancak aşama sıralaması (F-14), `detail.wgsl` içindeki `ai_denoise` bypass'ı (F-12), `lens.wgsl` karo hatası (F-06), 10-bit `Rgba16Float` export (F-13) ve fırça maskesi 128-nokta havuz sınırı (F-27) giderilmelidir.
4. **Performans:** 1920px proxy tasarımı yerindedir; ancak UI thread blokajları (F-08), `rayon` eksikliği ve 3.2–4.8 GB pik RAM (F-21), kare başına `BindGroup`/LUT yeniden hesabı (F-22) ve GPU doku sızıntısı (F-23) büyük dosya ve uzun oturumlarda darboğaz yaratır.
5. **Kütüphane:** SQLite WAL ve parametreli sorgular (SQL injection yok) iyi temellerdir; ancak 50k+ ölçekte sayfalama/indeks/FTS5 eksikliği, `find_duplicates` RAM/N+1 sorunu, transaction içi thumbnail üretimi (F-20), diske XMP yazılmaması (F-19) ve Windows/wildcard hataları (F-29) çözülmelidir.
6. **Güvenlik:** Üretim kodunda `unsafe` yoktur ve dosya/piksel/ICC sınırları düşünülmüştür; ancak CLI eklenti yürütmesi (F-07), `lut.path` traversal ve RON özyineleme (F-15), `custom_suffix` traversal (F-16), `fuzz_raw_decode` içinde panic yutulması (F-31) ve `RUSTSEC-2026-0543` (`chrono`) öncelikli güvenlik gündemidir.
7. **Export:** 4 format ve şablon desteği güçlüdür; ancak TIFF16'da straighten/sharpening/watermark/EXIF kaybı (F-05), EXIF önek tutarsızlığı ve sessiz ICC/EXIF hataları (F-17) ile seri batch export (F-30) acil düzeltme gerektirir.
8. **Güvenilirlik:** `beam-media` ve `beam-library`'de 0 `unwrap`/`expect`/`panic!` olması olumludur. `beam-gpu` ve `beam-app`'teki kalan az sayıdaki `unwrap`/`expect` (F-28), sızan GPU dokuları (F-23), Tethered Watch config yazım fırtınası (F-30) ve sürükleme sırası çökme kurtarma eksikliği iyileştirilmelidir.
9. **Kod Kalitesi, Dağıtım ve Profesyonel Eksikler:** 109 testin tamamı geçiyor; ancak golden-image piksel doğrulaması yalnızca `exposure.wgsl` için var, `app.rs` 8.148 satırlık monolit (F-32), PR CI workflow'u yok (F-25) ve `accesskit` kapalı (F-33). Profesyonel RAW editörlere kıyasla başlıca eksik yetenekler: **monitör ICC renk yönetimi**, **leke/klon/iyileştirme (healing/clone) aracı**, **Lensfun otomatik lens profili veritabanı**, **standart `.xmp` yan dosya birlikte çalışabilirliği**, **seçilebilir demosaic & highlight reconstruction**, **özel yazıcı/CMYK ICC ile yumuşak prova ve baskı modülü**, **imleç altı canlı RGB/Lab okuması** ve **gerçek PTP/gPhoto2 kablolu çekim / HDR-panorama birleştirme**.

---

## 6. Geliştirme Yol Haritası ve Öncelikli Düzeltme Planı

### Kısa Vadeli (Acil — 1–2 Hafta, v1.3.0 Hedefi)
- **RAW & Renk Acil Düzeltmeleri:** F-02 (`validate_raw_container` `total_file_size`), F-01 (RAW WB + kamera matrisi → Linear Rec.2020), F-03 (`REC2020_TO_ADOBE_RGB` eşitleme), F-04 (`moxcms` doğrudan Linear Rec.2020), F-10 (`tone_curve` siyah nokta), F-11 (HSL bant kenarı).
- **Güvenlik & Veri Bütünlüğü:** F-07 (CLI eklenti güvenlik modeli + timeout + Windows injection fix), F-15 (`lut.path` + `snapshots` + RON limit), F-16 (`custom_suffix` sanitizasyonu), F-31 (`fuzz_raw_decode` `catch_unwind` kaldırma + `deny.toml` temizliği).
- **Export & CI Acil Düzeltmeleri:** F-05 (TIFF16 tam özellik eşliği), F-13 (`Rgba32Float` gerçek 16-bit export), F-17 (EXIF öneki & hata raporlama), F-25 (`ci.yml` + `LICENSE` in `.tar.gz` + Windows `/DMyAppVersion` + `appimagetool` SHA256).

### Orta Vadeli (3–6 Hafta, v1.4.0 Hedefi)
- **GPU & Düzenleme Hattı:** F-06 (`lens.wgsl` dinamik karo kaynak kutusu), F-12 (`denoise`/`detail` aşama ayrımı), F-14 (doğru pipeline sırası + kırpma sonrası vignette), F-22 (`BindGroup` önbelleği + tek `CommandEncoder` + dirty tracking), F-23 (`free_texture` + LRU thumbnail cache), F-24 (sRGB skop + debounce + WB damlalık geometrisi), F-27 (`R8Unorm` fırça maske dokusu).
- **Performans & Kütüphane:** F-08 (UI thread blokajlarının arka plana taşınması), F-18 (RAW EXIF/boyut/gömülü önizleme), F-20 (SQLite sayfalama, indeksler, FTS5, streaming SHA-256 duplicate, transaction dışı thumbnail), F-21 (`rayon` paralelleştirme + in-place ICC + 100 MP RAM azaltımı), F-29 (kütüphane `RETURNING id`, Windows yol filtresi, relink/temizlik), F-30 (`notify` tethered watch + paralel batch export).
- **Kalite & Erişilebilirlik:** F-32 (`app.rs` modüler bölünmesi + clippy temizliği), F-33 (`accesskit`, `CARGO_PKG_VERSION`, eksik çeviriler), tüm 15 shader için sayısal golden-image testleri.

### Uzun Vadeli (2–4 Ay, v2.0.0 Hedefi)
- **Renk & Optik Profesyonelleşme:** F-09 (Bradford CAT), monitör ICC profil desteği, özel yazıcı/kağıt ICC ile soft proofing, Lensfun veritabanı entegrasyonu, seçilebilir demosaic ve highlight reconstruction.
- **Katalog & Yaratıcı Araçlar:** F-19 (tam `.xmp` sidecar birlikte çalışabilirliği), Healing/Clone spot removal aracı, imleç altı RGB/Lab piksel okuyucu, gerçek gPhoto2/PTP tethered shooting, HDR merge / panorama birleştirme.

---

## 7. Sonraki Düzeltme Aşamaları İçin Önerilen İş Bölümü (Prompt Planı)

Bu raporu tek bir dev değişiklik yerine **6 bağımsız, test edilebilir faz** halinde uygulamanız önerilir:

1. **Faz 1 — RAW & Renk Bilimi Doğruluğu (F-01, F-02, F-03, F-04, F-09, F-10, F-11, F-26):**
   - `beam-media/src/decoder.rs`, `beam-core/src/color.rs`, `shaders/{white_balance,tone_curve,hsl,color_grading,vibrance_saturation,display_transform,export_transform}.wgsl` + golden-image ve matris testleri.
2. **Faz 2 — Güvenlik Sertleştirme ve CI/Paketleme (F-07, F-15, F-16, F-25, F-31, F-34):**
   - `beam-core/src/{plugin,sidecar,preset,i18n,lut}.rs`, `beam-media/src/export.rs`, `fuzz/`, `deny.toml`, `.github/workflows/{ci,release}.yml`, `packaging/`.
3. **Faz 3 — GPU Pipeline, Tiled Lens ve 16-Bit Export (F-05, F-06, F-12, F-13, F-14, F-17, F-22, F-27):**
   - `beam-gpu/src/pipeline.rs`, `shaders/{detail,lens,mask_adjust,vignette_grain,export_transform}.wgsl`, `beam-media/src/export.rs` + tiled-vs-monolithic lens ve gerçek 16-bit testleri.
4. **Faz 4 — Performans, Bellek ve UI Thread Temizliği (F-08, F-18, F-21, F-23, F-24, F-30):**
   - `rayon` entegrasyonu (`beam-media`), `beam-app/src/{app.rs,views/library.rs,widgets/histogram.rs}` asenkron IO/DB/eklenti/segmentasyon işçileri, `free_texture`, LRU önbellek, `notify` tethered watch.
5. **Faz 5 — Kütüphane Ölçeklenebilirliği ve XMP (F-19, F-20, F-29):**
   - `beam-library/src/lib.rs` yeni şema migrasyonu (indeksler, FTS5, `file_ext`, `content_hash`), sayfalama, streaming duplicate hash, EXIF `DateTimeOriginal`, `.xmp` senkronizasyonu.
6. **Faz 6 — `app.rs` Modülerleştirme, Erişilebilirlik ve Yerelleştirme (F-28, F-32, F-33):**
   - `app.rs`'in alt modüllere bölünmesi, üretim `unwrap`/`expect` temizliği, `accesskit` etkinleştirme, `CARGO_PKG_VERSION` ve eksik `.ftl` anahtarlarının tamamlanması.
