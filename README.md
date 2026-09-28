# GameLib

Steam'deki **tüm çıkmış oyunları** kapak görselleriyle birlikte bilgisayarına indiren ve şık, hızlı bir arayüzde listeleyen masaüstü uygulaması. Windows, macOS ve Linux'ta çalışır ([Tauri v2](https://tauri.app) + React).

![Oyun ızgarası](docs/screenshots/grid.jpg)

| Detay penceresi | Yeni çıkanlar |
| --- | --- |
| ![Detay](docs/screenshots/detail.jpg) | ![Yeni çıkanlar](docs/screenshots/new-releases.jpg) |

## Özellikler

- **Tüm katalog:** Steam'deki ~130.600 çıkmış oyun, dikey kapak görselleriyle. Katalog yerel veritabanında durduğu için arama ve filtreler anında çalışır.
- **Arama ve filtreler:**
  - Türkçe karakter ve noktalama duyarsız arama. Örneğin "baldurs" yazınca *Baldur's Gate 3*, "stalker" yazınca *S.T.A.L.K.E.R.* bulunur.
  - Türkçe etiketler, platform, Steam Deck uyumu, inceleme puanı ve "yalnızca ücretsiz" filtreleri.
  - Altı sıralama seçeneği.
- **Yeni çıkanlar (istendiğinde):** "Yeni çıkanları getir", yalnızca son kontrolden bu yana çıkan oyunları çeker; bu genellikle tek istek ve birkaç saniye sürer. Ayrı sekmede son 7, 30 ya da 90 günün oyunları listelenir.
- **Detay penceresi:**
  - Büyük görsel, varsa Türkçe açıklama ve büyütülebilir ekran görüntüleri.
  - Fiyat ve indirim, geliştirici, yayıncı, platformlar, Steam Deck durumu.
  - Etiketler; tıklanınca filtreye eklenir.
  - "Steam'de Aç" ve "Steam uygulamasında aç" düğmeleri.
- **Steam dışı bağlantılar (altyapı ve temel arayüz):**
  - Her oyuna elle bağlantı eklenebilir, düzenlenebilir ve silinebilir.
  - "Kontrol et" bağlantının yönlendirmelerini izler ve son adresi, dosya adını, türünü ve boyutunu gösterir; dosyayı indirmez.
  - Bağlantılar varsayılan tarayıcıda açılır.
  - Her site için ayrı bir işleyici yazılabilir (bkz. [Yeni site işleyicisi ekleme](#yeni-site-işleyicisi-ekleme)).
- **Yetişkin içerik** Steam'de olduğu gibi varsayılan olarak gizlidir; filtrelerden açılabilir.

![Steam dışı bağlantılar](docs/screenshots/links.jpg)

## Nasıl çalışır

- **Veri kaynağı:** Valve eski `ISteamApps/GetAppList` servisini kaldırdı, yenisi ise API anahtarı istiyor. GameLib bunun yerine Steam mağazasının kendi kullandığı anahtarsız servisleri kullanır:
  - `IStoreQueryService/Query`: oyun listesi, sayfa başına 1000 oyun.
  - `IStoreService/GetTagList`: Türkçe etiket adları.
  - `IStoreBrowseService/GetItems`: detay penceresindeki Türkçe açıklama ve ekran görüntüleri.
- **Bölge ve dil:**
  - Bölge Türkiye'dir, fiyatlar Steam'in Türkiye için belirlediği USD fiyatlarıdır.
  - Açıklamalar İngilizce çekilir, çünkü çoğu oyunun Türkçe açıklaması yoktur. Türkçesi olanlar detay penceresinde Türkçe gösterilir.
- **Depolama:** Katalog yerel bir SQLite veritabanında (WAL, FTS5 arama) tutulur ve yaklaşık 130 MB yer kaplar. Görseller indirilmez, Steam'in sunucularından gösterilir.
- **Tam güncelleme:**
  - ~130 istekte yapılır ve 4–8 dakika sürer.
  - Önce en çok satanlar gelir, böylece ızgara birkaç saniyede dolar.
  - Yarıda kalırsa kaldığı yerden devam eder.
- **Güvenli güncelleme:** Mağazadan kalkan oyunlar silinmez, yalnızca gizlenir. Eklediğin bağlantılar hiçbir güncellemeden etkilenmez.

## Gereksinimler

- Node.js 22.12+ ve pnpm 10
- Rust 1.90+
- İşletim sistemine göre:
  - **Linux (Debian/Ubuntu):**
    `sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev libxdo-dev build-essential pkg-config`.
    WebKitGTK 2.40 veya üstü gerekir.
  - **Windows:** Microsoft C++ Build Tools ve WebView2. WebView2 Windows 10/11'de hazır gelir.
  - **macOS:** Xcode Command Line Tools ve macOS 13.3 veya üstü.

## Geliştirme

```bash
pnpm install
pnpm tauri dev          # uygulamayı geliştirme modunda açar
```

**Tarayıcı önizlemesi:** `pnpm dev` komutundan sonra http://localhost:1420 adresini aç.

- Tauri olmadan, gerçek katalogdan alınmış 272 oyunluk örnek veriyle çalışır.
- `?mock=empty` ile ilk açılış ekranı görülebilir.
- Örnek veriyi yenilemek için önce CLI ile bir katalog indir, sonra `pnpm fixture` çalıştır.

Testler ve kontroller:

```bash
cargo test                                  # çekirdek (GTK gerektirmez)
cargo test -p gamelib-core -- --ignored     # gerçek Steam API'siyle canlı test
pnpm test                                   # arayüz yardımcıları
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

## Derleme

```bash
pnpm tauri build        # işletim sistemine uygun kurulum paketlerini üretir
```

## Komut satırı aracı

`gamelib-cli`, katalogla arayüz olmadan çalışmayı sağlar:

```bash
cargo run --release -p gamelib-core --bin gamelib-cli -- --db gamelib.db sync
cargo run --release -p gamelib-core --bin gamelib-cli -- --db gamelib.db new-releases
cargo run --release -p gamelib-core --bin gamelib-cli -- --db gamelib.db query --search "witcher" --limit 5
cargo run --release -p gamelib-core --bin gamelib-cli -- --db gamelib.db stats
cargo run --release -p gamelib-core --bin gamelib-cli -- check-link https://ornek.com/dosya.zip
```

Tüm komutlar için `help` alt komutuna bak. Uygulamanın kendi veritabanını kullanmak için `--db` ile aşağıdaki yolu ver.

## Veri konumu

| Sistem | Veritabanı |
| --- | --- |
| Windows | `%LOCALAPPDATA%\com.gamelib.desktop\gamelib.db` |
| macOS | `~/Library/Application Support/com.gamelib.desktop/gamelib.db` |
| Linux | `~/.local/share/com.gamelib.desktop/gamelib.db` |

Kataloğu sıfırlamak için uygulama kapalıyken bu dosyayı silmen yeterli. Dosya silinince eklediğin bağlantılar da silinir.

## Proje yapısı

```
crates/gamelib-core/   Tauri'den bağımsız çekirdek: Steam istemcisi, SQLite, senkron, bağlantılar, CLI
  src/steam/           Steam servisleri ve görsel adresleri
  src/db/              şema, okuma/yazma, bağlantı kayıtları
  src/sync.rs          tam katalog indirme
  src/new_releases.rs  yeni çıkanları getirme
  src/links/           URL doğrulama, site işleyicileri, yönlendirme kontrolü
src-tauri/             masaüstü kabuğu: komutlar, olaylar, pencere ve güvenlik ayarları
src/                   React arayüzü (tüm metinler src/i18n/tr.ts içinde)
  mocks/               yalnızca tarayıcı önizlemesi için sahte arka uç
```

## Yeni site işleyicisi ekleme

Her indirme sitesinin bağlantı yapısı ve yönlendirmeleri farklıdır. Bu yüzden siteye özel davranış, `crates/gamelib-core/src/links/sites/` altındaki işleyicilerde yazılır. Tanınmayan siteler genel işleyiciyle çalışır. Genel işleyici yalnızca standart HTTP yönlendirmelerini izler; JavaScript, captcha ya da bekleme sayfası gerektiren akışlar otomatikleştirilmez.

1. `sites/<site>.rs` dosyasında `SiteHandler` uygulayan bir tür oluştur:
   - `info()` sabit bir `id` (bağlantılarla birlikte saklanır, değiştirme), görünen ad, alan adları ve rozet rengi döndürür.
   - `normalize()` yapıştırılan adresi düzenler; isteğe bağlıdır.
   - `resolve()` sitenin özel yönlendirme adımlarını uygular; isteğe bağlıdır.
2. `sites/mod.rs` içindeki `builtin()` listesine ekle.
3. Sitenin gerçek adres örnekleriyle bir test yaz.

Örnek iskelet `sites/mod.rs` dosyasının başındaki açıklamada yer alıyor.

## Sorun giderme

- **Linux'ta boş ya da siyah pencere (bazı NVIDIA/Wayland kurulumları):** `WEBKIT_DISABLE_DMABUF_RENDERER=1 gamelib` ile başlat.
- **Kurumsal proxy:** `HTTPS_PROXY` ortam değişkeni ve sistem sertifika deposu kullanılır.
- **Steam yanıt vermiyorsa:** İstekler otomatik olarak birkaç kez yeniden denenir. Yarıda kalan indirme bir sonraki "Güncelle"de devam eder.

## Yol haritası

- Siteye özel bağlantı işleyicileri
- Bağlantıları dışa ve içe aktarma (yedekleme)
- Bilgisayardaki Steam kütüphanesini okuma
- Valve anahtarsız servisi kapatırsa API anahtarıyla çalışan yedek kaynak
- Favoriler, yerel görsel önbelleği, tek oyun güncelleme

## Yasal uyarı

GameLib, Valve Corporation ile bağlantılı değildir. Steam ve ilgili logolar Valve Corporation'ın ticari markalarıdır. Oyun adları, açıklamaları ve görselleri sahiplerine aittir; Steam'in herkese açık mağaza servislerinden gösterilir.
