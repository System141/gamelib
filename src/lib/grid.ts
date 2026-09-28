// Responsive layout for the virtualized game grid.

/** Height reserved under each cover for the title and meta line. */
export const CAPTION_HEIGHT = 62;
export const MIN_CARD_WIDTH = 180;
/** Library capsules are 2:3 portraits (600×900). */
export const ART_RATIO = 1.5;

export interface GridLayout {
  cols: number;
  cardWidth: number;
  /** Row pitch: cover + caption + gap. */
  rowHeight: number;
  gap: number;
  padding: number;
}

export function computeGridLayout(width: number): GridLayout {
  const padding = width >= 1280 ? 32 : 24;
  const gap = width >= 1280 ? 22 : 18;
  const inner = Math.max(0, width - padding * 2);
  const cols = Math.max(2, Math.floor((inner + gap) / (MIN_CARD_WIDTH + gap)));
  const cardWidth = Math.max(0, (inner - gap * (cols - 1)) / cols);
  const rowHeight = Math.round(cardWidth * ART_RATIO + CAPTION_HEIGHT + gap);
  return { cols, cardWidth, rowHeight, gap, padding };
}
