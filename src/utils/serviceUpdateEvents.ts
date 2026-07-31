import type { Dispatch, SetStateAction } from "react";
import type {
  ArxivUpdatePayload,
  GpuUpdatePayload,
  PaperUpdatePayload,
  QuotaUpdatePayload,
} from "../types/events";
import { mergeGpuServerUpdate } from "./gpuDataSync";
import { tauriListen } from "./tauriListen";

export interface ServiceUpdateErrorClearers {
  gpu?: { clearRefresh?: () => void };
  paper?: { clearRefresh?: () => void; clearBackend?: () => void };
  arxiv?: { clearRefresh?: () => void; clearBackend?: () => void };
  quota?: { clearRefresh?: () => void; clearBackend?: () => void };
}

export interface ServiceUpdateHandlers {
  gpuSetter?: Dispatch<SetStateAction<GpuUpdatePayload[]>>;
  paperSetter?: Dispatch<SetStateAction<PaperUpdatePayload>>;
  arxivSetter?: Dispatch<SetStateAction<ArxivUpdatePayload>>;
  quotaSetter?: Dispatch<SetStateAction<QuotaUpdatePayload>>;
  onGpuUpdate?: (payload: GpuUpdatePayload) => void;
  onPaperUpdate?: (payload: PaperUpdatePayload) => void;
  onArxivUpdate?: (payload: ArxivUpdatePayload) => void;
  onQuotaUpdate?: (payload: QuotaUpdatePayload) => void;
}

export async function listenServiceUpdateEvents(
  isActive: () => boolean,
  clearers: ServiceUpdateErrorClearers,
  handlers?: ServiceUpdateHandlers
): Promise<() => void> {
  const unsubs: (() => void)[] = [];
  const cleanup = () => unsubs.splice(0).forEach((unsubscribe) => unsubscribe());
  const track = async (registration: Promise<() => void>): Promise<boolean> => {
    const unsubscribe = await registration;
    if (!isActive()) {
      unsubscribe();
      cleanup();
      return false;
    }
    unsubs.push(unsubscribe);
    return true;
  };

  try {
    if (clearers.gpu || handlers?.onGpuUpdate || handlers?.gpuSetter) {
      const active = await track(tauriListen("gpu_update", (event) => {
        if (!isActive()) return;
        clearers.gpu?.clearRefresh?.();
        if (handlers?.onGpuUpdate) {
          handlers.onGpuUpdate(event.payload);
        } else if (handlers?.gpuSetter) {
          handlers.gpuSetter((prev) => mergeGpuServerUpdate(prev, event.payload));
        }
      }));
      if (!active) return () => {};
    }

    if (clearers.paper || handlers?.onPaperUpdate || handlers?.paperSetter) {
      const active = await track(tauriListen("paper_update", (event) => {
        if (!isActive()) return;
        clearers.paper?.clearRefresh?.();
        clearers.paper?.clearBackend?.();
        if (handlers?.onPaperUpdate) {
          handlers.onPaperUpdate(event.payload);
        } else if (handlers?.paperSetter) {
          handlers.paperSetter(event.payload);
        }
      }));
      if (!active) return () => {};
    }

    if (clearers.arxiv || handlers?.onArxivUpdate || handlers?.arxivSetter) {
      const active = await track(tauriListen("arxiv_update", (event) => {
        if (!isActive()) return;
        clearers.arxiv?.clearRefresh?.();
        clearers.arxiv?.clearBackend?.();
        if (handlers?.onArxivUpdate) {
          handlers.onArxivUpdate(event.payload);
        } else if (handlers?.arxivSetter) {
          handlers.arxivSetter(event.payload);
        }
      }));
      if (!active) return () => {};
    }

    if (clearers.quota || handlers?.onQuotaUpdate || handlers?.quotaSetter) {
      const active = await track(tauriListen("quota_update", (event) => {
        if (!isActive()) return;
        clearers.quota?.clearRefresh?.();
        clearers.quota?.clearBackend?.();
        if (handlers?.onQuotaUpdate) {
          handlers.onQuotaUpdate(event.payload);
        } else if (handlers?.quotaSetter) {
          handlers.quotaSetter(event.payload);
        }
      }));
      if (!active) return () => {};
    }
  } catch (error) {
    cleanup();
    throw error;
  }

  return cleanup;
}
