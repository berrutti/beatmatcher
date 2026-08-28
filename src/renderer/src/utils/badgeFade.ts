export const BADGE_FADE_MS = 140;

export type Badge = { label: string; solo: boolean };

export type BadgeFade = { visible: boolean; badge: Badge | null; changedAtMs: number };

export function updateBadgeFade(
  previous: BadgeFade | undefined,
  badge: Badge | null,
  nowMs: number
): BadgeFade {
  const visible = badge !== null;
  if (previous === undefined) {
    return { visible, badge, changedAtMs: visible ? nowMs : nowMs - BADGE_FADE_MS };
  }
  return {
    visible,
    badge: badge ?? previous.badge,
    changedAtMs: visible === previous.visible ? previous.changedAtMs : nowMs
  };
}

function progress(fade: BadgeFade, nowMs: number): number {
  return Math.min(1, Math.max(0, (nowMs - fade.changedAtMs) / BADGE_FADE_MS));
}

export function badgeAlpha(fade: BadgeFade, nowMs: number): number {
  const eased = easeInOut(progress(fade, nowMs));
  return fade.visible ? eased : 1 - eased;
}

export function badgeFading(fade: BadgeFade, nowMs: number): boolean {
  return progress(fade, nowMs) < 1;
}

function easeInOut(value: number): number {
  return value < 0.5 ? 2 * value * value : 1 - 2 * (1 - value) * (1 - value);
}
