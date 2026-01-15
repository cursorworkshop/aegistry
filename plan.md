# MVP API Bring-Up - Implementation Plan
**Overall Progress:** 50%

## Tasks
- [ ] Step 1: Start the API on a free port
  - [ ] Subtask 1: Set default `BIND_ADDR` to a free port (e.g. 127.0.0.1:3100) and improve bind error message
  - [ ] USER: start with `BIND_ADDR=127.0.0.1:3100 cargo run -p screening-api` and confirm that the server logs "listening"

- [ ] Step 2: Prove that endpoints work on the new port
  - [ ] Subtask 1: Update usage snippet (curl) and verify `/health` and `/v1/persons/screen` return JSON 200 responses
  - [ ] USER: curl `http://127.0.0.1:3100/health` and `http://127.0.0.1:3100/v1/persons/screen` (with body) and confirm status 200 and JSON with `request_id`

- [ ] Step 3: Update documentation and changelog
  - [ ] Subtask 1: Note port change/quickstart in `docs/changelog.md` and (if needed) `docs/refactor.md`
  - [ ] USER: open the mentioned docs and confirm that the new run instructions and port are noted

## Final Verification Round (present in chat when implementation is done)
1. USER: start server on 127.0.0.1:3100 and confirm "listening" log
2. USER: curl `/health` and `/v1/persons/screen` on 127.0.0.1:3100 with valid body and see JSON 200 with `request_id`
3. USER: read docs/changelog (and refactor if added) and confirm that port/quickstart are mentioned

## Operational Checklist
- Docs: update `docs/refactor.md` (what / why / impact / verification)
- If UI changed: `npm run build` -> `rsync .../dist/assets/ ...` -> `python3 tools/sync_static_aliases.py` -> curl checks (index.js/.css)
- Refresh containers: `sudo docker compose build app && sudo docker compose up -d`
- Functional sanity: login -> critical flow -> last modified feature

## Non-Goals
- No further ingest/parser/matching extensions now; only run and document the MVP API.
