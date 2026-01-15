# MVP API Bring-Up — Implementation Plan
**Overall Progress:** 50%

## Tasks
- [ ] 🟨 Step 1: Laat de API starten op een vrije poort
  - [ ] 🟩 Subtask 1: Zet default `BIND_ADDR` op een vrije poort (bijv. 127.0.0.1:3100) en verbeter bind-foutmelding
  - [ ] 🟥  🔎 USER: start met `BIND_ADDR=127.0.0.1:3100 cargo run -p screening-api` en bevestig dat de server “listening” logt

- [ ] 🟨 Step 2: Bewijs dat endpoints werken op nieuwe poort
  - [ ] 🟩 Subtask 1: Werk usage snippet bij (curl) en controleer `/health` en `/v1/persons/screen` op JSON 200-responses
  - [ ] 🟥  🔎 USER: curl `http://127.0.0.1:3100/health` en `http://127.0.0.1:3100/v1/persons/screen` (met body) en bevestig status 200 en JSON met `request_id`

- [ ] 🟨 Step 3: Documentatie en changelog bijwerken
  - [ ] 🟩 Subtask 1: Noteer poortwijziging/quickstart in `docs/changelog.md` en (indien nodig) `docs/refactor.md`
  - [ ] 🟥  🔎 USER: open de genoemde docs en bevestig dat de nieuwe run-instructies en poort staan genoteerd

## Final Verification Round (present in chat when implementation is done)
1. 🟥🔎 USER: start server op 127.0.0.1:3100 en bevestig “listening”-log
2. 🟥🔎 USER: curl `/health` en `/v1/persons/screen` op 127.0.0.1:3100 met geldige body en zie JSON 200 met `request_id`
3. 🟥🔎 USER: lees docs/changelog (en refactor indien toegevoegd) en bevestig dat poort/quickstart vermeld zijn

## Operational Checklist
- Docs: update `docs/refactor.md` (wat / waarom / impact / verificatie)
- Indien UI gewijzigd: `npm run build` → `rsync …/dist/assets/ …` → `python3 tools/sync_static_aliases.py` → curl-checks (index.js/.css)
- Containers vernieuwen: `sudo docker compose build app && sudo docker compose up -d`
- Functionele sanity: login → kritieke flow → laatst aangepaste feature

## Non-Goals
- Geen verdere ingest/parser/matching-uitbreidingen nu; alleen draaien en documenteren van de MVP API.
