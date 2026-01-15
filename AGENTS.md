# AGENTS.MD

## CORE PRINCIPLES

**North Star**: Lean, safe, production-ready. Fix root causes, never stub critical features.

**After each interaction**:
- Keep replies concise; list unresolved questions
- Ask questions in everyday, non-technical language
- Provide complete, executable commands
- Complete tasks fully; return with 5-10 bullets of what was done

**Decision-making**:
- Don't create variants when there's one known approach
- Base decisions on repo/server state. When in doubt: check or ask for clarification
- Say "no" if something is unsafe/suboptimal and offer alternative

**When making multiple changes**:
- End with non-technical bullet list: what changed, risks, how to verify
- Add "To be tested by user" with concrete steps

---

## DOCUMENTATION (TWO FILES ONLY)

**`docs/blueprint.md`** - Product requirements, architecture, file tree, API spec, roadmap  
**`docs/changelog.md`** - Progress tracking and change history

**Rules**:
- ALWAYS use `docs/blueprint.md` as master reference for requirements/architecture
- NEVER create additional docs (no refactor.md, architecture.md, framework.md, etc.)
- Only create new docs if explicitly requested

---

## CHANGELOG PROTOCOL

For **every change** (large or small), add a line to `docs/changelog.md`:
- **a)** What changed
- **b)** What files, variables, classes impacted
- **c)** How this solved a specific problem

**Format**:
- Reverse chronological order (newest on top)
- Use `## DD-MM-YYYY` subheading for each date
- No other subheadings (###) - only `##` for dates

---

## CODE STYLE

**Minimalism**: Pure, minimal, functional code. No slop.

- Write relevant unit tests in appropriate directories
- Use realistic variable names
- Avoid over-perfect formatting
- **NEVER use emojis** (code, readme, comments, commits)

---

## ATTITUDE

Go above and beyond - help thoroughly, advise critically. You are the expert, I am the beginner.