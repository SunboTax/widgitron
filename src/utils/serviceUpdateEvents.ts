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

  const uGpu = await tauriListen("gpu_update", (event) => {
    if (!isActive()) return;
    clearers.gpu?.clearRefresh?.();
    if (handlers?.onGpuUpdate) {
      handlers.onGpuUpdate(event.payload);
    } else if (handlers?.gpuSetter) {
      handlers.gpuSetter((prev) => mergeGpuServerUpdate(prev, event.payload));
    }
  });
  unsubs.push(uGpu);

  const uPaper = await tauriListen("paper_update", (event) => {
    if (!isActive()) return;
    clearers.paper?.clearRefresh?.();
    clearers.paper?.clearBackend?.();
    if (handlers?.onPaperUpdate) {
      handlers.onPaperUpdate(event.payload);
    } else if (handlers?.paperSetter) {
      handlers.paperSetter(event.payload);
    }
  });
  unsubs.push(uPaper);

  const uArxiv = await tauriListen("arxiv_update", (event) => {
    if (!isActive()) return;
    clearers.arxiv?.clearRefresh?.();
    clearers.arxiv?.clearBackend?.();
    if (handlers?.onArxivUpdate) {
      handlers.onArxivUpdate(event.payload);
    } else if (handlers?.arxivSetter) {
      handlers.arxivSetter(event.payload);
    }
  });
  unsubs.push(uArxiv);

  const uQuota = await tauriListen("quota_update", (event) => {
    if (!isActive()) return;
    clearers.quota?.clearRefresh?.();
    clearers.quota?.clearBackend?.();
    if (handlers?.onQuotaUpdate) {
      handlers.onQuotaUpdate(event.payload);
    } else if (handlers?.quotaSetter) {
      handlers.quotaSetter(event.payload);
    }
  });
  unsubs.push(uQuota);

  return () => unsubs.forEach((f) => f());
}
