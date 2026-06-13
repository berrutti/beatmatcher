export function basename(path: string): string {
  return path.split('/').pop() ?? path;
}

// Maps filename -> full path; on duplicate filenames the first occurrence wins
// (scan_folder returns paths in sorted order, so this is deterministic).
export function indexByBasename(paths: string[]): Map<string, string> {
  const map = new Map<string, string>();
  for (const p of paths) {
    const name = basename(p);
    if (!map.has(name)) map.set(name, p);
  }
  return map;
}
