// @vitest-environment happy-dom
import { describe, it, expect, vi } from 'vitest';
import { mount } from '@vue/test-utils';
import TableHeaderCells from '../TableHeaderCells.vue';

const LABELS: Record<string, string> = { title: 'Title', artist: 'Artist' };

function baseProps() {
  return {
    fields: ['title', 'artist'],
    getLabel: (field: string) => LABELS[field],
    draggingColumn: null,
    dropTargetColumn: null,
    isResizable: () => true,
    onColumnHeaderPointerDown: vi.fn(),
    onResizerPointerDown: vi.fn(),
    onAutoFitColumn: vi.fn()
  };
}

describe('TableHeaderCells', () => {
  it('renders one th per field with the label and data-column-field attribute', () => {
    const wrapper = mount(
      { template: '<table><thead><tr><TableHeaderCells v-bind="props" /></tr></thead></table>' },
      {
        global: { components: { TableHeaderCells } },
        data: () => ({ props: baseProps() })
      }
    );
    const ths = wrapper.findAll('th');
    expect(ths).toHaveLength(2);
    expect(ths[0].attributes('data-column-field')).toBe('title');
    expect(ths[0].text()).toContain('Title');
    expect(ths[1].attributes('data-column-field')).toBe('artist');
    expect(ths[1].text()).toContain('Artist');
  });

  it('applies the dragging class only to the field currently being dragged', () => {
    const wrapper = mount(
      { template: '<table><thead><tr><TableHeaderCells v-bind="props" /></tr></thead></table>' },
      {
        global: { components: { TableHeaderCells } },
        data: () => ({ props: { ...baseProps(), draggingColumn: 'artist' } })
      }
    );
    const ths = wrapper.findAll('th');
    expect(ths[0].classes()).not.toContain('table__header-cell--dragging');
    expect(ths[1].classes()).toContain('table__header-cell--dragging');
  });

  it('applies the drop-target class only to the field currently targeted', () => {
    const wrapper = mount(
      { template: '<table><thead><tr><TableHeaderCells v-bind="props" /></tr></thead></table>' },
      {
        global: { components: { TableHeaderCells } },
        data: () => ({ props: { ...baseProps(), dropTargetColumn: 'title' } })
      }
    );
    const ths = wrapper.findAll('th');
    expect(ths[0].classes()).toContain('table__header-cell--drop-target');
    expect(ths[1].classes()).not.toContain('table__header-cell--drop-target');
  });

  it('calls onColumnHeaderPointerDown with the field when the header is pressed', async () => {
    const props = baseProps();
    const wrapper = mount(
      { template: '<table><thead><tr><TableHeaderCells v-bind="props" /></tr></thead></table>' },
      {
        global: { components: { TableHeaderCells } },
        data: () => ({ props })
      }
    );
    await wrapper.findAll('th')[1].trigger('pointerdown');
    expect(props.onColumnHeaderPointerDown).toHaveBeenCalledTimes(1);
    expect(props.onColumnHeaderPointerDown.mock.calls[0][1]).toBe('artist');
  });

  it('calls onResizerPointerDown and onAutoFitColumn from the resizer handle', async () => {
    const props = baseProps();
    const wrapper = mount(
      { template: '<table><thead><tr><TableHeaderCells v-bind="props" /></tr></thead></table>' },
      {
        global: { components: { TableHeaderCells } },
        data: () => ({ props })
      }
    );
    const resizer = wrapper.find('.table__col-resizer');
    await resizer.trigger('pointerdown');
    expect(props.onResizerPointerDown).toHaveBeenCalledTimes(1);
    expect(props.onResizerPointerDown.mock.calls[0][1]).toBe('title');

    await resizer.trigger('dblclick');
    expect(props.onAutoFitColumn).toHaveBeenCalledTimes(1);
    expect(props.onAutoFitColumn.mock.calls[0][0]).toBe('title');
  });

  it('lets the default cell slot be overridden per field', () => {
    const wrapper = mount(
      {
        template: `
          <table>
            <thead>
              <tr>
                <TableHeaderCells v-bind="props">
                  <template #default="{ field, label }">
                    <button v-if="field === 'title'" class="sort-btn">{{ label }} sortable</button>
                  </template>
                </TableHeaderCells>
              </tr>
            </thead>
          </table>
        `
      },
      {
        global: { components: { TableHeaderCells } },
        data: () => ({ props: baseProps() })
      }
    );
    const ths = wrapper.findAll('th');
    expect(ths[0].find('.sort-btn').exists()).toBe(true);
    expect(ths[0].text()).toContain('Title sortable');
    // The slot fallback is evaluated per field independently: artist's
    // invocation of the override renders nothing (v-if false), so it falls
    // back to the component's own default label span for that field only.
    expect(ths[1].find('.table__header-cell-label').exists()).toBe(true);
    expect(ths[1].text()).toBe('Artist');
  });

  it('omits the resizer handle for a field that is not resizable', () => {
    const wrapper = mount(
      { template: '<table><thead><tr><TableHeaderCells v-bind="props" /></tr></thead></table>' },
      {
        global: { components: { TableHeaderCells } },
        data: () => ({
          props: { ...baseProps(), isResizable: (field: string) => field !== 'artist' }
        })
      }
    );
    const ths = wrapper.findAll('th');
    expect(ths[0].find('.table__col-resizer').exists()).toBe(true);
    expect(ths[1].find('.table__col-resizer').exists()).toBe(false);
  });

  it('keeps the resizer as a direct child of the th, not nested inside the truncated content wrapper', () => {
    // happy-dom cannot observe clipping, so the guard is structural: the resizer
    // straddles the border and must not nest inside the truncating wrapper.
    const wrapper = mount(
      { template: '<table><thead><tr><TableHeaderCells v-bind="props" /></tr></thead></table>' },
      {
        global: { components: { TableHeaderCells } },
        data: () => ({ props: baseProps() })
      }
    );
    const th = wrapper.find('th');
    const resizer = th.find('.table__col-resizer');
    const content = th.find('.table__header-cell-content');
    expect(resizer.element.parentElement).toBe(th.element);
    expect(content.element.contains(resizer.element)).toBe(false);
  });

  it('falls back to the default label span when no slot content is provided', () => {
    const wrapper = mount(
      { template: '<table><thead><tr><TableHeaderCells v-bind="props" /></tr></thead></table>' },
      {
        global: { components: { TableHeaderCells } },
        data: () => ({ props: baseProps() })
      }
    );
    const label = wrapper.find('th .table__header-cell-label');
    expect(label.exists()).toBe(true);
    expect(label.text()).toBe('Title');
  });
});
