// Pure helpers for "Bilgisayarım kaldırır mı?": which requirement row a check belongs to, the
// verdict of a whole list, and which model in a requirement to compare this computer's with.

import type { CheckKind, RequirementCheck, RequirementKind, RequirementList, Verdict } from "./types";

/** The requirement row each check is shown on. */
export const CHECK_ROW: Record<CheckKind, RequirementKind> = {
  memory: "memory",
  video_memory: "graphics",
  storage: "storage",
  ssd: "storage",
  directx: "directx",
  windows: "os",
  bits64: "os",
};

/** The worst of some checks: short, then unknown, then ok; null without checks. */
export function worst(checks: RequirementCheck[]): Verdict | null {
  if (checks.some((c) => c.verdict === "short")) return "short";
  if (checks.some((c) => c.verdict === "unknown")) return "unknown";
  return checks.length > 0 ? "ok" : null;
}

export type ListVerdict = { kind: "ok" } | { kind: "short"; missing: CheckKind[] } | { kind: "partial" } | { kind: "none" };

/** Met, short of something, met as far as it could be measured, or nothing to measure. */
export function listVerdict(list: RequirementList): ListVerdict {
  const missing = list.checks.filter((c) => c.verdict === "short").map((c) => c.kind);
  if (missing.length > 0) return { kind: "short", missing };
  if (list.checks.length === 0) return { kind: "none" };
  if (list.checks.some((c) => c.verdict === "unknown")) return { kind: "partial" };
  return { kind: "ok" };
}

export type Vendor = "nvidia" | "amd" | "intel" | "apple" | "qualcomm";

const VENDOR_WORDS: [Vendor, RegExp][] = [
  ["nvidia", /nvidia|geforce|\brtx\b|\bgtx\b|\bgt\s?\d|quadro/i],
  ["amd", /\bamd\b|radeon|\brx\s?\d|ryzen|\bfx[-\s]?\d|athlon|phenom|threadripper/i],
  ["intel", /intel|\bcore\s?(?:i\d|ultra)|\bi[3579][-\s]?\d|xeon|pentium|celeron|\barc\s?[ab]\d|iris|\buhd\b|\bhd graphics/i],
  ["qualcomm", /qualcomm|snapdragon|adreno/i],
  ["apple", /\bapple\b|\bm[1-5]\b/i],
];

export function vendorOf(name: string): Vendor | null {
  return VENDOR_WORDS.find(([, re]) => re.test(name))?.[0] ?? null;
}

/**
 * The model in a requirement ("GeForce GTX 1660 / Radeon RX 5500 XT 8GB / Arc A580") made by
 * the same company as `mine`, else the first one; notes in parentheses and memory sizes are
 * left out. Only names of a known maker count, so "2.0 Ghz" or "Shader Model 2.0" do not.
 */
export function comparableModel(requirement: string, mine: string | null): string | null {
  const options = requirement
    .replace(/\([^)]*\)/g, " ")
    .split(/\s*(?:\/|\bor\b|,|;|\|)\s*/i)
    .map((o) =>
      o
        .replace(/\b\d+(?:\.\d+)?\s*(?:GB|MB)\+?(?:\s*(?:of\s+)?(?:VRAM|video memory))?/gi, "")
        .replace(/\s+/g, " ")
        .trim(),
    )
    .filter((o) => /\d/.test(o) && vendorOf(o) != null);
  if (options.length === 0) return null;
  const vendor = mine ? vendorOf(mine) : null;
  return (vendor && options.find((o) => vendorOf(o) === vendor)) || options[0]!;
}
