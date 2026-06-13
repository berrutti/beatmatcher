import { describe, it, expect } from 'vitest';

import { planTimelineClick, type ClickPlan } from '../timelineClick';

const NOTHING: ClickPlan = {
  seek: false,
  selectClip: false,
  clearClipSelection: false,
  selectLane: false,
  clearLaneSelection: false
};

describe('planTimelineClick', () => {
  describe('outside edit mode', () => {
    it('seeks on a clip without selecting it', () => {
      expect(planTimelineClick({ editMode: false, inLabelColumn: false, target: 'clip' })).toEqual({
        ...NOTHING,
        seek: true
      });
    });

    it('seeks on an empty clip band', () => {
      expect(
        planTimelineClick({ editMode: false, inLabelColumn: false, target: 'clip-band' })
      ).toEqual({ ...NOTHING, seek: true });
    });

    it('seeks on background', () => {
      expect(
        planTimelineClick({ editMode: false, inLabelColumn: false, target: 'background' })
      ).toEqual({ ...NOTHING, seek: true });
    });
  });

  describe('edit mode', () => {
    it('clicking a clip seeks, selects it, and closes the open lane', () => {
      expect(planTimelineClick({ editMode: true, inLabelColumn: false, target: 'clip' })).toEqual({
        ...NOTHING,
        seek: true,
        selectClip: true,
        clearLaneSelection: true
      });
    });

    it('clicking an empty clip band seeks, clears clip selection, and closes the open lane', () => {
      expect(
        planTimelineClick({ editMode: true, inLabelColumn: false, target: 'clip-band' })
      ).toEqual({
        ...NOTHING,
        seek: true,
        clearClipSelection: true,
        clearLaneSelection: true
      });
    });

    it('clicking an unselected lane seeks, selects the lane, and clears the clip selection', () => {
      expect(
        planTimelineClick({
          editMode: true,
          inLabelColumn: false,
          target: 'lane',
          laneAlreadySelected: false
        })
      ).toEqual({ ...NOTHING, seek: true, selectLane: true, clearClipSelection: true });
    });

    it('clicking the body of an already selected lane seeks and clears the clip selection', () => {
      expect(
        planTimelineClick({
          editMode: true,
          inLabelColumn: false,
          target: 'lane',
          laneAlreadySelected: true
        })
      ).toEqual({ ...NOTHING, seek: true, clearClipSelection: true });
    });

    it('clicking background seeks without touching selections', () => {
      expect(
        planTimelineClick({ editMode: true, inLabelColumn: false, target: 'background' })
      ).toEqual({ ...NOTHING, seek: true });
    });
  });

  describe('label column in edit mode (lane controls, never a seek target)', () => {
    it('selects an unselected lane without seeking and clears the clip selection', () => {
      expect(
        planTimelineClick({
          editMode: true,
          inLabelColumn: true,
          target: 'lane',
          laneAlreadySelected: false
        })
      ).toEqual({ ...NOTHING, selectLane: true, clearClipSelection: true });
    });

    it('deselects a selected lane without seeking', () => {
      expect(
        planTimelineClick({
          editMode: true,
          inLabelColumn: true,
          target: 'lane',
          laneAlreadySelected: true
        })
      ).toEqual({ ...NOTHING, clearLaneSelection: true });
    });

    it('does nothing on other targets', () => {
      expect(planTimelineClick({ editMode: true, inLabelColumn: true, target: 'clip' })).toEqual(
        NOTHING
      );
      expect(
        planTimelineClick({ editMode: true, inLabelColumn: true, target: 'background' })
      ).toEqual(NOTHING);
    });
  });

  it('label column outside edit mode still seeks (existing scrub behavior)', () => {
    expect(planTimelineClick({ editMode: false, inLabelColumn: true, target: 'lane' })).toEqual({
      ...NOTHING,
      seek: true
    });
  });
});
