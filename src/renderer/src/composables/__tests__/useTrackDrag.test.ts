// @vitest-environment happy-dom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { startTrackDrag } from '../useTrackDrag';
import { DROP_LANDING_ATTRIBUTE } from '@renderer/utils/dropLanding';

function deckAt(): HTMLElement {
  const deck = document.createElement('div');
  deck.dataset.deckId = 'A';
  deck.innerHTML = `<span ${DROP_LANDING_ATTRIBUTE}></span>`;
  document.body.appendChild(deck);
  return deck;
}

function rowElement(): HTMLElement {
  const row = document.createElement('div');
  document.body.appendChild(row);
  return row;
}

// The drag clones the element it was pressed on, so the press has to be a real
// event on a real row rather than a bare object.
function pressOn(row: HTMLElement, store: { startDrag: () => void; endDrag: () => void }) {
  row.addEventListener('pointerdown', (event) => startTrackDrag(store, event, '/music/a.mp3'), {
    once: true
  });
  row.dispatchEvent(
    new PointerEvent('pointerdown', { button: 0, clientX: 0, clientY: 0, bubbles: true })
  );
}

describe('a track reaches the deck when the ghost does', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    document.body.innerHTML = '';
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('offers the track to the deck before the ghost is released', () => {
    const deck = deckAt();
    const row = rowElement();
    const store = { startDrag: vi.fn(), endDrag: vi.fn() };
    const offered = vi.fn();
    window.addEventListener('bm:collection-drop', offered);
    vi.spyOn(document, 'elementFromPoint').mockReturnValue(deck);

    pressOn(row, store);
    window.dispatchEvent(new PointerEvent('pointermove', { clientX: 100, clientY: 100 }));
    window.dispatchEvent(new PointerEvent('pointerup', { clientX: 100, clientY: 100 }));

    // Synchronous: whether a deck takes it is what decides where the ghost
    // flies, so it cannot be learned after the animation has started.
    expect(offered).toHaveBeenCalledTimes(1);

    window.removeEventListener('bm:collection-drop', offered);
  });

  it('takes the ghost home when the deck refuses the track', () => {
    const deck = deckAt();
    const row = rowElement();
    const store = { startDrag: vi.fn(), endDrag: vi.fn() };
    const refused = vi.fn();
    // A deck already loading, or already holding this track, answers nothing.
    window.addEventListener('bm:collection-drop', refused);
    vi.spyOn(document, 'elementFromPoint').mockReturnValue(deck);

    pressOn(row, store);
    window.dispatchEvent(new PointerEvent('pointermove', { clientX: 100, clientY: 100 }));
    const ghost = document.querySelector('.collection__drag-ghost');
    expect(ghost).not.toBeNull();
    window.dispatchEvent(new PointerEvent('pointerup', { clientX: 100, clientY: 100 }));

    expect(refused).toHaveBeenCalledTimes(1);
    // Refused, so it goes back to where it started rather than into the deck.
    expect(ghost instanceof HTMLElement && ghost.style.transform).toBe('scale(1)');

    window.removeEventListener('bm:collection-drop', refused);
  });

  it('shrinks the ghost into the deck that accepted it', () => {
    const deck = deckAt();
    const row = rowElement();
    const store = { startDrag: vi.fn(), endDrag: vi.fn() };
    const accepting = (event: Event) => {
      const detail = (event as CustomEvent<{ accept: () => void }>).detail;
      detail.accept();
    };
    window.addEventListener('bm:collection-drop', accepting);
    vi.spyOn(document, 'elementFromPoint').mockReturnValue(deck);

    pressOn(row, store);
    window.dispatchEvent(new PointerEvent('pointermove', { clientX: 100, clientY: 100 }));
    const ghost = document.querySelector('.collection__drag-ghost');
    window.dispatchEvent(new PointerEvent('pointerup', { clientX: 100, clientY: 100 }));

    expect(ghost instanceof HTMLElement && ghost.style.transform).not.toBe('scale(1)');
    window.removeEventListener('bm:collection-drop', accepting);
  });

  it('does not drop at all when the release was over nothing', () => {
    const row = rowElement();
    const store = { startDrag: vi.fn(), endDrag: vi.fn() };
    const dropped = vi.fn();
    window.addEventListener('bm:collection-drop', dropped);
    vi.spyOn(document, 'elementFromPoint').mockReturnValue(null);

    pressOn(row, store);
    window.dispatchEvent(new PointerEvent('pointermove', { clientX: 100, clientY: 100 }));
    window.dispatchEvent(new PointerEvent('pointerup', { clientX: 100, clientY: 100 }));

    expect(dropped).not.toHaveBeenCalled();
    window.removeEventListener('bm:collection-drop', dropped);
  });
});

describe('a drag only starts from a plain left press on the row', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    document.body.innerHTML = '';
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  function press(row: HTMLElement, init: PointerEventInit, target?: HTMLElement) {
    const store = { startDrag: vi.fn(), endDrag: vi.fn() };
    const event = new PointerEvent('pointerdown', { bubbles: true, ...init });
    const prevented = vi.spyOn(event, 'preventDefault');
    (target ?? row).addEventListener(
      'pointerdown',
      (pointer) => startTrackDrag(store, pointer, '/music/a.mp3'),
      { once: true }
    );
    (target ?? row).dispatchEvent(event);
    window.dispatchEvent(new PointerEvent('pointermove', { clientX: 300, clientY: 300 }));
    return { store, prevented };
  }

  it('ignores a right or middle press', () => {
    for (const button of [1, 2]) {
      const row = rowElement();
      const { store, prevented } = press(row, { button, clientX: 0, clientY: 0 });
      expect(store.startDrag, `button ${button}`).not.toHaveBeenCalled();
      expect(prevented, `button ${button}`).not.toHaveBeenCalled();
    }
  });

  it('leaves a press on a button in the row alone', () => {
    const row = rowElement();
    const action = document.createElement('button');
    row.appendChild(action);
    const { store, prevented } = press(row, { button: 0, clientX: 0, clientY: 0 }, action);

    // Otherwise Analyze, Locate and Remove each arm a ghost drag.
    expect(store.startDrag).not.toHaveBeenCalled();
    expect(prevented).not.toHaveBeenCalled();
  });
});
