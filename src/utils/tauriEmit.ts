import { emit } from "@tauri-apps/api/event";
import type { TauriEvent, TauriEventMap } from "./tauriListen";

export function tauriEmit<E extends TauriEvent>(
  event: E,
  payload: TauriEventMap[E]
): Promise<void> {
  return emit(event, payload);
}
