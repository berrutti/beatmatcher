<template>
  <!-- The key includes the width itself, not just the field, so a width
       change tears down and recreates the element instead of patching its
       style attribute in place - table-layout: fixed can otherwise cache a
       column's width from the initial layout pass and never re-measure an
       existing <col> node after a later style mutation. -->
  <col
    v-for="field in fields"
    :key="`${field}:${getWidth(field)}`"
    :style="{ width: getWidth(field) }"
  />
</template>

<script setup lang="ts" generic="F extends string">
defineProps<{
  fields: F[];
  getWidth: (field: F) => string;
}>();
</script>
