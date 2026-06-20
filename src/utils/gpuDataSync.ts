import type { Dispatch, SetStateAction } from "react";
import type { ServerGpuData } from "../types/config";
import { tauriListen } from "./tauriListen";

export type GpuServerRecord = Pick<ServerGpuData, "host">;

export function mergeGpuServerUpdate<T extends GpuServerRecord>(
  prev: T[],
  item: T
): T[] {
  const index = prev.findIndex((s) => s.host === item.host);
  if (index === -1) return [...prev, item];
  const next = [...prev];
  next[index] = item;
  return next;
}

export async function listenGpuDataSync<T extends GpuServerRecord>(
  setter: Dispatch<SetStateAction<T[]>>,
  isActive: () => boolean
): Promise<() => void> {
  const unsubs: (() => void)[] = [];

  const uClear = await tauriListen("gpu_clear", () => {
    if (!isActive()) return;
    setter([]);
  });
  unsubs.push(uClear);

  const uPrune = await tauriListen("gpu_prune", (event) => {
    if (!isActive()) return;
    const host = event.payload;
    setter((prev) => prev.filter((s) => s.host !== host));
  });
  unsubs.push(uPrune);

  return () => unsubs.forEach((f) => f());
}
