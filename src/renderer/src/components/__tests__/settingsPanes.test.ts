import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { SUPPORTED_LOCALES } from '@renderer/i18n';

const COMPONENT = readFileSync('src/renderer/src/components/Settings.vue', 'utf8');

function locale(code: string): Record<string, Record<string, unknown>> {
  return JSON.parse(readFileSync(`src/renderer/src/locales/${code}.json`, 'utf8')).settings;
}

function shownSections(): string[] {
  return [...COMPONENT.matchAll(/shows\('([a-zA-Z]+)'\)/g)].map((match) => match[1]);
}

function routedSections(): string[] {
  const table = COMPONENT.slice(
    COMPONENT.indexOf('const SECTION_PANE'),
    COMPONENT.indexOf('type PaneId')
  );
  return [...table.matchAll(/^ {2}([a-zA-Z]+):/gm)].map((match) => match[1]);
}

function paneIds(): string[] {
  const list = COMPONENT.slice(
    COMPONENT.indexOf('const PANES'),
    COMPONENT.indexOf('const storedPane')
  );
  return [...list.matchAll(/id: '([a-z]+)'/g)].map((match) => match[1]);
}

describe('settings panes', () => {
  it('every section in the template is routed to a pane', () => {
    for (const section of shownSections()) {
      expect(routedSections(), `${section} is rendered but reaches no pane`).toContain(section);
    }
  });

  it('every routed section is rendered by the template', () => {
    for (const section of routedSections()) {
      expect(shownSections(), `${section} is routed but never rendered`).toContain(section);
    }
  });

  it('every pane holds at least one section', () => {
    const table = COMPONENT.slice(
      COMPONENT.indexOf('const SECTION_PANE'),
      COMPONENT.indexOf('type PaneId')
    );
    for (const pane of paneIds()) {
      expect(table, `${pane} is offered but holds nothing`).toContain(`'${pane}'`);
    }
  });

  it.each(SUPPORTED_LOCALES)('%s names every pane and the filter', (code) => {
    const strings = locale(code);
    for (const pane of paneIds()) {
      expect(strings.panes?.[pane], `${code} is missing pane ${pane}`).toBeTruthy();
    }
    expect(strings.filter?.placeholder, `${code} filter placeholder`).toBeTruthy();
    expect(strings.filter?.noMatch, `${code} filter noMatch`).toBeTruthy();
  });

  it.each(SUPPORTED_LOCALES)('%s titles every section, since the filter reads them', (code) => {
    const strings = locale(code);
    for (const section of routedSections()) {
      expect(strings[section]?.title, `${code} is missing ${section}.title`).toBeTruthy();
    }
  });
});
