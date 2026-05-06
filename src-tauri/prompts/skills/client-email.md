---
name: client-email
version: 1.0.0
description: Draft client-facing emails in Caitlin's voice from current campaign state. Reads the latest snapshot, the latest thread from the account manager, and produces a ready-to-paste reply with PDF attach instructions and shareable Dropbox link placeholder. Triggers on "/client-email", "draft email to <contact>", "reply to Mahala with the numbers".
metadata:
  openclaw:
    category: "client-work"
    domain: "comms"
---

# client-email

Turn a campaign snapshot + the latest Gmail thread into a ready-to-send email reply. Uses Caitlin's voice, applies stop-slop rules, handles the Dropbox-attach convention.

<Use_When>
- A snapshot has just shipped and the client team needs an update email with the top-line numbers
- A client has asked a direct question ("how's the campaign going?", "can you send an update?") and the answer comes from the latest snapshot
- Moving between phases and the email needs to carry both the wrap of the prior phase and the plan for the next
</Use_When>

<Do_Not_Use_When>
- The message is a sensitive / high-stakes reply (use `sensitive-email-writer` instead)
- There's no snapshot yet — run `/snapshot <slug>` first so the numbers exist
- The reply is a one-liner ("sent, thanks!") that doesn't need drafting
</Do_Not_Use_When>

## Procedure

### Step 1 — Confirm inputs

Need from Caitlin:
- Slug (which client)
- Target recipient (Mahala, Stu, Edvard, whoever)
- Framing date: when is this email going out? Morning-after-snapshot send frames "last night" / "this morning" / "today" correctly.

### Step 2 — Pull the latest snapshot entry

Read `<typst-data>/<slug>.json`. Grab the most recent entry's:
- `entry_label` and `entry_date`
- 3–4 top-line numbers from the `numbers` section (reach, clicks, spend, key conversion signal)
- The `next_up` spend plan if there is one for the upcoming week

### Step 3 — Pull the Gmail thread context

Use the Gmail `search_threads` tool with the client's `email_keywords`. Find the most recent thread with the account manager. Grab:
- The last message's sender, timestamp, and body
- Any direct question they asked ("how's the campaign going?", "any insights?", content calendar attached)

### Step 4 — Pull the shareable Dropbox link

The snapshot mirror lives at `CLIENTS/<Client>/<Project>/4. Reports/<Campaign>/`. Ask Caitlin to right-click → Copy link, paste it in. The `dropbox.com/home/` URL is her admin view; clients can't open it.

### Step 5 — Draft the reply

Apply Caitlin's voice rules. **Canonical voice profile is `~/.claude/voice/voice.md`** — read it before drafting. The rule file at `~/.claude/rules/in-cahoots-voice.md` is the In-Cahoots-business overlay (service language, credentials, audience tone calibration).

Client emails are **studio register** (not founder register). Key rules:
- **Long trailing sentences, 25 to 60+ words common.** Comma-stitched compound structures with parenthetical clauses. If three short sentences appear in a row, rewrite as one or two longer ones. No staccato fragments, no period-fragmented sequences, no tricolon punchlines, no one-word emphasis lines.
- Australian spelling, contractions everywhere
- `!` and `:)` fine in casual emails
- **No em dashes ever.** Use colons, semicolons, full stops.
- No marketing clichés ("game-changing", "unmissable", "iconic", "next-level", "must-attend")
- No rhetorical openers ("the truth is", "here's the thing", "it's not about X it's about Y")
- No slang substitutions ("lean into", "play", "lands", "sits at", "carries", "wears", "ballpark", "punch list")
- Smuggle credentials mid-sentence; numbers as flavour, not flex.

Apply stop-slop, but with one override: **stop-slop's "vary rhythm by mixing short and long" does NOT apply when drafting in Caitlin's voice.** Her rhythm is long-and-longer. Short lines reserved for sign-offs and bullet items only.

Other stop-slop rules still apply:
- No emotional adverbs in studio register
- No "here's what"
- Active voice, name the human
- No binary contrasts

Shape:
1. One-line opener framing when/why you're writing
2. Top-line numbers, 3–5 bullets, bold on the headline figures
3. Forward-looking spend split or next-steps block if phase-transitioning
4. Specific asks (send this, ping me when, confirm by)
5. Sign-off: `Cheers,` / `Best,` + first name

### Step 6 — Include the PDF attach instruction

Remind Caitlin:
- Attach the PDF from `CLIENTS/.../4. Reports/<Campaign>/`
- Use the shareable link (not the `dropbox.com/home/` admin URL)

### Step 7 — Hand over

Present the draft as **plain text** (no markdown indentation, no numbered-list indent) so Caitlin can copy-paste into Gmail / Outlook without formatting artefacts.

## Voice examples

**Opening lines Caitlin uses:**
- "Morning! Wrapped the Re-warm numbers last night, report's attached."
- "Hey M, quick update on where we've landed."
- "Happy Tuesday! Just finalised the Phase 1 check-in, doc attached."

**Never uses:**
- "I hope this email finds you well"
- "Just circling back"
- "Wanted to touch base"
- "Please find attached..."
- "Looking forward to your thoughts"

**Signs off:**
- Casual: `Cheers, Caitlin` or `Catch you soon, C`
- Formal-but-warm: `Best, Caitlin`

## Integration

- **snapshot** — provides the numbers this skill draws from
- **phase-transition** — if this email covers a phase hand-off, coordinate with the phase-transition skill's plan table
- **sensitive-email-writer** — escalate there when stakes are high (overspend discussions, scope disputes, client concerns)
