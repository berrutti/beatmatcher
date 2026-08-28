export type GhostBox = { left: number; top: number; width: number; height: number };
export type LandingSpot = { left: number; top: number };
export type Landing = { left: number; top: number; scale: number; opacity: number };

const SWALLOW_SCALE = 0.2;

// How long a released ghost takes to reach where it landed. The deck takes the
// track on the same beat, so the name changes as the ghost arrives rather than
// twice: once on release and again when the decode finishes.
export const DROP_LANDING_MS = 160;

// Where inside the row the pointer went down. Holding the ghost by this instead
// of by its centre keeps it over the row it was taken from, so a drag straight
// down never slides sideways and the release travels back the way it came.
export function grabOffset(row: GhostBox, clientX: number, clientY: number) {
  return { x: clientX - row.left, y: clientY - row.top };
}

// Where a released ghost travels before it disappears: into the deck that took
// the track, or back to the row it was picked up from when none did.
export function ghostLanding(release: {
  anchor: { x: number; y: number };
  origin: LandingSpot;
  target: GhostBox | null;
}): Landing {
  if (!release.target) {
    return { ...release.origin, scale: 1, opacity: 0 };
  }
  const { left, top, width, height } = release.target;
  return {
    left: left + width / 2 - release.anchor.x,
    top: top + height / 2 - release.anchor.y,
    scale: SWALLOW_SCALE,
    opacity: 0
  };
}
