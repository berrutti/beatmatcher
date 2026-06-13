import { describe, it, expect } from 'vitest';
import { basename, indexByBasename } from '../path';

describe('basename', () => {
  it('returns the last path segment', () => {
    expect(basename('/music/sub/a.mp3')).toBe('a.mp3');
  });

  it('returns the input when there is no separator', () => {
    expect(basename('a.mp3')).toBe('a.mp3');
  });
});

describe('indexByBasename', () => {
  it('maps filenames to full paths', () => {
    const map = indexByBasename(['/lib/a.mp3', '/lib/deep/b.mp3']);
    expect(map.get('a.mp3')).toBe('/lib/a.mp3');
    expect(map.get('b.mp3')).toBe('/lib/deep/b.mp3');
  });

  it('keeps the first occurrence on duplicate filenames', () => {
    const map = indexByBasename(['/lib/a.mp3', '/lib/deep/a.mp3']);
    expect(map.get('a.mp3')).toBe('/lib/a.mp3');
  });
});
