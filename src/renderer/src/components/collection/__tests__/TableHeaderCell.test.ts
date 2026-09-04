// @vitest-environment happy-dom
import { describe, it, expect } from 'vitest';
import { mount } from '@vue/test-utils';
import TableHeaderCell from '../TableHeaderCell.vue';

describe('TableHeaderCell', () => {
  it('left-aligns by default', () => {
    const wrapper = mount(TableHeaderCell);
    expect(wrapper.classes()).not.toContain('table__header-cell--right');
  });

  it('applies the right-align modifier when align is "right"', () => {
    const wrapper = mount(TableHeaderCell, { props: { align: 'right' } });
    expect(wrapper.classes()).toContain('table__header-cell--right');
  });
});
