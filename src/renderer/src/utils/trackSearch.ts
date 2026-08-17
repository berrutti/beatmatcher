export function matchesTrackQuery(
  track: { title: string | null; artist: string | null },
  fallbackLabel: string,
  query: string
): boolean {
  const needle = query.trim().toLowerCase();
  if (!needle) return true;
  const title = (track.title ?? fallbackLabel).toLowerCase();
  if (title.includes(needle)) return true;
  return track.artist ? track.artist.toLowerCase().includes(needle) : false;
}
