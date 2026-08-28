// A deck marks the element a dropped track should appear to fall into. The drag
// asks for it by this attribute rather than reaching for a class of the deck's
// own markup, so a deck can move or rename that element freely.
export const DROP_LANDING_ATTRIBUTE = 'data-drop-landing';

export function dropLandingWithin(deck: HTMLElement): HTMLElement | null {
  const marked = deck.querySelector(`[${DROP_LANDING_ATTRIBUTE}]`);
  return marked instanceof HTMLElement ? marked : deck;
}
