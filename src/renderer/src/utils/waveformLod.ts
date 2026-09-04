// A pan or zoom fires per wheel event, up to 120 Hz on a trackpad, and each one can
// reach for peaks the cache does not hold. The two values differ because the timeline
// asks once per visible clip where the edit view asks once.
export const DECK_LOD_DEBOUNCE_MS = 80;
export const TIMELINE_LOD_DEBOUNCE_MS = 150;
