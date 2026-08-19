# NiceSSH Tauri updater relay

A 113-line Node.js service that sits between Zealot and the Tauri auto-updater.

## Why

Zealot's `ReleaseSerializer` doesn't expose a `latest.json`-shaped endpoint that Tauri
can consume directly. This relay queries Zealot's `/api/apps/latest?channel_key=...`
on each request, reads the embedded minisign signature from
`release.custom_fields.tauri_signature`, strips the `untrusted/trusted comment:` lines
(minisign format wraps a 2-line base64 between two comment lines), and assembles a
manifest Tauri understands.

## Deploy

- Node.js 20+ (uses native `fetch`).
- The service binds to `127.0.0.1:8081`. Expose it via your reverse proxy:
  ```nginx
  location /relay/ {
      proxy_pass http://127.0.0.1:8081/;
      proxy_buffering off;
      proxy_cache off;
      add_header Cache-Control "no-cache, no-store" always;
  }
  ```
- Run as a systemd unit:
  ```ini
  [Service]
  WorkingDirectory=/opt/1panel/apps/nicessh-relay
  ExecStart=/opt/1panel/apps/nodejs/20.x.x/bin/node server.js
  ...
  ```

## Configure

`server.js` has three constants at the top:
- `ZEALOT_INTERNAL` — internal Zealot URL (default `http://127.0.0.1:36300`).
- `DOWNLOAD_BASE` — public Zealot URL Tauri clients will hit.
- `PLATFORMS` — list of 6 `{ id, key }` pairs mapping Tauri platform strings
  (`darwin-aarch64`, `windows-x86_64`, etc.) to Zealot channel keys.

## Tested against Zealot 6.2.0

`ReleaseSerializer` exposes `install_url` (not `download_url`); for non-iOS the two
are equivalent. `custom_fields` is a jsonb column on the release; we use it to stash
the `.sig` file content the CI uploads alongside each bundle.
