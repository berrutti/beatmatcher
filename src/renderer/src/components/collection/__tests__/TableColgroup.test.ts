import { describe, it, expect } from 'vitest';
import { mount } from '@vue/test-utils';
import TableColgroup from '../TableColgroup.vue';

describe('TableColgroup', () => {
  it('renders one col per field with the width string from getWidth, used as-is', () => {
    // happy-dom (the test environment) can't represent calc() in a style
    // attribute, even though real browsers do - a plain percentage still
    // proves getWidth's string is used verbatim rather than always having
    // 'px' appended, which is the actual behavior under test.
    const widths: Record<string, string> = { a: '100px', b: '50%', c: '50px' };
    const wrapper = mount(
      { template: '<table><TableColgroup :fields="fields" :get-width="getWidth" /></table>' },
      {
        global: { components: { TableColgroup } },
        data: () => ({
          fields: ['a', 'b', 'c'],
          getWidth: (field: string) => widths[field]
        })
      }
    );
    const cols = wrapper.findAll('col');
    expect(cols).toHaveLength(3);
    expect(cols[0].attributes('style')).toContain('width: 100px');
    expect(cols[1].attributes('style')).toContain('width: 50%');
    expect(cols[2].attributes('style')).toContain('width: 50px');
  });

  it('recreates the col element (not just patches its style attribute) when its width value changes', async () => {
    // table-layout: fixed can cache column widths from the initial layout
    // pass in some engines, silently ignoring a later in-place style
    // mutation on an existing <col> node. Forcing Vue to tear down and
    // recreate the element (via a key that includes the width) sidesteps
    // that instead of relying on the browser noticing the attribute change.
    const widths: Record<string, string> = { a: '100px' };
    const wrapper = mount(
      { template: '<table><TableColgroup :fields="fields" :get-width="getWidth" /></table>' },
      {
        global: { components: { TableColgroup } },
        data: () => ({
          fields: ['a'],
          getWidth: (field: string) => widths[field]
        })
      }
    );
    const firstEl = wrapper.find('col').element as HTMLElement & { _marker?: boolean };
    firstEl._marker = true;

    widths.a = '200px';
    await wrapper.setData({ getWidth: (field: string) => widths[field] });

    const secondEl = wrapper.find('col').element as HTMLElement & { _marker?: boolean };
    expect(secondEl._marker).toBeUndefined();
    expect(secondEl.getAttribute('style')).toContain('width: 200px');
  });

  it('renders no cols for an empty field list', () => {
    const wrapper = mount(
      { template: '<table><TableColgroup :fields="fields" :get-width="getWidth" /></table>' },
      {
        global: { components: { TableColgroup } },
        data: () => ({
          fields: [],
          getWidth: () => '0px'
        })
      }
    );
    expect(wrapper.findAll('col')).toHaveLength(0);
  });
});
