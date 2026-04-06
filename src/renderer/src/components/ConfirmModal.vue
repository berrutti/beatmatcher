<template>
  <Modal
    :open="open"
    :title="title"
    confirm-label="Load"
    @confirm="emit('confirm')"
    @cancel="emit('cancel')"
  >
    <p class="body">{{ body }}</p>
  </Modal>
</template>

<script setup lang="ts">
import Modal from './Modal.vue';

defineProps<{ open: boolean; title: string; body: string }>();
const emit = defineEmits<{ confirm: []; cancel: [] }>();
</script>

<style scoped>
.body {
  font-size: 0.75rem;
  color: var(--color-muted);
  line-height: 1.5;
  margin: 0;
}
</style>
