import { useState, useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { WidgetTheme } from "../types/theme";
import { resolveWidgetTheme, type WidgetThemeKind } from "../utils/widgetTheme";
import { tauriInvoke } from "../utils/tauriInvoke";
import { tauriListen } from "../utils/tauriListen";

export function useWidgetTheme(kind: WidgetThemeKind): WidgetTheme | null {
  const win = getCurrentWindow();
  const [currentTheme, setCurrentTheme] = useState<WidgetTheme | null>(null);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;

    const load = async () => {
      try {
        const config = await tauriInvoke("get_theme_config");
        if (!active) return;
        setCurrentTheme(resolveWidgetTheme(config, win.label, kind));

        unlisten = await tauriListen("theme_update", (event) => {
          if (!active) return;
          setCurrentTheme(resolveWidgetTheme(event.payload, win.label, kind));
        });
      } catch (e) {
        console.error("Widget theme load failed", e);
      }
    };

    load();

    return () => {
      active = false;
      unlisten?.();
    };
  }, [win.label, kind]);

  return currentTheme;
}
