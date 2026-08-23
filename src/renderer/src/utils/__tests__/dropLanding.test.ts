import { describe, it, expect } from 'vitest';
import { dropLandingWithin, DROP_LANDING_ATTRIBUTE } from '../dropLanding';

function deckWith(inner: string): HTMLElement {
  const deck = document.createElement('div');
  deck.dataset.deckId = 'A';
  deck.innerHTML = inner;
  return deck;
}

describe('a deck says where a dropped track lands', () => {
  it('picks the element the deck marked, not one named by its class', () => {
    const deck = deckWith(`<div><span ${DROP_LANDING_ATTRIBUTE} id="ring"></span></div>`);
    expect(dropLandingWithin(deck)?.id).toBe('ring');
  });

  it('falls back to the deck itself when it marks nothing', () => {
    const deck = deckWith('<div><span id="other"></span></div>');
    expect(dropLandingWithin(deck)).toBe(deck);
  });

  it('takes the first mark, so a deck cannot name two landings', () => {
    const deck = deckWith(
      `<span ${DROP_LANDING_ATTRIBUTE} id="first"></span><span ${DROP_LANDING_ATTRIBUTE} id="second"></span>`
    );
    expect(dropLandingWithin(deck)?.id).toBe('first');
  });
});
