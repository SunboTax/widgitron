import { RefreshErrorBanner } from "./RefreshErrorBanner";

interface ServiceErrorBannersProps {
  backendError?: string | null;
  refreshError?: string | null;
  onDismissBackend?: () => void;
  onDismissRefresh: () => void;
  theme?: "light" | "dark";
  backendCachedLabel?: string;
  refreshCachedLabel?: string;
  showBackend?: boolean;
  /** GPU-style panels: refresh banner only, no backend channel yet. */
  refreshOnly?: boolean;
  className?: string;
}

export function ServiceErrorBanners({
  backendError,
  refreshError,
  onDismissBackend,
  onDismissRefresh,
  theme = "dark",
  backendCachedLabel,
  refreshCachedLabel,
  showBackend = true,
  refreshOnly = false,
  className,
}: ServiceErrorBannersProps) {
  const showBackendBanner = !refreshOnly && showBackend;
  const bannerClass = className ? "" : "mb-4";
  const content = (
    <>
      <RefreshErrorBanner
        message={showBackendBanner ? (backendError ?? null) : null}
        onDismiss={onDismissBackend ?? (() => {})}
        theme={theme}
        cachedLabel={backendCachedLabel}
        className={bannerClass}
      />
      <RefreshErrorBanner
        message={refreshError ?? null}
        onDismiss={onDismissRefresh}
        theme={theme}
        cachedLabel={refreshCachedLabel}
        className={bannerClass}
      />
    </>
  );

  if (!className) return content;

  return <div className={className}>{content}</div>;
}
