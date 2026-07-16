import type { QuotaItem } from "../types/config";
import { messageShowsCached } from "./cachedLabels";

export type QuotaDisplayItem = Pick<
  QuotaItem,
  "current_value" | "error_msg" | "bars"
>;

export function isStaleQuotaWarning(msg: string | null | undefined): boolean {
  if (!msg) return false;
  const lower = msg.toLowerCase();
  // Soft retention only — hard fetch errors should render as errors even if
  // last-known values are still displayed.
  return (
    lower.includes("showing cached") ||
    messageShowsCached(msg) ||
    lower.includes("retaining last") ||
    lower.includes("offline —") ||
    lower.includes("offline -")
  );
}

export function quotaHasDisplayValue(q: QuotaDisplayItem): boolean {
  if (q.bars && q.bars.length > 0) return true;
  return q.current_value !== undefined && q.current_value !== null;
}

export function quotaErrorClassName(q: QuotaDisplayItem, staleSuffix: string, errorSuffix: string): string {
  const stale = isStaleQuotaWarning(q.error_msg) && quotaHasDisplayValue(q);
  return stale ? staleSuffix : errorSuffix;
}

export function orderQuotaByConfig<T extends { id: string }>(
  quotaData: T[],
  configItems: { id: string }[] | undefined | null,
): T[] {
  if (!configItems || configItems.length === 0) {
    return quotaData;
  }
  const byId = new Map(quotaData.map((q) => [q.id, q]));
  return configItems
    .map((cfg) => byId.get(cfg.id))
    .filter((q): q is T => q !== undefined);
}
