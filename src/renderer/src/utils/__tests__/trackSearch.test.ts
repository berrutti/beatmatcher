import { describe, it, expect } from 'vitest';
import { matchesTrackQuery } from '../trackSearch';

describe('matchesTrackQuery', () => {
  it('matches on title', () => {
    expect(matchesTrackQuery({ title: 'Strobe', artist: 'deadmau5' }, 'fallback', 'strobe')).toBe(
      true
    );
  });

  it('matches on artist even when the artist is not in the title', () => {
    expect(matchesTrackQuery({ title: 'Strobe', artist: 'deadmau5' }, 'fallback', 'deadmau5')).toBe(
      true
    );
  });

  it('falls back to the display label when title is null', () => {
    expect(matchesTrackQuery({ title: null, artist: null }, 'track01.mp3', 'track01')).toBe(true);
  });

  it('returns false when neither title, artist, nor fallback label match', () => {
    expect(matchesTrackQuery({ title: 'Strobe', artist: 'deadmau5' }, 'fallback', 'skrillex')).toBe(
      false
    );
  });

  it('returns true for an empty query', () => {
    expect(matchesTrackQuery({ title: 'Strobe', artist: 'deadmau5' }, 'fallback', '')).toBe(true);
  });
});
