import { WidgetTheme, WidgetThemeConfig } from "../types/theme";

export type WidgetThemeKind = "gpu" | "deadline" | "arxiv" | "quota";

export const DEFAULT_THEME_IDS: Record<WidgetThemeKind, string> = {
  gpu: "theme-gpu-transparent",
  deadline: "theme-deadline-transparent",
  arxiv: "theme-arxiv-transparent",
  quota: "theme-quota-transparent",
};

const SIDEBAR_THEME_IDS: Record<WidgetThemeKind, string> = {
  gpu: "theme-gpu-default",
  deadline: "theme-deadline-default",
  arxiv: "theme-arxiv-default",
  quota: "theme-quota-default",
};

export const PRESET_THEME_IDS: Record<WidgetThemeKind, readonly [string, string]> = {
  gpu: ["theme-gpu-default", "theme-gpu-transparent"],
  deadline: ["theme-deadline-default", "theme-deadline-transparent"],
  arxiv: ["theme-arxiv-default", "theme-arxiv-transparent"],
  quota: ["theme-quota-default", "theme-quota-transparent"],
};

export function widgetThemeKindFromLabel(label: string): WidgetThemeKind {
  if (label.includes("gpu")) return "gpu";
  if (label.includes("deadlines")) return "deadline";
  if (label.includes("arxiv")) return "arxiv";
  return "quota";
}

export function defaultThemeIdForWidgetLabel(label: string): string {
  return DEFAULT_THEME_IDS[widgetThemeKindFromLabel(label)];
}

export function isPresetThemeForWidget(themeId: string, widgetId: string): boolean {
  const kind = widgetThemeKindFromLabel(widgetId);
  return PRESET_THEME_IDS[kind].includes(themeId);
}

export function resolveWidgetTheme(
  config: WidgetThemeConfig,
  windowLabel: string,
  kind?: WidgetThemeKind,
  options?: { sidebarLight?: boolean }
): WidgetTheme | null {
  const defaultId =
    kind && windowLabel === "sidebar"
      ? options?.sidebarLight
        ? DEFAULT_THEME_IDS[kind]
        : SIDEBAR_THEME_IDS[kind]
      : kind
      ? DEFAULT_THEME_IDS[kind]
      : defaultThemeIdForWidgetLabel(windowLabel);
  const themeId = config.assignments?.[windowLabel];
  const theme =
    config.themes.find((t) => t.id === themeId) || config.themes.find((t) => t.id === defaultId);
  return theme || null;
}
