import { describe, expect, it } from "vitest";
import { fold, normalizeName } from "./fold";
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
