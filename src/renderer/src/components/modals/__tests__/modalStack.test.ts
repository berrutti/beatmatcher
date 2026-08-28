import { describe, it, expect } from 'vitest';
import { mount } from '@vue/test-utils';
import { nextTick } from 'vue';
import { i18n } from '@renderer/i18n';
import Modal from '../Modal.vue';
import { anyModalOpen } from '@renderer/utils/modalStack';

function open(title: string) {
  return mount(Modal, { props: { open: true, title }, global: { plugins: [i18n] } });
}

describe('global key bindings while a modal is up', () => {
  it('reports a modal open only while one is mounted and open', async () => {
    expect(anyModalOpen.value).toBe(false);
    const wrapper = open('T');
    await nextTick();
    expect(anyModalOpen.value).toBe(true);
    wrapper.unmount();
    await nextTick();
    expect(anyModalOpen.value).toBe(false);
  });

  it('releases on close as well as on unmount, without double counting', async () => {
    const wrapper = open('T');
    await nextTick();
    await wrapper.setProps({ open: false });
    expect(anyModalOpen.value).toBe(false);
    wrapper.unmount();
    await nextTick();
    expect(anyModalOpen.value).toBe(false);
  });

  it('counts two modals so closing one does not free the keyboard', async () => {
    const first = open('A');
    const second = open('B');
    await nextTick();
    expect(anyModalOpen.value).toBe(true);
    first.unmount();
    await nextTick();
    expect(anyModalOpen.value).toBe(true);
    second.unmount();
    await nextTick();
    expect(anyModalOpen.value).toBe(false);
  });
});
