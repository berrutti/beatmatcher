import { contextBridge } from 'electron';
import { electronAPI } from '@electron-toolkit/preload';

if (process.contextIsolated) {
  try {
    contextBridge.exposeInMainWorld('electron', electronAPI);
  } catch (error) {
    console.error(error);
  }
} else {
  // @ts-expect-error: electron should be added to the window object
  window.electron = electronAPI;
}
