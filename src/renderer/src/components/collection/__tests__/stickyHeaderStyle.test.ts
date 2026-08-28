import { describe, it, expect } from 'vitest';
import tableSource from '../Table.vue?raw';
import headerCellSource from '../TableHeaderCell.vue?raw';
import headerCellsSource from '../TableHeaderCells.vue?raw';

const SOURCES: Record<string, string> = {
  'Table.vue': tableSource,
  'TableHeaderCell.vue': headerCellSource,
  'TableHeaderCells.vue': headerCellsSource
};

// jsdom applies no component styles, so the stylesheet is the subject.
function stylesOf(component: string): string {
  const source = SOURCES[component];
  return source.slice(source.indexOf('<style'), source.lastIndexOf('</style>'));
}

function ruleFor(component: string, selector: string): string {
  const styles = stylesOf(component);
  const at = styles.indexOf(`${selector} {`);
  expect(at, `${selector} not found in ${component}`).toBeGreaterThan(-1);
  return styles.slice(at, styles.indexOf('}', at));
}

const HEADER_CELLS: [string, string][] = [
  ['TableHeaderCell.vue', '.table__header-cell'],
  ['Table.vue', '.table__filler']
];

describe('the frozen header draws one divider, not two', () => {
  it.each(HEADER_CELLS)('%s %s carries a shadow and no bottom border', (component, selector) => {
    const rule = ruleFor(component, selector);
    // The shadow is painted by the cell so it stays under a sticky row. A
    // collapsed border would scroll away. Keeping both draws two lines.
    expect(rule).toContain('box-shadow: inset 0 -1px 0');
    expect(rule).not.toContain('border-bottom');
  });

  it.each(HEADER_CELLS)('%s %s stays sticky', (component, selector) => {
    expect(ruleFor(component, selector)).toContain('position: sticky');
  });

  it('lets no header cell override the sticky with another position', () => {
    const meta = ruleFor('TableHeaderCells.vue', '.table__header-cell--meta');
    const declarations = meta.replace(/\/\*[\s\S]*?\*\//g, '');
    expect(declarations).not.toMatch(/position\s*:/);
  });
});
