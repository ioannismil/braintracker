const DAY = 86_400_000;

function startOfDay(d: Date) {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

/** Bucket label for the entry list ("Today", "Yesterday", "This week", "March", "March 2025"). */
export function groupLabel(ts: number): string {
  const d = new Date(ts);
  const now = new Date();
  const days = Math.round((startOfDay(now) - startOfDay(d)) / DAY);
  if (days <= 0) return "Today";
  if (days === 1) return "Yesterday";
  if (days < 7) return "This week";
  if (d.getFullYear() === now.getFullYear()) return d.toLocaleString(undefined, { month: "long" });
  return d.toLocaleString(undefined, { month: "long", year: "numeric" });
}

/** Compact time for list rows: "14:32" today, otherwise "12 Nov" / "12 Nov 2024". */
export function shortWhen(ts: number): string {
  const d = new Date(ts);
  const now = new Date();
  if (startOfDay(d) === startOfDay(now)) {
    return d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  }
  return d.toLocaleDateString(undefined, {
    day: "numeric",
    month: "short",
    ...(d.getFullYear() !== now.getFullYear() ? { year: "numeric" } : {}),
  });
}

export function longDate(ts: number): string {
  return new Date(ts).toLocaleString(undefined, {
    weekday: "short",
    day: "numeric",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function relative(ts: number): string {
  const s = Math.round((Date.now() - ts) / 1000);
  if (s < 45) return "just now";
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h} h ago`;
  const d = Math.round(h / 24);
  if (d < 30) return `${d} day${d === 1 ? "" : "s"} ago`;
  return shortWhen(ts);
}
