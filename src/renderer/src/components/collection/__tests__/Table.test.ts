import { describe, it, expect, vi } from 'vitest';
import { mount } from '@vue/test-utils';
import Table from '../Table.vue';

describe('Table', () => {
  it('always renders the table element at 100% width', () => {
    const wrapper = mount(Table);
    expect(wrapper.find('table').attributes('style')).toContain('width: 100%');
  });

  it('renders colgroup slot content inside a colgroup element', () => {
    const wrapper = mount(Table, {
      slots: { colgroup: '<col style="width: 50px" />' }
    });
    expect(wrapper.find('colgroup col').attributes('style')).toContain('width: 50px');
  });

  it('renders header slot content inside the head row', () => {
    const wrapper = mount(Table, {
      slots: { header: '<th class="my-th">Title</th>' }
    });
    const headRow = wrapper.find('thead tr.table__head-row');
    expect(headRow.exists()).toBe(true);
    expect(headRow.find('.my-th').text()).toBe('Title');
  });

  it('renders default slot content inside the body', () => {
    const wrapper = mount(Table, {
      slots: { default: '<tr class="row"><td>a</td></tr>' }
    });
    expect(wrapper.find('tbody tr.row').exists()).toBe(true);
  });

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
