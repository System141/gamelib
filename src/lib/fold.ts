// Text folding for client-side matching.

/**
 * Case- and accent-insensitive key for Turkish text: "Bağımsız" and "bagimsiz" fold to the same
 * string, as do "İ", "I", "ı" and "i".
 */
export function fold(s: string): string {
  return s
    .toLocaleLowerCase("tr")
    .normalize("NFD")
    .replace(/\p{M}+/gu, "")
    .replace(/ı/g, "i");
}

const DROPPED = new Set(["'", "’", "‘", "`", "´", "ʼ", "."]);

/**
 * Same normalization as `gamelib_core::search::normalize` in Rust (used by the browser preview's
 * mock search): lowercase, Turkish i unified, apostrophes/dots dropped, other punctuation → space.
 */
export function normalizeName(input: string): string {
  let out = "";
  let pendingSpace = false;
  const push = (c: string) => {
    if (pendingSpace) {
      out += " ";
      pendingSpace = false;
    }
    out += c;
  };
  for (const ch of input) {
    if (ch === "İ" || ch === "I" || ch === "ı" || ch === "i") {
      push("i");
    } else if (DROPPED.has(ch) || /\p{M}/u.test(ch)) {
      continue;
    } else if (/[\p{L}\p{N}]/u.test(ch)) {
      push(ch.toLowerCase());
    } else {
      pendingSpace = out.length > 0;
    }
  }
  return out;
}
