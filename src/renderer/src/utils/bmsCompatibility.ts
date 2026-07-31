import type { SessionEvent } from './types';

type ParamMapping = {
  slot: string;
  // Master-scope events carry no deck, and a deck-scope one is not portable without.
  scope: 'deck' | 'master';
  param: (event: SessionEvent) => string | undefined;
  value: (event: SessionEvent) => number | undefined;
};

// Version 1 named its control instead of addressing a slot. Every address maps onto the
// classic mixer, which a headerless session resolves to, so this renames over reinterprets.
const V1_PARAM_MAPPINGS: Record<string, ParamMapping> = {
  set_volume: {
    slot: 'fader',
    scope: 'deck',
    param: () => 'gain',
    value: (event) => event.gain
  },
  set_eq: {
    slot: 'eq',
    scope: 'deck',
    param: (event) => event.band,
    value: (event) => event.db
  },
  set_filter: {
    slot: 'filter',
    scope: 'deck',
    param: () => 'value',
    value: (event) => event.value
  },
  set_filter_active: {
    slot: 'filter',
    scope: 'deck',
    param: () => 'active',
    value: (event) => (event.active === undefined ? undefined : event.active ? 1 : 0)
  },
  set_master_gain: {
    slot: 'gain',
    scope: 'master',
    param: () => 'gain',
    value: (event) => event.gain
  }
};

function portedFromV1(event: SessionEvent): SessionEvent {
  const mapping = V1_PARAM_MAPPINGS[event.type];
  if (!mapping) return event;
  // A `"deck": null` in the file is no deck, which is what serde hands session-core.
  const deckScoped = typeof event.deck === 'string';
  if (mapping.scope === 'deck' ? !deckScoped : deckScoped) return event;
  const param = mapping.param(event);
  const value = mapping.value(event);
  if (param === undefined || value === undefined) return event;
  return { ...event, type: 'set_param', slot: mapping.slot, param, value };
}

// Duplicated from session-core's `port_events` because the editor writes the file it
// reads, and the timeline recognizes a param by its event type before any Rust call.
export function portEvents(events: SessionEvent[], fromVersion: number): SessionEvent[] {
  switch (fromVersion) {
    case 1:
      return events.map(portedFromV1);
    default:
      return events;
  }
}
