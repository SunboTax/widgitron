export const SHOWING_CACHED_PHRASE = "showing cached";

export function staleCountHint(count: number): string {
  return `${count} ${SHOWING_CACHED_PHRASE}`;
}

export function messageShowsCached(msg: string | null | undefined): boolean {
  return typeof msg === "string" && msg.toLowerCase().includes(SHOWING_CACHED_PHRASE);
}

export const CACHED_LABELS = {
  gpu: {
    // Placeholder for a future gpu_error backend channel.
    backend: "Update failed — showing cached GPU data.",
    refresh: "Refresh failed — showing cached GPU data.",
  },
  deadlines: {
    backend: "Update failed — showing cached deadlines.",
    refresh: "Refresh failed — showing cached deadlines.",
  },
  arxiv: {
    backend: "Update failed — showing cached papers.",
    refresh: "Refresh failed — showing cached papers.",
  },
  quota: {
    backend: "Update failed — showing cached quota data.",
    refresh: "Refresh failed — showing cached quota data.",
  },
} as const;

export function cachedLabelWhen(hasData: boolean, label: string): string | undefined {
  return hasData ? label : undefined;
}

export function gpuRefreshCachedLabel(hasData: boolean): string | undefined {
  return cachedLabelWhen(hasData, CACHED_LABELS.gpu.refresh);
}
