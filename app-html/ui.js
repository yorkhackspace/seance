const invoke = window.__TAURI__.core.invoke;

function openDesign(path) {
    invoke('open_design', { path });
}
