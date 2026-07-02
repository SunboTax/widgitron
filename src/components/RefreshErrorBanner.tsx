interface RefreshErrorBannerProps {
  message: string | null;
  onDismiss: () => void;
  theme?: "light" | "dark";
  cachedLabel?: string;
  className?: string;
}

export function RefreshErrorBanner({
  message,
  onDismiss,
  theme = "dark",
  cachedLabel,
  className = "mb-4",
}: RefreshErrorBannerProps) {
  if (!message) return null;

  const isLight = theme === "light";

  return (
    <div
      className={`${className} p-3 rounded-xl border text-[10px] font-medium leading-relaxed flex items-start justify-between gap-3 ${
        isLight
          ? "bg-amber-50 border-amber-200 text-amber-800"
          : "bg-amber-500/10 border-amber-500/20 text-amber-400"
      }`}
    >
      <span className="min-w-0 flex-1 whitespace-pre-wrap break-words overflow-hidden">
        {cachedLabel ? `${cachedLabel} ` : ""}
        {message}
      </span>
      <button
        type="button"
        onClick={onDismiss}
        className={`text-[9px] uppercase tracking-widest shrink-0 ${
          isLight ? "text-amber-700/70 hover:text-amber-900" : "opacity-70 hover:opacity-100"
        }`}
      >
        Dismiss
      </button>
    </div>
  );
}
