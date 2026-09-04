// @vitest-environment happy-dom
import { describe, it, expect } from 'vitest';
import { mount } from '@vue/test-utils';
import { createI18n } from 'vue-i18n';
import ProgressModal from '@renderer/components/modals/ProgressModal.vue';
import en from '@renderer/locales/en.json';

const i18n = createI18n({ legacy: false, locale: 'en', messages: { en } });

function mountModal(props: {
  open: boolean;
  fraction: number;
  determinate?: boolean;
  counts?: string;
  label?: string;
  cancelLabel?: string;
}) {
  return mount(ProgressModal, {
    props: { title: 'Working', body: 'Please wait', label: 'Decoding tracks', ...props },
    global: { plugins: [i18n] }
  });
}

describe('ProgressModal', () => {
  it('renders nothing while closed', () => {
    const wrapper = mountModal({ open: false, fraction: 0 });
    expect(wrapper.find('.modal').exists()).toBe(false);
  });

  it('offers no way to dismiss itself when no cancel label is given', () => {
    const wrapper = mountModal({ open: true, fraction: 0.5 });
    expect(wrapper.findAll('button')).toHaveLength(0);
  });

  it('fills the bar to the given fraction', () => {
    const wrapper = mountModal({ open: true, fraction: 0.42 });
    expect(wrapper.find('.loading-modal__fill').attributes('style')).toContain('width: 42%');
  });

  it('floors the percentage so it never reads 100 while work is outstanding', () => {
    const wrapper = mountModal({ open: true, fraction: 0.999 });
    expect(wrapper.find('.loading-modal__percent').text()).toBe('99%');
  });

  it('clamps a fraction outside 0..1 rather than overflowing the bar', () => {
    const over = mountModal({ open: true, fraction: 1.4 });
    expect(over.find('.loading-modal__percent').text()).toBe('100%');
    const under = mountModal({ open: true, fraction: -0.2 });
    expect(under.find('.loading-modal__percent').text()).toBe('0%');
  });

  it('shows no counts when it was given none', () => {
    const wrapper = mountModal({ open: true, fraction: 0 });
    expect(wrapper.find('.loading-modal__counts').exists()).toBe(false);
  });

  it('exposes progress to assistive tech', () => {
    const wrapper = mountModal({ open: true, fraction: 0.25 });
    const bar = wrapper.find('[role="progressbar"]');
    expect(bar.attributes('aria-valuenow')).toBe('25');
    expect(bar.attributes('aria-valuemin')).toBe('0');
    expect(bar.attributes('aria-valuemax')).toBe('100');
  });

  it('sweeps an indeterminate bar and hides the percentage when not measured', () => {
    const wrapper = mountModal({ open: true, fraction: 0, determinate: false, counts: '1 of 2' });
    expect(wrapper.find('.loading-modal__fill--indeterminate').exists()).toBe(true);
    expect(wrapper.find('.loading-modal__percent').exists()).toBe(false);
  });

  it('switches to a measured bar when determinate', () => {
    const wrapper = mountModal({ open: true, fraction: 0.5, determinate: true });
    expect(wrapper.find('.loading-modal__fill--indeterminate').exists()).toBe(false);
    expect(wrapper.find('.loading-modal__percent').text()).toBe('50%');
  });

  it('emits cancel from the button a cancel label adds', async () => {
    const wrapper = mountModal({ open: true, fraction: 0.5, cancelLabel: 'Cancel' });
    await wrapper.find('.loading-modal__cancel').trigger('click');
    expect(wrapper.emitted('cancel')).toHaveLength(1);
  });
});
