import type { BackendServiceErrorEvent } from "../types/events";
import { tauriListen } from "./tauriListen";

export async function listenBackendServiceError(
  eventName: BackendServiceErrorEvent,
  onError: (message: string | null) => void,
  isActive: () => boolean
): Promise<() => void> {
  return tauriListen(eventName, (event) => {
    if (!isActive()) return;
    const msg = event.payload?.trim();
    onError(msg ? msg : null);
  });
}
