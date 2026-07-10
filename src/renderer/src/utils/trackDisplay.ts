export function displayName(filename: string): string {
  return filename.replace(/\.(mp3|wav|flac|aac|ogg|m4a|aiff?)$/i, '');
}

// Playlist items have a null addedAt when the playlist was saved before
// per-playlist "date added" existed, so there's genuinely nothing recorded.
export function formatAddedDate(addedAt: number | null): string {
  if (addedAt === null) return '-';
  return new Date(addedAt).toLocaleDateString(undefined, {
    year: '2-digit',
    month: '2-digit',
    day: '2-digit'
  });
}
