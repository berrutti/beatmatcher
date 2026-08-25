import { describe, it, expect, vi, afterEach } from 'vitest';
import { offerToDeck, loadToDeck } from '../deckDrop';

describe('a deck offer always carries the contract', () => {
  const listeners: EventListener[] = [];
  afterEach(() => {
    for (const listener of listeners) window.removeEventListener('bm:collection-drop', listener);
    listeners.length = 0;
  });

  function listen(handler: EventListener) {
    listeners.push(handler);
    window.addEventListener('bm:collection-drop', handler);
  }

  it('gives every listener an accept it can call', () => {
    let called = false;
    listen((event) => {
      const detail = (event as CustomEvent).detail;
      expect(typeof detail.accept).toBe('function');
      detail.accept();
      called = true;
    });

    expect(offerToDeck('/music/a.mp3', 'A')).toBe(true);
    expect(called).toBe(true);
  });

  it('reports refusal when nothing accepts', () => {
    listen(() => {});
    expect(offerToDeck('/music/a.mp3', 'A')).toBe(false);
  });

  it('sends the same shape from loadToDeck as from a drag', () => {
    const seen = vi.fn();
    listen((event) => seen(Object.keys((event as CustomEvent).detail).sort()));
    loadToDeck('/music/a.mp3', 'B');
    expect(seen).toHaveBeenCalledWith(['accept', 'deckId', 'path']);
  });
});
