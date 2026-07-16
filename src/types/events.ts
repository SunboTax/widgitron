import type {
  AppConfig,
  ArxivConfig,
  ArxivPaper,
  GpuConfig,
  PaperConfig,
  PaperDeadlineInfo,
  QuotaConfig,
  QuotaItem,
  ServerGpuData,
} from "./config";
import type { OtaDownloadProgress, SidebarDockState } from "./tauri";
import type { WidgetThemeConfig } from "./theme";

export type AppConfigUpdatePayload = AppConfig;
export type QuotaConfigUpdatePayload = QuotaConfig;
export type ArxivConfigUpdatePayload = ArxivConfig;
export type GpuConfigUpdatePayload = GpuConfig;
export type PaperConfigUpdatePayload = PaperConfig;
export type ThemeUpdatePayload = WidgetThemeConfig;
export type OtaDownloadProgressPayload = OtaDownloadProgress;
export type SidebarStateUpdatePayload = SidebarDockState;

export type GpuUpdatePayload = ServerGpuData;
export type PaperUpdatePayload = PaperDeadlineInfo[];
export type ArxivUpdatePayload = ArxivPaper[];
export type QuotaUpdatePayload = QuotaItem[];

export type WidgetVisibilityChangedPayload = {
  id: string;
  visible: boolean;
};

export type GpuPrunePayload = string;
export type BackendServiceErrorPayload = string;
export type BackendServiceErrorEvent = "paper_error" | "arxiv_error";

export type QuotaMonitorStatusPayload = {
  consecutive_failures: number;
  backoff_secs: number;
  all_hard_failed: boolean;
  last_error?: string | null;
};

/** Payload for unit-emit events (gpu_clear, arxiv archive refresh signals). */
export type UnitPayload = null;
