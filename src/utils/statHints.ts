import { staleCountHint } from "./cachedLabels";

export type StatHintTone = "default" | "warning";

export interface StatHintResult {
  hint?: string;
  tone: StatHintTone;
}

export function refreshFailedStatHint(hasCached: boolean): string {
  return hasCached ? "Refresh failed · cached" : "Refresh failed";
}

export function updateFailedStatHint(hasCached: boolean): string {
  return hasCached ? "Update failed · cached" : "Update failed";
}

export function serviceUpdateStatHint(
  refreshError: string | null | undefined,
  backendError: string | null | undefined,
  hasCached: boolean
): StatHintResult {
  if (refreshError) {
    return { hint: refreshFailedStatHint(hasCached), tone: "warning" };
  }
  if (backendError) {
    return { hint: updateFailedStatHint(hasCached), tone: "warning" };
  }
  return { tone: "default" };
}

export interface GpuStatHintInput {
  refreshError: string | null | undefined;
  totalGpus: number;
  gpuStaleCount: number;
  gpuServerCount: number;
  gpuServersOnline: number;
  gpuOfflineCount: number;
}

export function gpuStatHint(input: GpuStatHintInput): StatHintResult {
  const {
    refreshError,
    totalGpus,
    gpuStaleCount,
    gpuServerCount,
    gpuServersOnline,
    gpuOfflineCount,
  } = input;
  const gpuHasPartialOffline =
    gpuServerCount > 0 && gpuServersOnline > 0 && gpuOfflineCount > 0;

  if (refreshError) {
    const hasCached = refreshError.includes("cached") || totalGpus > 0;
    return { hint: refreshFailedStatHint(hasCached), tone: "warning" };
  }
  if (gpuStaleCount > 0) {
    return { hint: staleCountHint(gpuStaleCount), tone: "warning" };
  }
  if (gpuHasPartialOffline) {
    return {
      hint: `${gpuServersOnline}/${gpuServerCount} servers online`,
      tone: "warning",
    };
  }
  if (gpuServerCount > 0 && gpuServersOnline === 0 && totalGpus > 0) {
    return { hint: "Offline · cached", tone: "warning" };
  }
  if (gpuServerCount > 0 && gpuServersOnline === 0) {
    return { hint: "All servers offline", tone: "warning" };
  }
  return { tone: "default" };
}

export interface QuotaStatHintInput {
  refreshError: string | null | undefined;
  visibleQuotaCount: number;
  quotaHardErrorCount: number;
  quotaBackoffActive: boolean;
  backoffSecs: number;
  quotaStaleCount: number;
}

export function quotaStatHint(input: QuotaStatHintInput): StatHintResult {
  const {
    refreshError,
    visibleQuotaCount,
    quotaHardErrorCount,
    quotaBackoffActive,
    backoffSecs,
    quotaStaleCount,
  } = input;

  if (refreshError) {
    return {
      hint: refreshFailedStatHint(visibleQuotaCount > 0),
      tone: "default",
    };
  }
  if (quotaHardErrorCount > 0) {
    return {
      hint: `${quotaHardErrorCount} update error${quotaHardErrorCount > 1 ? "s" : ""}`,
      tone: "default",
    };
  }
  if (quotaBackoffActive) {
    return { hint: `Backing off ${backoffSecs}s`, tone: "warning" };
  }
  if (quotaStaleCount > 0) {
    return { hint: staleCountHint(quotaStaleCount), tone: "warning" };
  }
  return { tone: "default" };
}
