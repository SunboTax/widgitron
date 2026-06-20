import { RefreshErrorBanner } from "./RefreshErrorBanner";
import {
  serviceToggleErrorForTab,
  type ServiceToggleError,
} from "../utils/widgetLifecycle";
import { isLiveDataSection, type AppTab, type SettingsSection } from "../utils/sectionLiveData";

interface ToggleErrorBannerProps {
  message: string | null | undefined;
  theme?: string;
  dismissed?: boolean;
  onDismiss?: () => void;
  showWhen?: boolean;
  className?: string;
}

export function ToggleErrorBanner({
  message,
  theme = "dark",
  dismissed = false,
  onDismiss,
  showWhen = true,
  className = "mb-4",
}: ToggleErrorBannerProps) {
  if (!showWhen || !message || dismissed) return null;

  return (
    <RefreshErrorBanner
      message={message}
      onDismiss={onDismiss ?? (() => {})}
      theme={theme === "light" ? "light" : "dark"}
      className={className}
    />
  );
}

interface DashboardServiceToggleErrorProps {
  activeTab: AppTab;
  error: ServiceToggleError | null;
  theme?: string;
  dismissed?: boolean;
  onDismiss?: () => void;
}

export function DashboardServiceToggleError({
  activeTab,
  error,
  theme = "dark",
  dismissed = false,
  onDismiss,
}: DashboardServiceToggleErrorProps) {
  const message = serviceToggleErrorForTab(error, activeTab);

  return (
    <ToggleErrorBanner
      showWhen={isLiveDataSection(activeTab)}
      message={message}
      theme={theme}
      dismissed={dismissed}
      onDismiss={onDismiss}
    />
  );
}

interface SettingsGeneralServiceToggleErrorProps {
  activeSection: SettingsSection;
  error: ServiceToggleError | null;
  theme?: string;
  dismissed?: boolean;
  onDismiss?: () => void;
}

export function SettingsGeneralServiceToggleError({
  activeSection,
  error,
  theme = "dark",
  dismissed = false,
  onDismiss,
}: SettingsGeneralServiceToggleErrorProps) {
  return (
    <ToggleErrorBanner
      showWhen={activeSection === "general"}
      message={error?.message}
      theme={theme}
      dismissed={dismissed}
      onDismiss={onDismiss}
      className=""
    />
  );
}
