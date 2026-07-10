import { describe, it, expect } from 'vitest';
import { mount } from '@vue/test-utils';
import TableHeaderCell from '../TableHeaderCell.vue';

describe('TableHeaderCell', () => {
  it('renders a th with the slot content', () => {
    const wrapper = mount(TableHeaderCell, { slots: { default: 'BPM' } });
    expect(wrapper.element.tagName).toBe('TH');
    expect(wrapper.text()).toBe('BPM');
  });

  it('left-aligns by default', () => {
    const wrapper = mount(TableHeaderCell);
    expect(wrapper.classes()).not.toContain('table__header-cell--right');
  });

  it('applies the right-align modifier when align is "right"', () => {
    const wrapper = mount(TableHeaderCell, { props: { align: 'right' } });
    expect(wrapper.classes()).toContain('table__header-cell--right');
  });

  it('forwards extra attributes (class, data-*) to the th element', () => {
    const wrapper = mount(TableHeaderCell, {
      attrs: { class: 'extra-class', 'data-column-field': 'artist' }
    });
    expect(wrapper.classes()).toContain('extra-class');
    expect(wrapper.attributes('data-column-field')).toBe('artist');
  });
});
