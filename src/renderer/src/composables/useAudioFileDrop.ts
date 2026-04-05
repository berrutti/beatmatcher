import { ref } from 'vue';

export function useAudioFileDrop(onFile: (file: File) => void) {
  const isDragOver = ref(false);

  function onDragOver(e: DragEvent) {
    e.preventDefault();
    isDragOver.value = true;
  }

  function onDragLeave() {
    isDragOver.value = false;
  }

  function onDrop(e: DragEvent) {
    e.preventDefault();
    e.stopPropagation();
    isDragOver.value = false;
    const file = e.dataTransfer?.files[0];
    if (file) onFile(file);
  }

  function openFileDialog() {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = 'audio/*';
    input.style.display = 'none';
    document.body.appendChild(input);
    input.onchange = () => {
      document.body.removeChild(input);
      if (input.files?.[0]) onFile(input.files[0]);
    };
    input.click();
  }

  return { isDragOver, onDragLeave, onDragOver, onDrop, openFileDialog };
}
