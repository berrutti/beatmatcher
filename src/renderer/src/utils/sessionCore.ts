// Thin wrapper over the session-core WASM module. The timeline derivation and
// all clip/lane edit ops are implemented once in Rust (session-core) and reached
// here as JSON-in/JSON-out calls, replacing the former TypeScript copies.
//
// `initSessionCore()` must be awaited once at app startup before any of the
// (synchronous) functions below are called; wasm-bindgen exports are sync only
// after the module has initialized.

import init, {
  buildTimeline as wasmBuildTimeline,
  currentBeat as wasmCurrentBeat,
  blocksForDeck as wasmBlocksForDeck,
  blockBounds as wasmBlockBounds,
  moveTransportBlock as wasmMove,
  trimTransportBlock as wasmTrim,
  splitTransportBlock as wasmSplit,
  deleteTransportRanges as wasmDeleteRanges,
  normalizeGestureSamples as wasmNormalize,
  decimateSteps as wasmDecimate,
  spliceLaneEvents as wasmSplice,
  filterActiveAt as wasmFilterActiveAt,
  toggleFilterActiveRange as wasmToggleFilter,
  deleteFilterActiveSpan as wasmDeleteFilterSpan,
  resizeFilterActiveSpan as wasmResizeFilterSpan,
  moveFilterActiveSpan as wasmMoveFilterSpan,
  paintNudgeRange as wasmPaintNudge,
  deleteNudgeRange as wasmDeleteNudge,
  setRateAt as wasmSetRateAt,
  setRateSpan as wasmSetRateSpan,
  relocateEventPaths as wasmRelocate,
  bmsVersion as wasmBmsVersion,
  faderCurveGain as wasmFaderCurveGain,
  editConstants as wasmEditConstants,
  laneSpecs as wasmLaneSpecs,
  mixerParams as wasmMixerParams
} from '@core/session_core.js';
import wasmUrl from '@core/session_core_bg.wasm?url';

import type {
  SessionEvent,
  Clip,
  LoadedSpan,
  DeckLanes,
  MasterLanes,
  NudgeSpan,
  LanePoint,
  EditableLaneKey,
  TransportBlock
} from '@renderer/utils/types';

let initPromise: Promise<void> | null = null;

async function loadWasm(): Promise<void> {
  await init({ module_or_path: wasmUrl });
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
  nameForPath: (path: string) => string,
  gridForPath: (path: string) => { bpm: number; beatOffsetSec: number } | null
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
    // The beat grid (bpm + offset) is a property of the track, not of the
    // recording, so it is looked up by path from the track's saved grid (the
    // same source the edit view draws from). This keeps the session beats
    // aligned with the edit view for every clip, including older recordings
    // whose events never captured the offset. Recorded values stay as a
    // fallback for tracks missing from the collection.
    clips: raw.clips.map((clip) => {
      const grid = gridForPath(clip.trackPath);
      return {
        ...clip,
        trackName: nameForPath(clip.trackPath),
        bpm: grid?.bpm ?? clip.bpm,
        beatOffsetSec: grid?.beatOffsetSec ?? clip.beatOffsetSec
      };
    }),
    loadedSpans: raw.loadedSpans.map((span) => ({
      ...span,
      trackName: nameForPath(span.trackPath)
    })),
    deckLanes: raw.deckLanes,
    masterLanes: raw.masterLanes,
    deckNudges: raw.deckNudges
  };
}

export function currentBeat(positionSec: number, beatOffsetSec: number, bpm: number): number {
  return wasmCurrentBeat(positionSec, beatOffsetSec, bpm);
}

export function blocksForDeck(clips: Clip[], deck: string): TransportBlock[] {
  return parse(wasmBlocksForDeck(JSON.stringify(clips), deck));
}

export type BlockBounds = {
  minStartMs: number;
  maxEndMs: number;
  // Start-trim lower bound from the trim commit's own formula, so the drag
  // preview can never clamp differently.
  startTrimMinMs: number;
  minBlockMs: number;
};

export function blockBounds(
  events: SessionEvent[],
  clips: Clip[],
  block: TransportBlock
): BlockBounds | null {
  // The Rust side has no JSON form for an open-ended (Infinity) right bound, so
  // it sends null; restore Infinity here.
  const raw = parse<{
    minStartMs: number;
    maxEndMs: number | null;
    startTrimMinMs: number;
    minBlockMs: number;
  } | null>(wasmBlockBounds(JSON.stringify(events), JSON.stringify(clips), JSON.stringify(block)));
  if (!raw) return null;
  return {
    minStartMs: raw.minStartMs,
    maxEndMs: raw.maxEndMs ?? Infinity,
    startTrimMinMs: raw.startTrimMinMs,
    minBlockMs: raw.minBlockMs
  };
}

export function bmsVersion(): number {
  return wasmBmsVersion();
}

export function faderCurveGain(curve: string, position: number): number {
  return wasmFaderCurveGain(curve, position);
}

export type EditConstants = {
  eqMinDb: number;
  eqMaxDb: number;
  filterDeadZone: number;
  defaultMasterGain: number;
  minBlockMs: number;
  minGestureMs: number;
};

// Read on first use because a module-level `export const` cannot await and WASM is only
// initialized in the app's async init(). Memoized because the renderer reads these per draw.
let editConstantsCache: EditConstants | undefined;

export function editConstants(): EditConstants {
  const constants = editConstantsCache ?? parse<EditConstants>(wasmEditConstants());
  editConstantsCache = constants;
  return constants;
}

export type LaneSpec = {
  key: EditableLaneKey;
  min: number;
  max: number;
  defaultValue: number;
  epsilon: number;
  shortLabel: string;
  laneGroup: number;
  unit: LaneUnit;
};

type LaneUnit = 'db' | 'normalized' | 'bool' | 'ratio';

const laneSpecCache = new Map<string, Record<EditableLaneKey, LaneSpec>>();

// Keyed by mixer because a lane's range is a property of the manifest the
// session was recorded on, not of the lane.
export function laneSpecs(mixerId: string): Record<EditableLaneKey, LaneSpec> {
  const cached = laneSpecCache.get(mixerId);
  if (cached) return cached;
  const specs = parse<Record<EditableLaneKey, LaneSpec>>(wasmLaneSpecs(mixerId));
  laneSpecCache.set(mixerId, specs);
  return specs;
}

export type MixerParamSpec = {
  slot: string;
  param: string;
  label: string;
  shortLabel: string;
  min: number;
  max: number;
  defaultValue: number;
  step: number;
};

const mixerParamCache = new Map<string, Record<string, MixerParamSpec>>();

export function mixerParams(mixerId: string): Record<string, MixerParamSpec> {
  const cached = mixerParamCache.get(mixerId);
  if (cached) return cached;
  const specs = parse<Record<string, MixerParamSpec>>(wasmMixerParams(mixerId));
  mixerParamCache.set(mixerId, specs);
  return specs;
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

// Splits a block into two at splitMs, gaplessly (a stop immediately followed
// by a play, the right part resuming exactly the audio it already played). A
// no-op (returns the same events reference) if splitMs is within minBlockMs of
// either edge.
export function splitTransportBlock(
  events: SessionEvent[],
  clips: Clip[],
  block: TransportBlock,
  splitMs: number
): SessionEvent[] {
  const result = parse<SessionEvent[]>(
    wasmSplit(JSON.stringify(events), JSON.stringify(clips), JSON.stringify(block), splitMs)
  );
  return result.length === events.length ? events : result;
}

// A range covering a whole block deletes it, an edge range trims it, and an
// interior range splits the block (the right part keeps playing exactly the
// audio it played before).
export function deleteTransportRanges(
  events: SessionEvent[],
  clips: Clip[],
  ranges: { deck: string; startMs: number; endMs: number }[]
): SessionEvent[] {
  return parse(
    wasmDeleteRanges(JSON.stringify(events), JSON.stringify(clips), JSON.stringify(ranges))
  );
}

// Lane ops take a lane key plus the rate range (only used by the rate lane;
// other lanes ignore it). Defaults match the smallest pitch-range step.
export function spliceLaneEvents(
  events: SessionEvent[],
  laneKey: EditableLaneKey,
  mixerId: string,
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
      mixerId,
      deck,
      t0,
      t1,
      JSON.stringify(points),
      rateMin,
      rateMax
    )
  );
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

export function moveFilterActiveSpan(
  events: SessionEvent[],
  deck: string,
  startMs: number,
  endMs: number,
  deltaMs: number,
  durationMs: number
): SessionEvent[] {
  return parse(
    wasmMoveFilterSpan(JSON.stringify(events), deck, startMs, endMs, deltaMs, durationMs)
  );
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

// No-op (Rust sends null) returns the input reference so callers can skip it.
export function deleteNudgeRange(
  events: SessionEvent[],
  deck: string,
  t0: number,
  t1: number
): SessionEvent[] {
  const edited = parse<SessionEvent[] | null>(
    wasmDeleteNudge(JSON.stringify(events), deck, t0, t1)
  );
  return edited ?? events;
}

export function setRateAt(
  events: SessionEvent[],
  deck: string,
  ms: number,
  rate: number
): SessionEvent[] {
  return parse(wasmSetRateAt(JSON.stringify(events), deck, ms, rate));
}

export function setRateSpan(
  events: SessionEvent[],
  deck: string,
  startMs: number,
  endMs: number,
  rate: number
): SessionEvent[] {
  return parse(wasmSetRateSpan(JSON.stringify(events), deck, startMs, endMs, rate));
}

// No-op (Rust sends null) returns the input reference so callers can skip it.
export function relocateEventPaths(
  events: SessionEvent[],
  mapping: Record<string, string>
): SessionEvent[] {
  const edited = parse<SessionEvent[] | null>(
    wasmRelocate(JSON.stringify(events), JSON.stringify(mapping))
  );
  return edited ?? events;
}

export function normalizeGestureSamples(samples: LanePoint[]): LanePoint[] {
  return parse(wasmNormalize(JSON.stringify(samples)));
}

export function decimateSteps(points: LanePoint[], epsilon: number): LanePoint[] {
  return parse(wasmDecimate(JSON.stringify(points), epsilon));
}
