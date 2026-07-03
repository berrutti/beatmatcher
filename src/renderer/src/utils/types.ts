// One recorded .bms event, exactly as serialized (snake_case payload fields).
// Lives here rather than in the session store so the pure edit/timeline utils
// never import from a store (that made stores <-> utils circular).
export type SessionEvent = {
  elapsed_ms: number;
  type: string;
  deck?: string;
  path?: string;
  sec?: number;
  gain?: number;
  band?: string;
  db?: number;
  value?: number;
  active?: boolean;
  rate?: number;
  percent?: number;
  quantized?: boolean;
  beat_offset_sec?: number;
  start_sec?: number;
  end_sec?: number;
  is_playing?: boolean;
  position_sec?: number;
  cue_point_sec?: number;
  loop_active?: boolean;
  loop_end_sec?: number;
  bpm?: number;
  playback_rate?: number;
  duration?: number;
};

export type EditableLaneKey =
  'gain' | 'eqLow' | 'eqMid' | 'eqHigh' | 'filter' | 'rate' | 'masterGain';

// A user-draggable unit on the timeline: one regular play segment, or one run
// of loop iterations (which always moves as a whole). Derived from buildClips
// output, so every field reflects what the listener actually heard.
export type TransportBlock = {
  deck: string;
  blockId: number;
  startMs: number;
  endMs: number;
  trackPath: string;
  trackStartSec: number;
  playbackRate: number;
  loop: { startSec: number; endSec: number } | null;
};

export type WaveSegment = {
  wallStartMs: number;
  wallEndMs: number;
  trackStartSec: number;
  trackEndSec: number;
};

export type Clip = {
  // Clips emitted together form one editable unit: loop iterations share a blockId; a regular play segment is a block of its own.
  blockId: number;
  // Recorded beat grid in effect when the clip started; null bpm = draw no beats.
  bpm: number | null;
  // Constant-rate pieces of the clip (rate*nudge), each mapping a track-time window to a wall-time window. Drawing the waveform and beats per segment is  what keeps them stretched/compressed correctly across rate changes.
  waveSegments: WaveSegment[];
  beatOffsetSec: number | null;
  deck: string;
  loop: { startSec: number; endSec: number } | null;
  playbackRate: number;
  sessionEndMs: number;
  sessionStartMs: number;
  trackName: string;
  trackPath: string;
  trackStartSec: number;
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
