import { computed, type Ref } from 'vue';
import type { ParsedSession } from '@renderer/stores/session';
import { buildTimeline } from '@renderer/utils/sessionCore';

export type Clip = {
  deck: string;
  sessionStartMs: number;
  sessionEndMs: number;
  trackPath: string;
  trackName: string;
  trackStartSec: number;
  playbackRate: number;
  // Clips emitted together form one editable unit: loop iterations share a
  // blockId; a regular play segment is a block of its own.
  blockId: number;
  loop: { startSec: number; endSec: number } | null;
};

export type LoadedSpan = {
  deck: string;
  trackPath: string;
  trackName: string;
  startMs: number;
  endMs: number;
};

export type LanePoint = { ms: number; value: number };

export type FilterActiveSpan = { startMs: number; endMs: number };

export type NudgeSpan = { startMs: number; endMs: number; percent: number };

export type DeckLanes = {
  gain: LanePoint[];
  eqLow: LanePoint[];
  eqMid: LanePoint[];
  eqHigh: LanePoint[];
  filter: LanePoint[];
  rate: LanePoint[];
  rateMin: number;
  rateMax: number;
  filterActive: FilterActiveSpan[];
};

export type MasterLanes = {
  gain: LanePoint[];
};

export const DEFAULT_GAIN = 1;
export const DEFAULT_EQ_DB = 0;
export const DEFAULT_FILTER_VALUE = 0;
export const DEFAULT_RATE = 1;

export type { ParsedSession };

function defaultNameForPath(path: string): string {
  return (
    path
      .split('/')
      .pop()
      ?.replace(/\.[^.]+$/, '') ?? path
  );
}

export function useSessionTimeline(
  session: Ref<ParsedSession | null>,
  nameForPath: (path: string) => string = defaultNameForPath
) {
  const built = computed<{
    clips: Clip[];
    loadedSpans: LoadedSpan[];
    deckLanes: Record<string, DeckLanes>;
    masterLanes: MasterLanes;
    deckNudges: Record<string, NudgeSpan[]>;
  }>(() => {
    if (!session.value) {
      return {
        clips: [],
        loadedSpans: [],
        deckLanes: {},
        masterLanes: { gain: [] },
        deckNudges: {}
      };
    }
    return buildTimeline(session.value.events, session.value.durationMs, nameForPath);
  });

  const clips = computed(() => built.value.clips);
  const loadedSpans = computed(() => built.value.loadedSpans);
  const deckLanes = computed(() => built.value.deckLanes);
  const masterLanes = computed(() => built.value.masterLanes);
  const deckNudges = computed(() => built.value.deckNudges);

  return { clips, loadedSpans, deckLanes, masterLanes, deckNudges };
}
