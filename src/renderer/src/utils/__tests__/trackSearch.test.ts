import { describe, it, expect, vi } from 'vitest';

const originalNormalize = String.prototype.normalize;
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

  it('finds a diacritic name typed without its diacritics', () => {
    const track = { title: 'Kinks', artist: 'Rødhåd' };
    expect(matchesTrackQuery(track, '', 'rodhad')).toBe(true);
    expect(matchesTrackQuery(track, '', 'Rødhåd')).toBe(true);
  });

  it('folds the letters that carry no combining mark to strip', () => {
    for (const [artist, typed] of [
      ['Bjørn', 'bjorn'],
      ['Håkan', 'hakan'],
      ['Motörhead', 'motorhead'],
      ['Sigur Rós', 'sigur ros'],
      ['Straße', 'strasse']
    ]) {
      expect(matchesTrackQuery({ title: 't', artist }, '', typed)).toBe(true);
    }
  });

  it('still separates names the folding brings no closer', () => {
    expect(matchesTrackQuery({ title: 't', artist: 'Rødhåd' }, '', 'radhad')).toBe(false);
  });
});

describe('the fold is not redone for every keystroke', () => {
  it('folds a given string once, however many times it is searched', () => {
    let folds = 0;
    const spy = vi.spyOn(String.prototype, 'normalize').mockImplementation(function (
      this: string,
      form?: string
    ) {
      folds++;
      return originalNormalize.call(this, form as never);
    });

    const track = { title: 'Kinks', artist: 'Rødhåd' };
    for (const query of ['r', 'ro', 'rod', 'rodh']) {
      matchesTrackQuery(track, '', query);
    }
    spy.mockRestore();

    // Two fields plus one needle per keystroke would be 12. Caching the fields
    // leaves the needle and one fold of each field.
    expect(folds).toBeLessThan(12);
  });
});
