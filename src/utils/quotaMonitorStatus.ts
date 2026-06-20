import type { QuotaMonitorStatusPayload } from "../types/events";
import { tauriListen } from "./tauriListen";

export type QuotaMonitorStatus = QuotaMonitorStatusPayload;

export async function listenQuotaMonitorStatus(
  onStatus: (status: QuotaMonitorStatus | null) => void,
  onBackendError: (message: string | null) => void,
  isActive: () => boolean
): Promise<() => void> {
  return tauriListen("quota_monitor_status", (event) => {
    if (!isActive()) return;
    const status = event.payload;
    if (status.consecutive_failures === 0) {
      onStatus(null);
      onBackendError(null);
    } else {
      onStatus(status);
      const msg = status.last_error?.trim();
      onBackendError(msg ? msg : null);
    }
  });
}
