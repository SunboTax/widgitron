export const hexToRgba = (hex: string, opacity: number) => {
  let r = 0, g = 0, b = 0;
  const h = hex.replace("#", "");
  if (h.length === 3) {
    r = parseInt(h[0] + h[0], 16);
    g = parseInt(h[1] + h[1], 16);
    b = parseInt(h[2] + h[2], 16);
  } else if (h.length === 6) {
    r = parseInt(h.substring(0, 2), 16);
    g = parseInt(h.substring(2, 4), 16);
    b = parseInt(h.substring(4, 6), 16);
  }
  return `rgba(${r}, ${g}, ${b}, ${opacity})`;
};

export const adjustColorOpacity = (color: string, opacity: number): string => {
  if (color.startsWith("rgba")) {
    return color.replace(/[\d.]+\)$/, `${opacity})`);
  }
  if (color.startsWith("#")) {
    return hexToRgba(color, opacity);
  }
  return color;
};

export const isLightColor = (hex: string): boolean => {
  const h = hex.replace("#", "");
  if (h.length !== 3 && h.length !== 6) return false;
  const full = h.length === 3 ? h.split("").map((c) => c + c).join("") : h;
  const r = parseInt(full.substring(0, 2), 16);
  const g = parseInt(full.substring(2, 4), 16);
  const b = parseInt(full.substring(4, 6), 16);
  if ([r, g, b].some((v) => Number.isNaN(v))) return false;
  return (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255 > 0.72;
};
