---
name: client-onboarding
description: Drafts the In Cahoots client dossier (12-section internal source-of-truth doc) from any source — discovery call, intake form, email, Slack thread, calendar event, or manual paste. Use this skill whenever Caitlin or Rose needs to spin up a new client dossier or refresh an existing one off a fresh source.
---

# Client Onboarding — Dossier Drafter

## Role

You are drafting a client dossier for In Cahoots. The dossier is the internal source-of-truth document for a client engagement. Caitlin and Rose read it. Clients never see it.

You'll be given:

- The client's name and code (or a "new lead" name + slug if it's a fresh prospect).
- One source blob — call notes, email thread, Slack thread, intake-form submission, calendar event, or a manual paste.
- Optional extra notes from Caitlin.

Your job: extract everything you can from the source and produce the 12-section dossier markdown. Where the source doesn't cover a section, mark it `TBC` and (if useful) leave a one-line hint about what to ask. Do not invent facts.

---

## The 12-section format

Use this exact structure, with the YAML frontmatter at the top. Match the style of existing In Cahoots dossiers (warm, direct, plain language, Australian spelling, no em dashes, no marketing clichés).

```
---
doc_type: client_dossier
client_slug: <kebab-case slug>
client_name: <full name>
project_name: <best guess from source, or TBC>
status: <scoping | active | retainer | wrap | TBC>
owner: Caitlin Reilly
last_updated: <YYYY-MM-DD — today>
---

# Client Dossier — <client_name>

Internal source of truth. Not for client sharing.

---

## 1. Basics

- Client: <name + 1-line description of who they are>
- Project: <project name + 1-line description, or TBC>
- Status: <scoping / active / retainer / wrap / TBC>
- Engagement start: <date or TBC>
- Billing: <retainer / per-campaign / TBC>
- Contract: <CSA signed / pending / TBC>

## 2. Team

**In Cahoots side:**
- Strategy + oversight: Caitlin
- Execution: <Rose / Caitlin / TBC>

**Client side:**
- Primary contact (day-to-day): <name, role, email, phone>
- Decision-maker (if different): <name, role>
- Other team: <names + roles>
- Introduction made by: <name>, if relevant

## 3. Brand + Voice

### Tone of voice
- <bullet list of voice traits from source, or TBC>

### Dos
- <list, or TBC>

### Don'ts
- <list, or TBC>

### Voice examples
- <past campaigns, EDMs, social, or TBC>

## 4. Assets

- <list of asset locations / what's been shared, or TBC>

## 5. Scope + Phases

### Scope
- <bullet what In Cahoots is doing, or TBC>

### Phases + timeline
- <phase 1: dates + deliverables>
- <phase 2: ...>
- <or TBC>

### Budget
- <confirmed / range / TBC>

## 6. Access

- <accounts, drives, ad accounts, CMS, or TBC>

## 7. Audience

- Target demographics: <or TBC>
- Target markets: <or TBC>
- Psychographic notes: <or TBC>
- Competitor / reference orgs: <or TBC>

## 8. Content Guidelines

- Platforms in scope: <or TBC>
- Cadence: <or TBC>
- Content themes: <or TBC>
- Approval flow: <or TBC>

## 9. Working Conventions

- Update cadence: <fortnightly / monthly / TBC>
- Approval flow: <or TBC>
- Red flags / lessons learned: <or TBC>
- Client quirks: <or TBC>

## 10. History

- <chronological bullet list of relevant prior interactions extracted from the source, or TBC>

## 11. Current Open Items

- [ ] <action items extracted from the source>
- [ ] <or TBC>

## 12. Links

- Client folder: ~/Dropbox/IN CAHOOTS/<CLIENT NAME> - <year> (create once scope confirmed)
- Screenshot inbox: ~/Dropbox/IN CAHOOTS/SCREENSHOT INBOX/<slug>/
- Snapshot folder: ~/Dropbox/IN CAHOOTS/ADMIN:FINANCE/CAMPAIGN SNAPSHOTS/
- Memory file: ~/.claude/projects/-Users-caitlinreilly/memory/clients/<slug>.md
- Slack channel: #client-<slug>
- Slack Canvas: TBC (create once scope confirmed)

---

Last updated: <Friendly date> by Caitlin.
```

---

## Extraction rules

- **Pull verbatim where you can.** If the source has the contact's email and phone, lift them directly. Don't paraphrase factual details.
- **Don't invent.** If a section isn't in the source, write `TBC` and stop. Better to leave a clean gap than fabricate detail.
- **Compress, don't expand.** A bullet should be one line where possible. Don't pad with filler.
- **Use the client's actual language.** If they call themselves a "youth music org" don't upgrade it to "youth music sector leader".
- **Status default:** if you can't tell, `scoping` is the right answer for new leads.
- **Slug rule:** lowercase, kebab-case, drop punctuation. "The Push" → `the-push`. "C-DOC" → `cdoc`.

---

## Tone

- Australian spelling. No em dashes. No marketing clichés.
- Plain language. Match the warmth and direct style of the existing dossiers (the-push, cdoc, darcie).
- Internal voice — Caitlin reads this, not the client. It's fine to note red flags or things to watch.

---

# Output

Return the full dossier markdown (frontmatter + 12 sections) as the body, then the standard Companion receipt JSON block. The receipt's first section should list the dossier sections that were filled vs marked TBC, so Caitlin can see what's left to fill at a glance.
