import { computed, ref } from 'vue';

const openModals = ref(0);

// Global key bindings stand down while a dialog is up: Tab belongs to the dialog's focus
// trap, and every other key belongs to whatever the dialog puts in front of the user.
export const anyModalOpen = computed(() => openModals.value > 0);

export function markModalOpen(): void {
  openModals.value += 1;
}

export function markModalClosed(): void {
  openModals.value = Math.max(0, openModals.value - 1);
}
