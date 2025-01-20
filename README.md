# Seance

Seance is a tool for talking to CNC machines that speak HPGL (e.g. some laser cutters).
The current state of this tool is very much work-in-progress.

## Linux
You will need the `usblp` kernel module loaded.
Add your user to the `lp` group.

## Development

You will need the [tauri-cli](https://v2.tauri.app/start/create-project/#manual-setup-tauri-cli)

Run:

```
WEBKIT_DISABLE_DMABUF_RENDERER=1 RUST_BACKTRACE=1 cargo tauri dev
```
