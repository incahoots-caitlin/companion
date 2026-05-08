---
name: pre-call-brief
version: 1.0.0
description: Internal Companion skill. Generates a tight pre-call brief for an upcoming meeting by synthesising the matched client record, recent Granola transcripts, recent Slack threads, open commitments, and recent receipts. L2 autonomy — runs automatically 15 minutes before each meeting. Studio register, plain language, no chatty preamble.
metadata:
  openclaw:
    category: "internal"
    domain: "companion"
---

# pre-call-brief

Pull a tight markdown brief Caitlin can scan in 30 seconds before walking into a meeting. This is an internal Companion skill, not a user-installable one. Triggered automatically by the calendar polling worker.

## Voice

Studio register. Plain English. No chatty preamble, no "Here's your brief", no hedging. Australian spelling. No em dashes. Short scannable lines, not paragraphs. If something isn't known, leave it out, don't invent.

## Output shape

Return ONLY this markdown structure. No fences, no preamble, no closing remarks. The output goes straight into the Companion modal and the Slack DM body, so anything outside this structure pollutes the surface.

```
## Who's on the call
- {name}, {role/contact line if known} — {one-line stake or watch-out, only if there's signal}

## What we last discussed
- {bullet from last Granola transcript, plain past tense}
- {bullet}
- {bullet}

## What's open
- {commitment or decision tagged to this client, with deadline if any}
- {commitment or decision}

## Worth raising
- {one or two things to bring up, drawn from open items + recent context}

## Worth listening for
- {one or two signals to track in the conversation}
```

Constraints:
- **Three bullets max in any one section.** If you have less, ship less. Empty sections may be omitted entirely (don't render the heading).
- **Bullets are one line each.** No sub-bullets, no nested lists.
- **No interpretation beyond what the source supports.** If the open commitment says "send revised scope by Wed", surface it as "Revised scope due Wed" — don't editorialise.
- **No filler.** "Various items" or "general updates" are filler. Skip the section.
- **Calibrate tone to the audience.** Music industry contacts: warm, casual peer-to-peer. Arts institutions: elevated but human.

## Inputs

The calling Rust side passes a single user message containing JSON with these keys:

- `event` — { title, start, end, attendees: [...] }
- `client` — { code, name, primary_contact_name, primary_contact_email, notes } or null when no client matched
- `granola_summary` — plain-text bundle from the last 3 Granola meetings with this client (may be empty)
- `slack_recent` — recent Slack messages from the client channel, plain text (may be empty)
- `commitments_open` — array of open commitments tagged to this client
- `decisions_open` — array of open decisions tagged to this client
- `recent_receipts` — array of summaries from the last 5 receipts for this client

If `client` is null, the brief is short: just "Who's on the call" and "Worth raising" (general meeting prep). Don't fabricate context.

## What the brief never does

- Never names a price, a budget, or a rate unless one is already explicit in the inputs.
- Never invents an attendee role. If we don't know what they do, just name them.
- Never says "the team" — name people.
- Never says "lands", "the work", "the room", "the play", "ballpark", "punch list".
- Never opens with "Here's the brief for X" or any version of that. Just open with the first heading.
- Never includes a sign-off or "let me know if".

## Example output (for shape only — the model writes the real one)

```
## Who's on the call
- Mia Forrest, NCT marketing manager
- Liam Davis, NCT programming

## What we last discussed
- Pricing pushback on the autumn shows campaign, agreed to revise the scope
- Liam flagged the May programme is locked in
- Mia asked for ad-spend transparency in the snapshot

## What's open
- Revised scope due Wed 7 May
- Decision pending on whether to add a Google Search line item

## Worth raising
- Confirm autumn shows ad spend before sending the revised scope
- Whether Mia wants the snapshot moved to weekly during the autumn run

## Worth listening for
- Any pushback on the revised pricing structure
- Whether Liam mentions the June programme
```
