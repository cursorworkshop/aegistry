# MVP API Bring-Up - Implementation Plan

## Tasks

### Step 1: Start the API on a free port
- Set default `BIND_ADDR` to a free port (e.g. 127.0.0.1:3100) and improve bind error message
- USER: start with `BIND_ADDR=127.0.0.1:3100 cargo run -p screening-api` and confirm the server logs "listening"

### Step 2: Verify endpoints work on the new port
- Update usage snippet (curl) and check `/health` and `/v1/persons/screen` for JSON 200 responses
- USER: curl `http://127.0.0.1:3100/health` and `http://127.0.0.1:3100/v1/persons/screen` (with body) and confirm status 200 and JSON with `request_id`

### Step 3: Update documentation and changelog
- Note port change/quickstart in `docs/changelog.md` and (if needed) `docs/refactor.md`
- USER: open the mentioned docs and confirm the new run instructions and port are noted

## Final Verification Round
1. USER: start server on 127.0.0.1:3100 and confirm "listening" log
2. USER: curl `/health` and `/v1/persons/screen` on 127.0.0.1:3100 with valid body and see JSON 200 with `request_id`
3. USER: read docs/changelog (and refactor if added) and confirm port/quickstart are mentioned

## Operational Checklist
- Docs: update `docs/refactor.md` (what / why / impact / verification)
- If UI changed: `npm run build` -> `rsync .../dist/assets/ ...` -> `python3 tools/sync_static_aliases.py` -> curl checks (index.js/.css)
- Refresh containers: `sudo docker compose build app && sudo docker compose up -d`
- Functional sanity: login -> critical flow -> most recently changed feature

## Non-Goals
- No further ingest/parser/matching extensions now; only running and documenting the MVP API.
