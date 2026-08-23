// rodhad gives Rødhåd
// A collection can hold thousands of tracks and the filter runs on every
// keystroke, so a track's own fields are folded once and kept. The query is not
// cached: every keystroke is a new string, and the map would grow with each one.
const foldedFields = new Map<string, string>();

function foldField(value: string): string {
  const cached = foldedFields.get(value);
  if (cached !== undefined) return cached;
  const result = fold(value);
  foldedFields.set(value, result);
  return result;
}

function fold(value: string): string {
  return value
    .normalize('NFD')
    .replace(/\p{Diacritic}/gu, '')
    .replace(/ø/gi, 'o')
    .replace(/đ/gi, 'd')
    .replace(/ł/gi, 'l')
    .replace(/æ/gi, 'ae')
    .replace(/œ/gi, 'oe')
    .replace(/ß/g, 'ss')
    .toLowerCase();
}

export function matchesTrackQuery(
  track: { title: string | null; artist: string | null },
  fallbackLabel: string,
  query: string
): boolean {
  const needle = fold(query.trim());
  if (!needle) return true;
  const title = foldField(track.title ?? fallbackLabel);
  if (title.includes(needle)) return true;
  return track.artist ? foldField(track.artist).includes(needle) : false;
}
