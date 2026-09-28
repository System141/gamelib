// Turkish (tr-TR) formatting helpers. Unix times are in seconds.

const numberFormat = new Intl.NumberFormat("tr-TR");
const decimalFormat = new Intl.NumberFormat("tr-TR", { maximumFractionDigits: 1 });
const dateFormat = new Intl.DateTimeFormat("tr-TR", { day: "numeric", month: "long", year: "numeric" });
const relativeFormat = new Intl.RelativeTimeFormat("tr", { numeric: "auto" });

export const nowSeconds = () => Math.floor(Date.now() / 1000);

export function formatNumber(n: number): string {
  return numberFormat.format(n);
}

/** Turkish puts the percent sign first: %96. */
export function formatPercent(p: number): string {
  return `%${Math.round(p)}`;
}

export function formatDate(unix: number | null | undefined): string {
  return unix ? dateFormat.format(new Date(unix * 1000)) : "—";
}

export function formatYear(unix: number | null | undefined): string {
  return unix ? String(new Date(unix * 1000).getFullYear()) : "—";
}

const RELATIVE_STEPS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 365 * 86_400],
  ["month", 30 * 86_400],
  ["week", 7 * 86_400],
  ["day", 86_400],
  ["hour", 3_600],
  ["minute", 60],
];

/** "3 gün önce", "dün", "2 saat önce", "az önce". */
export function formatRelative(unix: number | null | undefined, now = nowSeconds()): string {
  if (!unix) return "—";
  const diff = unix - now;
  const abs = Math.abs(diff);
  if (abs < 60) return "az önce";
  for (const [unit, seconds] of RELATIVE_STEPS) {
    if (abs >= seconds) {
      return relativeFormat.format(Math.round(diff / seconds), unit);
    }
  }
  return "az önce";
}

/** Compact duration for ETAs: "45 sn", "3 dk 20 sn", "1 sa 5 dk". */
export function formatDuration(totalSeconds: number): string {
  const s = Math.max(0, Math.round(totalSeconds));
  if (s < 60) return `${s} sn`;
  const minutes = Math.floor(s / 60);
  if (minutes < 60) {
    const rest = s % 60;
    return rest ? `${minutes} dk ${rest} sn` : `${minutes} dk`;
  }
  const hours = Math.floor(minutes / 60);
  const restMinutes = minutes % 60;
  return restMinutes ? `${hours} sa ${restMinutes} dk` : `${hours} sa`;
}

const BYTE_UNITS = ["B", "KB", "MB", "GB", "TB"];

export function formatBytes(bytes: number | null | undefined): string {
  if (bytes == null || bytes < 0) return "—";
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < BYTE_UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${unit === 0 ? value : decimalFormat.format(value)} ${BYTE_UNITS[unit]}`;
}

export type ReviewTone = "pos" | "mixed" | "neg" | "none";

/** Steam colours reviews: positive (6–9) blue, mixed (5) amber, negative (1–4) red. */
export function reviewTone(score: number): ReviewTone {
  if (score >= 6) return "pos";
  if (score === 5) return "mixed";
  if (score >= 1) return "neg";
  return "none";
}

/** Released within the last `days` days. */
export function isRecent(releaseDate: number | null | undefined, days = 7, now = nowSeconds()): boolean {
  return releaseDate != null && releaseDate <= now + 86_400 && now - releaseDate <= days * 86_400;
}

/** Shortened file type from a MIME type, e.g. "application/zip" → "ZIP". */
export function fileKind(contentType: string | null | undefined, fileName?: string | null): string | null {
  const ext = fileName?.includes(".") ? fileName.split(".").pop() : null;
  if (ext && ext.length <= 5) return ext.toUpperCase();
  if (!contentType) return null;
  const sub = contentType.split("/")[1] ?? contentType;
  const cleaned = sub
    .replace(/^x-/, "")
    .replace(/^vnd\..*/, "")
    .replace(/-compressed$/, "");
  return cleaned ? cleaned.toUpperCase().slice(0, 8) : null;
}
