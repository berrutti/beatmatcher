<template>
  <span
    v-if="hasError"
    class="collection__item-tag collection__item-tag--error"
    v-tooltip="$t('browser.analyzeFailedTooltip')"
    >{{ $t('browser.statusError') }}</span
  >
</template>

<script setup lang="ts">
defineProps<{ hasError: boolean }>();
</script>
