import { MASTER_ROW_ID, type EditableLaneKey } from '@renderer/utils/types';
import { ROW_H } from '@renderer/utils/timelineDraw';

export const DEFAULT_LANE_HEIGHT = 96;
// The master lanes plot one value line, not a curve read against a waveform, so
// they need a fraction of the height a deck lane does.
export const DEFAULT_MASTER_LANE_HEIGHT = 32;
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

// Keyed by row as well as lane: every separator on screen is its own drag, so
// two rows showing the same lane size independently.
function slot(row: string, key: EditableLaneKey | 'waveform'): string {
  return `${row}:${key}`;
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

export function defaultLaneHeight(row: string): number {
  return row === MASTER_ROW_ID ? DEFAULT_MASTER_LANE_HEIGHT : DEFAULT_LANE_HEIGHT;
}

export function laneHeightFor(stored: unknown, row: string, key: EditableLaneKey): number {
  const height = storedHeight(stored, slot(row, key));
  return height === null ? defaultLaneHeight(row) : clampLaneHeight(height);
}

export function withLaneHeight(
  stored: unknown,
  deck: string,
  key: EditableLaneKey,
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
