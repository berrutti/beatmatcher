import type { MixerParamSpec } from '@renderer/utils/sessionCore';

type KnobScale = 'linear' | 'log' | 'centred';

// Times, frequencies and Q are heard in ratios, so a linear sweep would spend most of the
// knob on values that all sound alike.
const LOG_UNITS = new Set(['ms', 'hz', 'ratio']);

function scaleOf(spec: MixerParamSpec): KnobScale {
  if (spec.min < 0 && spec.max > 0) return 'centred';
  if (LOG_UNITS.has(spec.unit) && spec.min > 0) return 'log';
  return 'linear';
}

function clampTravel(travel: number): number {
  return Math.max(0, Math.min(1, travel));
}

export function knobTravel(spec: MixerParamSpec, value: number): number {
  const clamped = Math.max(spec.min, Math.min(spec.max, value));
  switch (scaleOf(spec)) {
    case 'centred':
      return clamped < 0 ? 0.5 * (1 - clamped / spec.min) : 0.5 + (0.5 * clamped) / spec.max;
    case 'log':
      return Math.log(clamped / spec.min) / Math.log(spec.max / spec.min);
    case 'linear':
      return (clamped - spec.min) / (spec.max - spec.min);
  }
}

function snappedToStep(spec: MixerParamSpec, value: number): number {
  const steps = Math.round((value - spec.min) / spec.step);
  const snapped = spec.min + steps * spec.step;
  return Math.max(spec.min, Math.min(spec.max, Number(snapped.toFixed(6))));
}

export function valueAtTravel(spec: MixerParamSpec, travel: number): number {
  const position = clampTravel(travel);
  switch (scaleOf(spec)) {
    case 'centred':
      return snappedToStep(
        spec,
        position < 0.5 ? spec.min * (1 - position / 0.5) : spec.max * ((position - 0.5) / 0.5)
      );
    case 'log':
      return snappedToStep(spec, spec.min * Math.pow(spec.max / spec.min, position));
    case 'linear':
      return snappedToStep(spec, spec.min + position * (spec.max - spec.min));
  }
}

export function knobOrigin(spec: MixerParamSpec): number {
  return scaleOf(spec) === 'centred' ? 0.5 : 0;
}
