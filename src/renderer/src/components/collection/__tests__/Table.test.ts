// @vitest-environment happy-dom
import { describe, it, expect, vi } from 'vitest';
import { mount } from '@vue/test-utils';
import Table from '../Table.vue';

describe('Table', () => {
  it('calls onHeaderContextmenu and prevents default when provided', () => {
    const onHeaderContextmenu = vi.fn();
    const wrapper = mount(Table, { props: { onHeaderContextmenu } });
    const headRow = wrapper.find('thead tr.table__head-row');
    const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
    headRow.element.dispatchEvent(event);
    expect(onHeaderContextmenu).toHaveBeenCalledTimes(1);
    expect(event.defaultPrevented).toBe(true);
  });

  it('does not prevent the native context menu when no handler is provided', () => {
    const wrapper = mount(Table);
    const headRow = wrapper.find('thead tr.table__head-row');
    const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
    headRow.element.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
  });

  it('forwards the thead element ref when theadRef is provided', () => {
    const theadRef = vi.fn();
    const wrapper = mount(Table, { props: { theadRef } });
    expect(theadRef).toHaveBeenCalledWith(wrapper.find('thead').element);
  });
});
