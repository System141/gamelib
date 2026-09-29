import { describe, expect, it } from "vitest";
import { fold, normalizeName } from "./fold";
import { comparableModel, listVerdict, vendorOf, worst } from "./requirements";
import { formatMoney, priceSteps, priceVerdict } from "./prices";
import {
  fileKind,
  formatBytes,
  formatDate,
  formatDuration,
  formatNumber,
  formatPercent,
  formatRelative,
  isRecent,
  reviewTone,
} from "./format";
import { CAPTION_HEIGHT, MIN_CARD_WIDTH, computeGridLayout } from "./grid";
import { reviewLabel, deckLabel, errorText } from "../i18n/tr";

describe("format", () => {
  it("formats numbers and percents the Turkish way", () => {
    expect(formatNumber(130615)).toBe("130.615");
    expect(formatPercent(96)).toBe("%96");
  });

  it("formats dates", () => {
    expect(formatDate(973_065_600)).toMatch(/2000/);
    expect(formatDate(973_065_600)).toMatch(/Kasım/);
    expect(formatDate(null)).toBe("—");
  });

  it("formats relative times", () => {
    const now = 1_790_000_000;
    expect(formatRelative(now - 10, now)).toBe("az önce");
    expect(formatRelative(now - 3 * 3600, now)).toBe("3 saat önce");
    expect(formatRelative(now - 86_400, now)).toBe("dün");
    expect(formatRelative(now - 3 * 86_400, now)).toBe("3 gün önce");
    expect(formatRelative(null, now)).toBe("—");
  });

  it("formats durations and sizes", () => {
    expect(formatDuration(45)).toBe("45 sn");
    expect(formatDuration(200)).toBe("3 dk 20 sn");
    expect(formatDuration(3900)).toBe("1 sa 5 dk");
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1_288_490_189)).toBe("1,2 GB");
    expect(formatBytes(null)).toBe("—");
  });

  it("classifies reviews like Steam", () => {
    expect(reviewTone(9)).toBe("pos");
    expect(reviewTone(6)).toBe("pos");
    expect(reviewTone(5)).toBe("mixed");
    expect(reviewTone(2)).toBe("neg");
    expect(reviewTone(0)).toBe("none");
  });

  it("detects recent releases", () => {
    const now = 1_790_000_000;
    expect(isRecent(now - 2 * 86_400, 7, now)).toBe(true);
    expect(isRecent(now - 8 * 86_400, 7, now)).toBe(false);
    expect(isRecent(null, 7, now)).toBe(false);
  });

  it("derives short file kinds", () => {
    expect(fileKind("application/zip", "game.zip")).toBe("ZIP");
    expect(fileKind("application/x-msdownload", "setup.exe")).toBe("EXE");
    expect(fileKind("application/x-gzip", null)).toBe("GZIP");
    expect(fileKind(null, null)).toBe(null);
  });
});

describe("fold", () => {
  it("ignores case, accents and Turkish i forms", () => {
    expect(fold("Bağımsız")).toBe("bagimsiz");
    expect(fold("İstanbul")).toBe(fold("istanbul"));
    expect(fold("IŞIK")).toBe("isik");
    expect(fold("Çok Oyunculu")).toBe("cok oyunculu");
  });

  it("normalizes names like the Rust core", () => {
    expect(normalizeName("Baldur's Gate 3")).toBe("baldurs gate 3");
    expect(normalizeName("S.T.A.L.K.E.R.: Shadow of Chernobyl")).toBe("stalker shadow of chernobyl");
    expect(normalizeName("  Half-Life   2 ")).toBe("half life 2");
    expect(normalizeName("İSTANBUL Kıyamet")).toBe("istanbul kiyamet");
  });
});

describe("grid", () => {
  it("fits columns into the width", () => {
    for (const width of [800, 1024, 1280, 1440, 1920, 2560]) {
      const g = computeGridLayout(width);
      expect(g.cols).toBeGreaterThanOrEqual(2);
      expect(g.cardWidth).toBeGreaterThanOrEqual(MIN_CARD_WIDTH);
      const used = g.cols * g.cardWidth + (g.cols - 1) * g.gap + 2 * g.padding;
      expect(Math.abs(used - width)).toBeLessThan(1);
      expect(g.rowHeight).toBeGreaterThan(g.cardWidth * 1.5 + CAPTION_HEIGHT);
    }
  });

  it("adds columns as the window grows", () => {
    expect(computeGridLayout(1000).cols).toBe(4);
    expect(computeGridLayout(1024).cols).toBe(5);
    expect(computeGridLayout(1440).cols).toBe(6);
    expect(computeGridLayout(1920).cols).toBeGreaterThan(computeGridLayout(1440).cols);
  });
});

describe("i18n", () => {
  it("labels reviews and deck status in Turkish", () => {
    expect(reviewLabel(9, 1000)).toBe("Son Derece Olumlu");
    expect(reviewLabel(5, 1000)).toBe("Karışık");
    expect(reviewLabel(0, 0)).toBe("İnceleme yok");
    expect(reviewLabel(0, 4)).toBe("4 kullanıcı incelemesi");
    expect(deckLabel(3)).toBe("Doğrulanmış");
  });

  it("explains errors", () => {
    expect(errorText({ kind: "invalid", message: "url_scheme" })).toMatch(/https/);
    expect(errorText({ kind: "network", message: "x" })).toMatch(/bağlan/i);
    expect(errorText({ kind: "invalid", message: "unknown_code" })).toBeTruthy();
  });
});

describe("errorText", () => {
  it("explains coded failures with their detail", async () => {
    const { errorText } = await import("../i18n/tr");
    expect(errorText({ kind: "invalid", message: "installer_failed:4" })).toBe("Kurulum programı hata verdi (çıkış kodu 4).");
    expect(errorText({ kind: "invalid", message: "archive_corrupt" })).toBe("Arşiv bozuk; yeniden indirmeyi dene.");
    expect(errorText({ kind: "invalid", message: "unknown_code:1" })).toBe("Girilen bilgi geçersiz.");
    expect(errorText({ kind: "network", message: "x" })).toContain("İnternet");
  });
});

describe("requirements", () => {
  const check = (verdict: "ok" | "short" | "unknown", kind: "memory" | "storage" = "memory") => ({ kind, need: 1, have: 1, verdict });

  it("finds the worst verdict of a list", () => {
    expect(worst([])).toBeNull();
    expect(worst([check("ok"), check("unknown")])).toBe("unknown");
    expect(worst([check("unknown"), check("short")])).toBe("short");
    expect(listVerdict({ lines: [], checks: [] })).toEqual({ kind: "none" });
    expect(listVerdict({ lines: [], checks: [check("ok"), check("short", "storage")] })).toEqual({ kind: "short", missing: ["storage"] });
    expect(listVerdict({ lines: [], checks: [check("ok"), check("unknown")] })).toEqual({ kind: "partial" });
    expect(listVerdict({ lines: [], checks: [check("ok")] })).toEqual({ kind: "ok" });
  });

  it("tells makers apart", () => {
    expect(vendorOf("NVIDIA GeForce RTX 3060")).toBe("nvidia");
    expect(vendorOf("AMD Radeon RX 6600 XT")).toBe("amd");
    expect(vendorOf("Intel(R) Core(TM) i7-8700K CPU @ 3.70GHz")).toBe("intel");
    expect(vendorOf("AMD Ryzen 5 3600X 6-Core Processor")).toBe("amd");
    expect(vendorOf("Intel Arc A580")).toBe("intel");
    expect(vendorOf("Qualcomm Adreno X1")).toBe("qualcomm");
    expect(vendorOf("Dual Core 3.0 Ghz")).toBeNull();
  });

  it("picks the model to compare with", () => {
    const gpu = "GeForce GTX 1660 / Radeon RX 5500 XT 8GB / Arc A580";
    expect(comparableModel(gpu, "AMD Radeon RX 6600")).toBe("Radeon RX 5500 XT");
    expect(comparableModel(gpu, "NVIDIA GeForce RTX 3060")).toBe("GeForce GTX 1660");
    expect(comparableModel(gpu, null)).toBe("GeForce GTX 1660");
    expect(
      comparableModel("Nvidia GTX 970 / RX 480 / Intel Arc A380 / Qualcomm Adreno X1 (4GB+ of VRAM)", "Intel(R) Arc(TM) A770 Graphics"),
    ).toBe("Intel Arc A380");
    expect(comparableModel("Core i5-8400 / Ryzen 5 2600", "AMD Ryzen 7 5800X3D")).toBe("Ryzen 5 2600");
    expect(comparableModel("2.0 Ghz", "Intel Core i5")).toBeNull();
    expect(comparableModel("Dual Core 3.0 Ghz", null)).toBeNull();
    expect(comparableModel("128mb Video Memory, capable of Shader Model 2.0+", null)).toBeNull();
    expect(comparableModel("Intel I5 4690 / AMD FX 8350 / Snapdragon X Elite", "AMD Ryzen 5 5600")).toBe("AMD FX 8350");
  });
});

describe("prices", () => {
  const usd = (amount: number) => ({ amount, currency: "USD" });

  it("formats money for Turkish readers", () => {
    expect(formatMoney(usd(9.99))).toBe("$9,99");
    expect(formatMoney({ amount: 299, currency: "TRY" })).toBe("₺299,00");
  });

  it("turns price changes into steps", () => {
    const steps = priceSteps(
      [
        { at: 100, price: 20, regular: 20, cut: 0 },
        { at: 200, price: 10, regular: 20, cut: 50 },
      ],
      500,
    );
    expect(steps).toEqual([
      { from: 100, to: 200, price: 20, regular: 20, cut: 0 },
      { from: 200, to: 500, price: 10, regular: 20, cut: 50 },
    ]);
    expect(priceSteps([], 500)).toEqual([]);
  });

  it("compares today's best price with the lowest ever", () => {
    const lowest = { shop: "GOG", price: usd(4), regular: usd(40), cut: 90, at: 1 };
    expect(priceVerdict(usd(4), lowest)).toEqual({ kind: "lowest", percent: 0 });
    expect(priceVerdict(usd(4.5), lowest)).toEqual({ kind: "near", percent: 13 });
    expect(priceVerdict(usd(9.99), lowest)).toEqual({ kind: "above", percent: 150 });
    expect(priceVerdict({ amount: 1, currency: "EUR" }, lowest)).toBeNull();
    expect(priceVerdict(null, lowest)).toBeNull();
    // Once given away: free again counts as the lowest, anything else can't be a percentage.
    const free = { ...lowest, price: usd(0), cut: 100 };
    expect(priceVerdict(usd(0), free)).toEqual({ kind: "lowest", percent: 0 });
    expect(priceVerdict(usd(4), free)).toBeNull();
  });
});
