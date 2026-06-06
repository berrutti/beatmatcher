import { describe, it, expect, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));
vi.mock('@tauri-apps/plugin-store', () => ({ load: vi.fn() }));

import { buildClips } from '../useSessionTimeline';
import type { SessionEvent } from '@renderer/stores/session';

const name = (path: string) => path.split('/').pop()?.replace(/\.[^.]+$/, '') ?? path;

function ev(overrides: Partial<SessionEvent> & { elapsed_ms: number; type: string }): SessionEvent {
  return overrides as SessionEvent;
}

describe('buildClips', () => {
  describe('basic play/stop', () => {
    it('creates a clip from play to stop', () => {
      const events = [
        ev({ elapsed_ms: 0, type: 'load_track', deck: 'A', path: '/tracks/song.mp3' }),
        ev({ elapsed_ms: 1000, type: 'play', deck: 'A' }),
        ev({ elapsed_ms: 5000, type: 'stop', deck: 'A', cue_point_sec: 4.0 }),
      ];
      const { clips } = buildClips(events, name);
      expect(clips).toHaveLength(1);
      expect(clips[0]).toMatchObject({
        deck: 'A',
        sessionStartMs: 1000,
        sessionEndMs: 5000,
        trackPath: '/tracks/song.mp3',
        trackName: 'song',
        trackStartSec: 0,
        playbackRate: 1,
      });
    });

    it('ignores play events when clip is already open', () => {
      const events = [
        ev({ elapsed_ms: 0, type: 'load_track', deck: 'A', path: '/t/a.mp3' }),
        ev({ elapsed_ms: 100, type: 'play', deck: 'A' }),
        ev({ elapsed_ms: 200, type: 'play', deck: 'A' }),
        ev({ elapsed_ms: 500, type: 'stop', deck: 'A' }),
      ];
      const { clips } = buildClips(events, name);
      expect(clips).toHaveLength(1);
      expect(clips[0].sessionStartMs).toBe(100);
    });

    it('finalizes clip at session end if still playing', () => {
      const events = [
        ev({ elapsed_ms: 0, type: 'load_track', deck: 'A', path: '/t/a.mp3' }),
        ev({ elapsed_ms: 500, type: 'play', deck: 'A' }),
        ev({ elapsed_ms: 3000, type: 'recording_stop', deck: 'A' }),
      ];
      const { clips } = buildClips(events, name);
      expect(clips).toHaveLength(1);
      expect(clips[0].sessionEndMs).toBe(3000);
    });
  });

  describe('seek', () => {
    it('splits clip at seek and starts new one', () => {
      const events = [
        ev({ elapsed_ms: 0, type: 'load_track', deck: 'A', path: '/t/a.mp3' }),
        ev({ elapsed_ms: 0, type: 'play', deck: 'A' }),
        ev({ elapsed_ms: 2000, type: 'seek', deck: 'A', sec: 10 }),
        ev({ elapsed_ms: 4000, type: 'stop', deck: 'A' }),
      ];
      const { clips } = buildClips(events, name);
      expect(clips).toHaveLength(2);
      expect(clips[0]).toMatchObject({ sessionStartMs: 0, sessionEndMs: 2000, trackStartSec: 0 });
      expect(clips[1]).toMatchObject({ sessionStartMs: 2000, sessionEndMs: 4000, trackStartSec: 10 });
    });
  });

  describe('loop iterations', () => {
    it('renders one clip per loop iteration', () => {
      const events = [
        ev({ elapsed_ms: 0, type: 'load_track', deck: 'A', path: '/t/a.mp3' }),
        ev({ elapsed_ms: 0, type: 'play', deck: 'A' }),
        ev({ elapsed_ms: 2000, type: 'loop_out', deck: 'A', start_sec: 4, end_sec: 6 }),
        ev({ elapsed_ms: 8000, type: 'exit_loop', deck: 'A' }),
        ev({ elapsed_ms: 10000, type: 'stop', deck: 'A' }),
      ];
      const { clips } = buildClips(events, name);
      const loopClips = clips.filter((c) => c.trackStartSec === 4);
      expect(loopClips.length).toBeGreaterThanOrEqual(3);
      for (const c of loopClips) {
        expect(c.trackStartSec).toBe(4);
      }
    });

    it('loop clip duration matches loop region duration', () => {
      const events = [
        ev({ elapsed_ms: 0, type: 'load_track', deck: 'A', path: '/t/a.mp3' }),
        ev({ elapsed_ms: 0, type: 'play', deck: 'A' }),
        ev({ elapsed_ms: 1000, type: 'loop_out', deck: 'A', start_sec: 2, end_sec: 4 }),
        ev({ elapsed_ms: 5000, type: 'exit_loop', deck: 'A' }),
        ev({ elapsed_ms: 6000, type: 'stop', deck: 'A' }),
      ];
      const { clips } = buildClips(events, name);
      const loopClips = clips.filter((c) => c.trackStartSec === 2);
      const loopDurMs = (4 - 2) / 1 * 1000;
      const fullIterations = loopClips.filter(
        (c) => Math.abs(c.sessionEndMs - c.sessionStartMs - loopDurMs) < 1
      );
      expect(fullIterations.length).toBe(2);
    });

    it('partial final iteration ends at exit time', () => {
      const events = [
        ev({ elapsed_ms: 0, type: 'load_track', deck: 'A', path: '/t/a.mp3' }),
        ev({ elapsed_ms: 0, type: 'play', deck: 'A' }),
        ev({ elapsed_ms: 0, type: 'loop_out', deck: 'A', start_sec: 0, end_sec: 2 }),
        ev({ elapsed_ms: 5000, type: 'exit_loop', deck: 'A' }),
        ev({ elapsed_ms: 6000, type: 'stop', deck: 'A' }),
      ];
      const { clips } = buildClips(events, name);
      const loopClips = clips.filter((c) => c.sessionStartMs < 5000);
      const lastLoop = loopClips[loopClips.length - 1];
      expect(lastLoop.sessionEndMs).toBeLessThanOrEqual(5000);
    });

    it('loop_in while looping exits the loop and continues', () => {
      const events = [
        ev({ elapsed_ms: 0, type: 'load_track', deck: 'A', path: '/t/a.mp3' }),
        ev({ elapsed_ms: 0, type: 'play', deck: 'A' }),
        ev({ elapsed_ms: 0, type: 'loop_out', deck: 'A', start_sec: 1, end_sec: 3 }),
        ev({ elapsed_ms: 6000, type: 'loop_in', deck: 'A' }),
        ev({ elapsed_ms: 9000, type: 'stop', deck: 'A' }),
      ];
      const { clips } = buildClips(events, name);
      const postLoop = clips.find((c) => c.sessionStartMs === 6000);
      expect(postLoop).toBeDefined();
      expect(postLoop!.sessionEndMs).toBe(9000);
    });

    it('reloop re-enters loop region', () => {
      const events = [
        ev({ elapsed_ms: 0, type: 'load_track', deck: 'A', path: '/t/a.mp3' }),
        ev({ elapsed_ms: 0, type: 'play', deck: 'A' }),
        ev({ elapsed_ms: 0, type: 'loop_out', deck: 'A', start_sec: 0, end_sec: 2 }),
        ev({ elapsed_ms: 4000, type: 'exit_loop', deck: 'A' }),
        ev({ elapsed_ms: 5000, type: 'reloop', deck: 'A' }),
        ev({ elapsed_ms: 7000, type: 'exit_loop', deck: 'A' }),
        ev({ elapsed_ms: 8000, type: 'stop', deck: 'A' }),
      ];
      const { clips } = buildClips(events, name);
      const reloopClips = clips.filter(
        (c) => c.sessionStartMs >= 5000 && c.sessionStartMs < 7000 && c.trackStartSec === 0
      );
      expect(reloopClips.length).toBeGreaterThanOrEqual(1);
    });
  });

  describe('deck_snapshot with loop_active (bug fix: cue_point_sec fallback)', () => {
    it('uses cue_point_sec as loop start when loop_start_sec absent', () => {
      const events = [
        ev({
          elapsed_ms: 0,
          type: 'deck_snapshot',
          deck: 'A',
          path: '/t/a.mp3',
          position_sec: 4,
          cue_point_sec: 4,
          is_playing: true,
          loop_active: true,
          loop_end_sec: 6,
          playback_rate: 1,
        }),
        ev({ elapsed_ms: 4000, type: 'stop', deck: 'A' }),
      ];
      const { clips } = buildClips(events, name);
      const loopClips = clips.filter((c) => c.trackStartSec === 4);
      expect(loopClips.length).toBeGreaterThanOrEqual(2);
    });

    it('loop start is always cue_point_sec (no separate loop_start_sec field exists)', () => {
      // deck_snapshot uses cue_point_sec as loop start because cue_point IS the loop start by invariant
      const events = [
        ev({
          elapsed_ms: 0,
          type: 'deck_snapshot',
          deck: 'A',
          path: '/t/a.mp3',
          position_sec: 4,
          cue_point_sec: 4,
          is_playing: true,
          loop_active: true,
          loop_end_sec: 8,
          playback_rate: 1,
        }),
        ev({ elapsed_ms: 8000, type: 'stop', deck: 'A' }),
      ];
      const { clips } = buildClips(events, name);
      const loopClips = clips.filter((c) => c.trackStartSec === 4);
      expect(loopClips.length).toBeGreaterThanOrEqual(2);
    });

    it('snapshot without loop_active starts a regular clip', () => {
      const events = [
        ev({
          elapsed_ms: 0,
          type: 'deck_snapshot',
          deck: 'A',
          path: '/t/a.mp3',
          position_sec: 10,
          cue_point_sec: 10,
          is_playing: true,
          loop_active: false,
          playback_rate: 1,
        }),
        ev({ elapsed_ms: 2000, type: 'stop', deck: 'A' }),
      ];
      const { clips } = buildClips(events, name);
      expect(clips).toHaveLength(1);
      expect(clips[0]).toMatchObject({ sessionStartMs: 0, sessionEndMs: 2000, trackStartSec: 10 });
    });
  });

  describe('load_track', () => {
    it('finalizes prior clip when new track is loaded', () => {
      const events = [
        ev({ elapsed_ms: 0, type: 'load_track', deck: 'A', path: '/t/a.mp3' }),
        ev({ elapsed_ms: 0, type: 'play', deck: 'A' }),
        ev({ elapsed_ms: 3000, type: 'load_track', deck: 'A', path: '/t/b.mp3' }),
        ev({ elapsed_ms: 4000, type: 'play', deck: 'A' }),
        ev({ elapsed_ms: 6000, type: 'stop', deck: 'A' }),
      ];
      const { clips } = buildClips(events, name);
      expect(clips).toHaveLength(2);
      expect(clips[0].trackName).toBe('a');
      expect(clips[1].trackName).toBe('b');
    });
  });

  describe('loaded spans', () => {
    it('spans the full time a track is loaded', () => {
      const events = [
        ev({ elapsed_ms: 0, type: 'load_track', deck: 'A', path: '/t/a.mp3' }),
        ev({ elapsed_ms: 5000, type: 'eject_track', deck: 'A' }),
      ];
      const { loadedSpans } = buildClips(events, name);
      expect(loadedSpans).toHaveLength(1);
      expect(loadedSpans[0]).toMatchObject({ startMs: 0, endMs: 5000 });
    });

    it('deck_snapshot initializes loaded span at time 0', () => {
      const events = [
        ev({
          elapsed_ms: 0,
          type: 'deck_snapshot',
          deck: 'A',
          path: '/t/a.mp3',
          position_sec: 0,
          is_playing: false,
          playback_rate: 1,
        }),
        ev({ elapsed_ms: 3000, type: 'eject_track', deck: 'A' }),
      ];
      const { loadedSpans } = buildClips(events, name);
      expect(loadedSpans).toHaveLength(1);
      expect(loadedSpans[0].startMs).toBe(0);
    });
  });

  describe('multi-deck independence', () => {
    it('clips from different decks do not interfere', () => {
      const events = [
        ev({ elapsed_ms: 0, type: 'load_track', deck: 'A', path: '/t/a.mp3' }),
        ev({ elapsed_ms: 0, type: 'load_track', deck: 'B', path: '/t/b.mp3' }),
        ev({ elapsed_ms: 0, type: 'play', deck: 'A' }),
        ev({ elapsed_ms: 1000, type: 'play', deck: 'B' }),
        ev({ elapsed_ms: 3000, type: 'stop', deck: 'A' }),
        ev({ elapsed_ms: 5000, type: 'stop', deck: 'B' }),
      ];
      const { clips } = buildClips(events, name);
      const deckA = clips.filter((c) => c.deck === 'A');
      const deckB = clips.filter((c) => c.deck === 'B');
      expect(deckA).toHaveLength(1);
      expect(deckB).toHaveLength(1);
      expect(deckA[0].sessionEndMs).toBe(3000);
      expect(deckB[0].sessionEndMs).toBe(5000);
    });
  });
});
