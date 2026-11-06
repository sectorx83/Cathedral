# Cathedral Studio preview

This is the React + TypeScript frontend for the Phase 4 Studio work.

## Local development

1. Start the authenticated local API from the repository root:

   ```sh
   cargo run -p cathedral-studio-api -- --port 0
   ```

2. In another terminal:

   ```sh
   cd studio
   npm ci
   npm run dev
   ```

3. Paste the API address and bearer token printed by the API into the connection screen.

The token is kept in component memory and cleared when disconnecting. Do not paste it into issue reports or commit it to the repository.

## Current UI

Experiment lifecycle, run controls, snapshot create/restore, captured logs, and polled resource summaries are connected to API v1. The native Tauri shell, filesystem browser, graph history, WebSocket streaming, and extension UI are not implemented yet.
