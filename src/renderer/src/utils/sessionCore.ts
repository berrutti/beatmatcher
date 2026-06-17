// Thin wrapper over the session-core WASM module. The timeline derivation and
// all clip/lane edit ops are implemented once in Rust (session-core) and reached
// here as JSON-in/JSON-out calls, replacing the former TypeScript copies.
//
// `initSessionCore()` must be awaited once at app startup before any of the
// (synchronous) functions below are called; wasm-bindgen exports are sync only
// after the module has initialized.

import init, {
  buildTimeline as wasmBuildTimeline,
  blocksForDeck as wasmBlocksForDeck,
  blockBounds as wasmBlockBounds,
  moveTransportBlock as wasmMove,
  trimTransportBlock as wasmTrim,
  normalizeGestureSamples as wasmNormalize,
  decimateSteps as wasmDecimate,
  originalValueAt as wasmOriginalValueAt,
  spliceLaneEvents as wasmSplice,
  filterActiveAt as wasmFilterActiveAt,
  toggleFilterActiveRange as wasmToggleFilter,
  deleteFilterActiveSpan as wasmDeleteFilterSpan,
  resizeFilterActiveSpan as wasmResizeFilterSpan,
  nudgeValueAt as wasmNudgeValueAt,
  paintNudgeRange as wasmPaintNudge,
  deleteNudgeRange as wasmDeleteNudge,
  relocateEventPaths as wasmRelocate
} from '@core/session_core.js';
import wasmUrl from '@core/session_core_bg.wasm?url';

import type { SessionEvent } from '@renderer/stores/session';
import type {
  Clip,
  LoadedSpan,
  DeckLanes,
  MasterLanes,
  NudgeSpan,
  LanePoint
} from '@renderer/composables/useSessionTimeline';
import type { EditableLaneKey, TransportBlock } from '@renderer/utils/types';

let initPromise: Promise<void> | null = null;

async function loadWasm(): Promise<void> {
  await init(wasmUrl);
}

export function initSessionCore(): Promise<void> {
  if (!initPromise) initPromise = loadWasm();
  return initPromise;
}

const parse = <T>(json: string): T => JSON.parse(json) as T;

// Track display names are derived here (the Rust core returns paths only), so
// the editor's collection-aware naming stays on the JS side.
type RawClip = Omit<Clip, 'trackName'>;
type RawLoadedSpan = Omit<LoadedSpan, 'trackName'>;

// Clips, loaded spans, and automation lanes in a single boundary crossing: the
// editor needs all of them on every event change, so deriving them together
// serializes the event list once instead of once per builder. Track display
// names are still resolved here (the Rust core returns paths only).
export function buildTimeline(
  events: SessionEvent[],
  durationMs: number,
  pitchOptions: readonly number[],
  nameForPath: (path: string) => string
): {
  clips: Clip[];
  loadedSpans: LoadedSpan[];
  deckLanes: Record<string, DeckLanes>;
  masterLanes: MasterLanes;
  deckNudges: Record<string, NudgeSpan[]>;
} {
  const raw = parse<{
    clips: RawClip[];
    loadedSpans: RawLoadedSpan[];
    deckLanes: Record<string, DeckLanes>;
    masterLanes: MasterLanes;
    deckNudges: Record<string, NudgeSpan[]>;
  }>(wasmBuildTimeline(JSON.stringify(events), durationMs, new Float64Array(pitchOptions)));
  return {
    clips: raw.clips.map((clip) => ({ ...clip, trackName: nameForPath(clip.trackPath) })),
    loadedSpans: raw.loadedSpans.map((span) => ({
      ...span,
      trackName: nameForPath(span.trackPath)
    })),
    deckLanes: raw.deckLanes,
    masterLanes: raw.masterLanes,
    deckNudges: raw.deckNudges
  };
}

export function blocksForDeck(clips: Clip[], deck: string): TransportBlock[] {
  return parse(wasmBlocksForDeck(JSON.stringify(clips), deck));
}

export function blockBounds(
  events: SessionEvent[],
  clips: Clip[],
  block: TransportBlock
): { minStartMs: number; maxEndMs: number } | null {
  // The Rust side has no JSON form for an open-ended (Infinity) right bound, so
  // it sends null; restore Infinity here.
  const raw = parse<{ minStartMs: number; maxEndMs: number | null } | null>(
    wasmBlockBounds(JSON.stringify(events), JSON.stringify(clips), JSON.stringify(block))
  );
  if (!raw) return null;
  return { minStartMs: raw.minStartMs, maxEndMs: raw.maxEndMs ?? Infinity };
}

export function moveTransportBlock(
  events: SessionEvent[],
  clips: Clip[],
  block: TransportBlock,
  deltaMs: number
): { events: SessionEvent[]; appliedDeltaMs: number } {
  const result = parse<{ events: SessionEvent[]; appliedDeltaMs: number }>(
    wasmMove(JSON.stringify(events), JSON.stringify(clips), JSON.stringify(block), deltaMs)
  );
  // No-op: preserve the input reference so callers relying on reference
  // equality (applyEdit's dirty/undo check) can skip a no-op edit.
  if (result.appliedDeltaMs === 0) return { events, appliedDeltaMs: 0 };
  return result;
}

export function trimTransportBlock(
  events: SessionEvent[],
  clips: Clip[],
  block: TransportBlock,
  edge: 'start' | 'end',
  newMs: number
): { events: SessionEvent[]; appliedMs: number } {
  const result = parse<{ events: SessionEvent[]; appliedMs: number }>(
    wasmTrim(JSON.stringify(events), JSON.stringify(clips), JSON.stringify(block), edge, newMs)
  );
  const unchangedMs = edge === 'start' ? block.startMs : block.endMs;
  // No-op: preserve the input reference, same reasoning as moveTransportBlock.
  if (result.appliedMs === unchangedMs) return { events, appliedMs: result.appliedMs };
  return result;
}

// Lane ops take a lane key plus the rate range (only used by the rate lane;
// other lanes ignore it). Defaults match the smallest pitch-range step.
export function spliceLaneEvents(
  events: SessionEvent[],
  laneKey: EditableLaneKey,
  deck: string,
  t0: number,
  t1: number,
  points: LanePoint[],
  rateMin = 0.92,
  rateMax = 1.08
): SessionEvent[] {
  return parse(
    wasmSplice(
      JSON.stringify(events),
      laneKey,
      deck,
      t0,
      t1,
      JSON.stringify(points),
      rateMin,
      rateMax
    )
  );
}

export function originalValueAt(
  events: SessionEvent[],
  laneKey: EditableLaneKey,
  deck: string,
  ms: number,
  rateMin = 0.92,
  rateMax = 1.08
): number {
  return wasmOriginalValueAt(JSON.stringify(events), laneKey, deck, ms, rateMin, rateMax);
}

export function filterActiveAt(
  events: SessionEvent[],
  deck: string,
  ms: number,
  inclusive = true
): boolean {
  return wasmFilterActiveAt(JSON.stringify(events), deck, ms, inclusive);
}

export function toggleFilterActiveRange(
  events: SessionEvent[],
  deck: string,
  t0: number,
  t1: number
): SessionEvent[] {
  return parse(wasmToggleFilter(JSON.stringify(events), deck, t0, t1));
}

export function deleteFilterActiveSpan(
  events: SessionEvent[],
  deck: string,
  startMs: number,
  endMs: number
): SessionEvent[] {
  return parse(wasmDeleteFilterSpan(JSON.stringify(events), deck, startMs, endMs));
}

export function resizeFilterActiveSpan(
  events: SessionEvent[],
  deck: string,
  startMs: number,
  endMs: number,
  edge: 'start' | 'end',
  newMs: number,
  durationMs: number
): SessionEvent[] {
  return parse(
    wasmResizeFilterSpan(JSON.stringify(events), deck, startMs, endMs, edge, newMs, durationMs)
  );
}

export function nudgeValueAt(
  events: SessionEvent[],
  deck: string,
  ms: number,
  inclusive = true
): number {
  return wasmNudgeValueAt(JSON.stringify(events), deck, ms, inclusive);
}

export function paintNudgeRange(
  events: SessionEvent[],
  deck: string,
  t0: number,
  t1: number,
  percent: number
): SessionEvent[] {
  return parse(wasmPaintNudge(JSON.stringify(events), deck, t0, t1, percent));
}

export function deleteNudgeRange(
  events: SessionEvent[],
  deck: string,
  t0: number,
  t1: number
): SessionEvent[] {
  return parse(wasmDeleteNudge(JSON.stringify(events), deck, t0, t1));
}

export function relocateEventPaths(
  events: SessionEvent[],
  mapping: Record<string, string>
): SessionEvent[] {
  return parse(wasmRelocate(JSON.stringify(events), JSON.stringify(mapping)));
}

export function normalizeGestureSamples(samples: LanePoint[]): LanePoint[] {
  return parse(wasmNormalize(JSON.stringify(samples)));
}

export function decimateSteps(points: LanePoint[], epsilon: number): LanePoint[] {
  return parse(wasmDecimate(JSON.stringify(points), epsilon));
}
