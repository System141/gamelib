// All user-facing text. Game data (names, English descriptions) comes from Steam as-is.

import { formatNumber, formatRelative } from "../lib/format";
import type { CheckStatus, CmdError, DeckCompat, SortKey, SyncPhase } from "../lib/types";

const n = formatNumber;

export const tr = {
  app: {
    name: "GameLib",
    tagline: "Steam oyun kataloğun",
  },
  views: {
    all: "Tüm Oyunlar",
    new: "Yeni Çıkanlar",
    links: "Bağlantılarım",
  },
  search: {
    placeholder: "Oyun ara…",
    clear: "Aramayı temizle",
  },
  sort: {
    label: "Sırala",
    relevance: "En alakalı",
    popular: "En popüler",
    rating: "En yüksek puan",
    newest: "En yeni",
    oldest: "En eski",
    name: "İsim (A–Z)",
  } satisfies Record<SortKey | "label", string>,
  count: {
    games: (count: number) => `${n(count)} oyun`,
    results: (count: number) => `${n(count)} sonuç`,
  },
  filters: {
    button: "Filtreler",
    title: "Filtreler",
    tags: "Etiketler",
    tagSearch: "Etiket ara…",
    showAllTags: (count: number) => `Tüm etiketleri göster (${n(count)})`,
    showFewerTags: "Daha az göster",
    noTags: "Eşleşen etiket yok",
    platforms: "Platform",
    deck: "Steam Deck",
    deckAny: "Tümü",
    deckPlayable: "Oynanabilir+",
    deckVerified: "Doğrulanmış",
    reviews: "Kullanıcı incelemeleri",
    reviewsAny: "Tümü",
    reviewsMostlyPositive: "Çoğunlukla Olumlu+",
    reviewsVeryPositive: "Çok Olumlu+",
    reviewsOverwhelming: "Son Derece Olumlu",
    other: "Diğer",
    freeOnly: "Yalnızca ücretsiz oyunlar",
    hasLinks: "Yalnızca Steam dışı bağlantısı olanlar",
    showAdult: "Yetişkin içeriği göster (+18)",
    showAdultHint: "Steam'de olduğu gibi varsayılan olarak gizlidir.",
    clear: "Temizle",
    clearAll: "Tümünü temizle",
    close: "Kapat",
    apply: "Sonuçları göster",
    removeFilter: (label: string) => `${label} filtresini kaldır`,
    minReview: (label: string) => `${label} ve üstü`,
  },
  platforms: {
    win: "Windows",
    mac: "macOS",
    linux: "Linux",
  },
  card: {
    new: "YENİ",
    free: "Ücretsiz",
    earlyAccess: "Erken Erişim",
    links: (count: number) => `${count} bağlantı`,
  },
  detail: {
    close: "Kapat",
    openInSteam: "Steam'de Aç",
    openInClient: "Steam uygulamasında aç",
    about: "Hakkında",
    englishDescription: "Türkçe açıklama yok; Steam'deki İngilizce metin gösteriliyor.",
    noDescription: "Bu oyun için kısa açıklama bulunmuyor.",
    screenshots: "Ekran görüntüleri",
    mediaError: "Ekran görüntüleri yüklenemedi.",
    noScreenshots: "Ekran görüntüsü yok.",
    info: "Bilgiler",
    releaseDate: "Çıkış tarihi",
    developer: "Geliştirici",
    publisher: "Yayıncı",
    franchise: "Seri",
    platforms: "Platformlar",
    deck: "Steam Deck",
    reviews: "Kullanıcı incelemeleri",
    reviewCount: (count: number) => `${n(count)} inceleme`,
    price: "Fiyat",
    free: "Ücretsiz",
    noPrice: "Satışta değil",
    priceNote: "Türkiye mağazası fiyatı (USD), son güncelleme itibarıyla.",
    tags: "Etiketler",
    tagHint: "Etikete tıklayarak listeyi filtreleyebilirsin.",
    delisted: "Bu oyun artık Steam mağazasında listelenmiyor.",
    lastUpdated: (rel: string) => `Veriler ${rel} güncellendi`,
    earlyAccess: "Erken Erişim",
    new: "Yeni çıktı",
    viewerPrev: "Önceki görsel",
    viewerNext: "Sonraki görsel",
    viewerCounter: (i: number, total: number) => `${i} / ${total}`,
    loadError: "Oyun bilgileri yüklenemedi.",
  },
  links: {
    title: "Steam dışı bağlantılar",
    hint: "Başka mağazalardaki sayfaları ya da indirme bağlantılarını buraya ekleyebilirsin. Bağlantılar varsayılan tarayıcında açılır; GameLib dosya indirmez.",
    empty: "Bu oyun için henüz bağlantı eklenmedi.",
    add: "Bağlantı ekle",
    open: "Aç",
    check: "Kontrol et",
    checking: "Kontrol ediliyor…",
    edit: "Düzenle",
    delete: "Sil",
    confirmDelete: "Bu bağlantı silinsin mi?",
    confirmYes: "Evet, sil",
    confirmNo: "Vazgeç",
    insecure: "Şifresiz (http)",
    notChecked: "Henüz kontrol edilmedi",
    checkedAgo: (rel: string) => `kontrol: ${rel}`,
    redirects: (count: number) => (count === 0 ? "yönlendirme yok" : `${count} yönlendirme`),
    webPage: "Web sayfası",
    file: "Dosya",
    kinds: { download: "İndirme", page: "Sayfa" },
    form: {
      titleNew: "Yeni bağlantı",
      titleEdit: "Bağlantıyı düzenle",
      url: "Bağlantı (URL)",
      urlPlaceholder: "https://…",
      detected: (site: string, host: string) => `Algılanan site: ${site} · ${host}`,
      insecureWarning: "Bu bağlantı şifrelenmemiş (http). Mümkünse https kullan.",
      label: "Etiket",
      labelPlaceholder: "Örn. Windows kurulumu",
      kind: "Tür",
      platform: "Platform",
      platformAny: "Tümü",
      version: "Sürüm",
      versionPlaceholder: "Örn. 1.2.0",
      notes: "Not",
      notesPlaceholder: "İsteğe bağlı",
      save: "Kaydet",
      saving: "Kaydediliyor…",
      cancel: "Vazgeç",
    },
    sites: { generic: "Diğer site" } as Record<string, string>,
    toastSaved: "Bağlantı kaydedildi",
    toastDeleted: "Bağlantı silindi",
  },
  checkStatus: {
    ok: "Çalışıyor",
    broken: "Bozuk",
    loop: "Yönlendirme döngüsü",
    too_many_redirects: "Çok fazla yönlendirme",
    timeout: "Zaman aşımı",
    network: "Ulaşılamadı",
    tls: "Güvenli bağlantı kurulamadı",
    unsupported_scheme: "Bir uygulamaya yönlendiriyor",
  } satisfies Record<CheckStatus, string>,
  firstRun: {
    eyebrow: "Hoş geldin",
    title: "Steam kataloğunu indir",
    subtitle:
      "GameLib, Steam'deki tüm çıkmış oyunları kapak görselleriyle birlikte bilgisayarına kaydeder. Sonrasında hepsinde anında arama yapabilir, filtreleyebilir ve keşfedebilirsin.",
    fullTitle: "Tüm katalog",
    fullDesc: "~130.000 oyun · ~80 MB indirme · 5–8 dakika",
    fullCta: "Kataloğu indir",
    newTitle: "Yalnızca yeni çıkanlar",
    newDesc: "Son 30 günde çıkan oyunlar · birkaç saniye",
    newCta: "Yeni çıkanları getir",
    note: "Veriler Steam'in herkese açık mağaza servisinden alınır; API anahtarı gerekmez. Görseller Steam'in sunucularından gösterilir.",
    legal: "GameLib, Valve Corporation ile bağlantılı değildir. Steam, Valve Corporation'ın ticari markasıdır.",
    progress: (fetched: number, total: number) => `${n(fetched)} / ${n(total)} oyun`,
    eta: (text: string) => `Tahmini kalan süre: ${text}`,
    cancel: "Durdur",
  },
  sync: {
    update: "Güncelle",
    newReleases: "Yeni çıkanları getir",
    newReleasesHint: (at: number | null) => (at ? `Son kontrol: ${formatRelative(at)}` : "Henüz kontrol edilmedi"),
    fullSync: "Tüm kataloğu güncelle",
    fullSyncHint: (at: number | null) => (at ? `Son tam güncelleme: ${formatRelative(at)}` : "Henüz tam güncelleme yapılmadı"),
    cancel: "Durdur",
    resumable: "Önceki katalog indirmesi yarıda kaldı.",
    resume: "Devam et",
    stale: (at: number) => `Katalog ${formatRelative(at)} güncellendi.`,
    refresh: "Güncelle",
    phases: {
      starting: "Hazırlanıyor…",
      tags: "Etiketler alınıyor…",
      featured: "Öne çıkan oyunlar alınıyor…",
      catalog: "Katalog indiriliyor…",
      new_releases: "Yeni çıkanlar alınıyor…",
      finalizing: "Son rötuşlar yapılıyor…",
    } satisfies Record<SyncPhase, string>,
    newProgress: (fetched: number) => `${n(fetched)} oyun işlendi`,
    toastFull: (games: number) => `Katalog güncellendi · ${n(games)} oyun`,
    toastFullNew: (inserted: number) => `${n(inserted)} oyun ilk kez eklendi`,
    toastNew: (inserted: number) => (inserted > 0 ? `${n(inserted)} yeni oyun eklendi` : "Yeni oyun yok, liste güncel"),
    toastNewDetail: (fetched: number) => `${n(fetched)} yeni çıkan oyun kontrol edildi`,
    toastPartial: "Aradan uzun süre geçmiş. Eksik kalmaması için tüm kataloğu güncellemeni öneririz.",
    toastCancelled: "Güncelleme durduruldu",
    toastCancelledDetail: "Katalog indirmesine daha sonra kaldığı yerden devam edebilirsin.",
    toastFailed: "Güncelleme tamamlanamadı",
  },
  newView: {
    title: "Yeni Çıkanlar",
    subtitle: (count: number, days: number) => `Son ${days} günde ${n(count)} oyun çıktı`,
    period: (days: number) => `${days} gün`,
  },
  linksView: {
    title: "Bağlantılarım",
    subtitle: "Steam dışı bağlantı eklediğin oyunlar",
    emptyTitle: "Henüz bağlantı eklemedin",
    emptyText: "Bir oyunun detayını açıp «Bağlantı ekle» ile başlayabilirsin.",
  },
  empty: {
    title: "Sonuç bulunamadı",
    text: "Aramayı ya da filtreleri değiştirmeyi dene.",
    clear: "Filtreleri temizle",
  },
  grid: {
    label: "Oyunlar",
    backToTop: "Başa dön",
  },
  error: {
    title: "Bir şeyler ters gitti",
    reload: "Yeniden yükle",
  },
};

const REVIEW_LABELS = [
  "İnceleme yok",
  "Son Derece Olumsuz",
  "Çok Olumsuz",
  "Olumsuz",
  "Çoğunlukla Olumsuz",
  "Karışık",
  "Çoğunlukla Olumlu",
  "Olumlu",
  "Çok Olumlu",
  "Son Derece Olumlu",
];

/** Steam's review summary in Turkish; games with a handful of reviews get no score. */
export function reviewLabel(score: number, count: number): string {
  if (score <= 0) {
    return count > 0 ? `${n(count)} kullanıcı incelemesi` : REVIEW_LABELS[0]!;
  }
  return REVIEW_LABELS[Math.min(9, score)]!;
}

const DECK_LABELS: Record<DeckCompat, string> = {
  0: "Bilinmiyor",
  1: "Desteklenmiyor",
  2: "Oynanabilir",
  3: "Doğrulanmış",
};

export function deckLabel(deck: DeckCompat): string {
  return DECK_LABELS[deck] ?? DECK_LABELS[0];
}

const INVALID_CODES: Record<string, string> = {
  url_empty: "Bir bağlantı yapıştır.",
  url_too_long: "Bağlantı çok uzun (en fazla 2048 karakter).",
  url_parse: "Bu geçerli bir bağlantı değil.",
  url_scheme: "Yalnızca http:// ve https:// ile başlayan bağlantılar eklenebilir.",
  url_host: "Bağlantıda bir alan adı yok.",
  url_credentials: "Kullanıcı adı ya da şifre içeren bağlantılar eklenemez.",
  label_too_long: "Etiket en fazla 120 karakter olabilir.",
  version_too_long: "Sürüm en fazla 60 karakter olabilir.",
  notes_too_long: "Not en fazla 1000 karakter olabilir.",
};

const ERROR_KINDS: Record<CmdError["kind"], string> = {
  network: "Steam'e bağlanılamadı. İnternet bağlantını kontrol et.",
  timeout: "Sunucu zamanında yanıt vermedi.",
  rate_limited: "Steam şu an çok fazla istek alıyor; biraz sonra tekrar dene.",
  http: "Sunucu beklenmeyen bir yanıt verdi.",
  parse: "Steam'in yanıtı okunamadı.",
  database: "Yerel veritabanında bir sorun oluştu.",
  cancelled: "İşlem durduruldu.",
  invalid: "Girilen bilgi geçersiz.",
  not_found: "Kayıt bulunamadı.",
  busy: "Zaten bir güncelleme sürüyor.",
  other: "Beklenmeyen bir hata oluştu.",
};

export function errorText(e: CmdError): string {
  if (e.kind === "invalid") {
    return INVALID_CODES[e.message] ?? ERROR_KINDS.invalid;
  }
  return ERROR_KINDS[e.kind] ?? ERROR_KINDS.other;
}
