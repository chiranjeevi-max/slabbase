import type { LayoutItem } from "../bindings";

export const GRID_COLUMNS = 12;
export const ROW_HEIGHT = 30;
export const MAX_GRID_ROW = 65_535; // LayoutItem coordinates use u16 on the wire.

export type GridRect = Pick<LayoutItem, "x" | "y" | "w" | "h">;
export type StudioItemSource = Pick<LayoutItem, "kind" | "w" | "h"> & { id?: string };

export function gridItemStyle({ x, y, w, h }: GridRect) {
  return {
    left: `${(x / GRID_COLUMNS) * 100}%`,
    top: y * ROW_HEIGHT,
    width: `${(w / GRID_COLUMNS) * 100}%`,
    height: h * ROW_HEIGHT - 4,
  };
}
