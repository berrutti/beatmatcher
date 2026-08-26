import type { DeckLaneKey } from '@renderer/utils/laneSelection';
import { ROW_H } from '@renderer/utils/timelineDraw';

export const DEFAULT_LANE_HEIGHT = 96;
export const MIN_LANE_HEIGHT = 28;
const MAX_LANE_HEIGHT = 240;

export const DEFAULT_WAVEFORM_HEIGHT = ROW_H;
export const MIN_WAVEFORM_HEIGHT = 40;
const MAX_WAVEFORM_HEIGHT = 240;

export type StoredLaneHeights = Record<string, unknown>;

export function clampLaneHeight(height: number): number {
  return Math.min(MAX_LANE_HEIGHT, Math.max(MIN_LANE_HEIGHT, height));
}

export function clampWaveformHeight(height: number): number {
  return Math.min(MAX_WAVEFORM_HEIGHT, Math.max(MIN_WAVEFORM_HEIGHT, height));
}

// Keyed by deck as well as lane: every separator on screen is its own drag, so
// two decks showing the same lane size independently.
function slot(deck: string, key: DeckLaneKey | 'waveform'): string {
  return `${deck}:${key}`;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function storedHeight(stored: unknown, key: string): number | null {
  if (!isRecord(stored)) return null;
  const height = stored[key];
  return typeof height === 'number' && Number.isFinite(height) ? height : null;
}

function withHeight(stored: unknown, key: string, height: number): StoredLaneHeights {
  return { ...(isRecord(stored) ? stored : {}), [key]: height };
}

export function laneHeightFor(stored: unknown, deck: string, key: DeckLaneKey): number {
  const height = storedHeight(stored, slot(deck, key));
  return height === null ? DEFAULT_LANE_HEIGHT : clampLaneHeight(height);
}

export function withLaneHeight(
  stored: unknown,
  deck: string,
  key: DeckLaneKey,
  height: number
): StoredLaneHeights {
  return withHeight(stored, slot(deck, key), clampLaneHeight(height));
}

export function waveformHeightFor(stored: unknown, deck: string): number {
  const height = storedHeight(stored, slot(deck, 'waveform'));
  return height === null ? DEFAULT_WAVEFORM_HEIGHT : clampWaveformHeight(height);
}

export function withWaveformHeight(
  stored: unknown,
  deck: string,
  height: number
): StoredLaneHeights {
  return withHeight(stored, slot(deck, 'waveform'), clampWaveformHeight(height));
}
