export function dateStamp(date: Date = new Date(), divider = '-'): string {
  const pad = (value: number) => String(value).padStart(2, '0');
  return `${pad(date.getDate())}${divider}${pad(date.getMonth() + 1)}${divider}${date.getFullYear()}`;
}

export function formatMs(ms: number): string {
  const totalSec = Math.floor(ms / 1000);
  const h = Math.floor(totalSec / 3600);
  const m = Math.floor((totalSec % 3600) / 60);
  const s = totalSec % 60;
  if (h > 0) return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
  return `${m}:${String(s).padStart(2, '0')}`;
}
