import { computed, type Ref } from 'vue';
import type { ParsedSession } from '@renderer/stores/session';
import { buildTimeline } from '@renderer/utils/sessionCore';
import { PITCH_RANGE_OPTIONS } from '@renderer/stores/settings';
import type { Clip, LoadedSpan, NudgeSpan, DeckLanes, MasterLanes } from '@renderer/utils/types';

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
  nameForPath: (path: string) => string = defaultNameForPath,
  gridForPath: (path: string) => { bpm: number; beatOffsetSec: number } | null = () => null
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
        masterLanes: { gain: [], xfader: [] },
        deckNudges: {}
      };
    }
    return buildTimeline(
      session.value.events,
      session.value.durationMs,
      PITCH_RANGE_OPTIONS,
      nameForPath,
      gridForPath
    );
  });

  const clips = computed(() => built.value.clips);
  const loadedSpans = computed(() => built.value.loadedSpans);
  const deckLanes = computed(() => built.value.deckLanes);
  const masterLanes = computed(() => built.value.masterLanes);
  const deckNudges = computed(() => built.value.deckNudges);

  return { clips, loadedSpans, deckLanes, masterLanes, deckNudges };
}
