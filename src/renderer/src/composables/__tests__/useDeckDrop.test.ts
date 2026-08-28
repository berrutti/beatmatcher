import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { defineComponent } from 'vue';
import { mount } from '@vue/test-utils';
import { useDeckDrop, type DropTargetDeck } from '../useDeckDrop';
import { offerToDeck } from '@renderer/utils/deckDrop';
import { DROP_LANDING_MS } from '@renderer/utils/dragGhostLanding';
import type { LoadableTrack } from '@renderer/stores/decks';

const TRACK: LoadableTrack = {
  path: '/music/a.mp3',
  name: 'a',
  bpm: 120,
  silenceEnd: 0,
  beatOffset: 0,
  onBeatOffsetChange: () => {}
};

function deckStub(overrides: Partial<DropTargetDeck> = {}) {
  return {
    id: 'A',
    loadedPath: null,
    loopPlaying: false,
    loadTrack: vi.fn().mockResolvedValue(undefined),
    ...overrides
  };
}

function mountTarget(deck: DropTargetDeck, resolve = (): LoadableTrack | null => TRACK) {
  const captured: { api?: ReturnType<typeof useDeckDrop> } = {};
  const wrapper = mount(
    defineComponent({
      setup() {
        captured.api = useDeckDrop({ deck: () => deck, resolve });
        return () => null;
      }
    })
  );
  const { api } = captured;
  if (!api) throw new Error('the composable did not run');
  return { api, wrapper };
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe('a deck that takes a dropped track', () => {
  it('accepts the offer, so the drag ghost animates into it', () => {
    const deck = deckStub();
    const { wrapper } = mountTarget(deck);

    expect(offerToDeck(TRACK.path, 'A')).toBe(true);
    wrapper.unmount();
  });

  it('holds the load for the drop animation, so the name changes as the ghost lands', () => {
    const deck = deckStub();
    const { wrapper } = mountTarget(deck);

    offerToDeck(TRACK.path, 'A');
    expect(deck.loadTrack).not.toHaveBeenCalled();

    vi.advanceTimersByTime(DROP_LANDING_MS);
    expect(deck.loadTrack).toHaveBeenCalledWith(TRACK);
    wrapper.unmount();
  });

  it('refuses an offer addressed to another deck', () => {
    const deck = deckStub({ id: 'B' });
    const { wrapper } = mountTarget(deck);

    expect(offerToDeck(TRACK.path, 'A')).toBe(false);
    vi.advanceTimersByTime(DROP_LANDING_MS);
    expect(deck.loadTrack).not.toHaveBeenCalled();
    wrapper.unmount();
  });

  it('refuses the track it already has, so a re-drop is not a reload', () => {
    const deck = deckStub({ loadedPath: TRACK.path });
    const { wrapper } = mountTarget(deck);

    expect(offerToDeck(TRACK.path, 'A')).toBe(false);
    wrapper.unmount();
  });

  it('refuses a path the collection cannot resolve', () => {
    const deck = deckStub();
    const { wrapper } = mountTarget(deck, () => null);

    expect(offerToDeck(TRACK.path, 'A')).toBe(false);
    wrapper.unmount();
  });

  it('asks before replacing a looping deck, and loads nothing until confirmed', () => {
    const deck = deckStub({ loopPlaying: true });
    const { api, wrapper } = mountTarget(deck);

    expect(offerToDeck(TRACK.path, 'A')).toBe(true);
    vi.advanceTimersByTime(DROP_LANDING_MS);
    expect(deck.loadTrack).not.toHaveBeenCalled();
    expect(api.pendingLoad.value).toEqual(TRACK);

    api.confirmPendingLoad();
    expect(deck.loadTrack).toHaveBeenCalledWith(TRACK);
    expect(api.pendingLoad.value).toBe(null);
    wrapper.unmount();
  });

  it('loads nothing when the replacement is cancelled', () => {
    const deck = deckStub({ loopPlaying: true });
    const { api, wrapper } = mountTarget(deck);

    offerToDeck(TRACK.path, 'A');
    api.cancelPendingLoad();

    vi.advanceTimersByTime(DROP_LANDING_MS);
    expect(deck.loadTrack).not.toHaveBeenCalled();
    expect(api.pendingLoad.value).toBe(null);
    wrapper.unmount();
  });

  it('drops its scheduled load on unmount, so a view left mid-animation loads nothing', () => {
    const deck = deckStub();
    const { wrapper } = mountTarget(deck);

    offerToDeck(TRACK.path, 'A');
    wrapper.unmount();

    vi.advanceTimersByTime(DROP_LANDING_MS);
    expect(deck.loadTrack).not.toHaveBeenCalled();
  });

  it('stops listening once unmounted', () => {
    const deck = deckStub();
    mountTarget(deck).wrapper.unmount();

    expect(offerToDeck(TRACK.path, 'A')).toBe(false);
  });
});
