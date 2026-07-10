export function matchesTrackQuery(
  track: { title: string | null; artist: string | null },
  fallbackLabel: string,
  query: string
): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  const title = (track.title ?? fallbackLabel).toLowerCase();
  if (title.includes(q)) return true;
  return track.artist ? track.artist.toLowerCase().includes(q) : false;
}
