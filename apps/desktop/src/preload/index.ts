// Preload: the only bridge between the sandboxed renderer and the main process.
// Exposes what needs OS access on `window.komoBridge` — gateway discovery and
// the native directory dialog. The renderer builds its HttpKomoClient over the
// resolver; all HTTP goes direct.

import { contextBridge, ipcRenderer } from "electron";

contextBridge.exposeInMainWorld("komoBridge", {
  gateway: () => ipcRenderer.invoke("komo:gateway"),
  chooseWorkspace: () => ipcRenderer.invoke("komo:choose-workspace"),
});
