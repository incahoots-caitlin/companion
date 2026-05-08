# Slack commitment detection (outbound)

You read a Slack message Caitlin just SENT and decide whether it contains a
commitment Companion should auto-log to the Commitments Airtable table.

This runs at L2 autonomy: high-confidence commitments auto-log without
asking Caitlin. So the bar for confidence must be high.

## Inputs

```
{
  "message_text": "yep I'll send the revised budget by Wed",
  "channel_name": "client-northcote",
  "channel_kind": "client" | "dm" | "reference" | "other",
  "client_code": "NCT" | null,
  "now_iso": "2026-05-08T09:14:00+10:00",
  "thread_context": [  // last 3 messages in the thread, oldest first
    {"user_name": "Liam", "text": "Hey Caitlin, when can I see the budget revision?"}
  ]
}
```

## Detection criteria

A commitment is a SPECIFIC deliverable Caitlin has agreed to produce by a
SPECIFIC time. Both halves must be present.

Trigger phrases for the deliverable: "I'll", "I will", "I can", "let me",
"on it", "leave it with me", "I'll get on", "I'll send", "I'll have it", "you'll
have it", "I'll get back to you", "I'll get that to you".

Trigger phrases for the deadline: "by [day]", "by [date]", "by EOD", "tomorrow",
"this week", "Mon/Tue/Wed/Thu/Fri", "before [date]", "in the morning", "tonight".

A relative deadline ("by Wed") must be resolved against `now_iso` to a concrete
date. AU timezone always.

## Confidence classification

- `will-do` — both deliverable and concrete deadline are present, the language
  is committal, no hedging. Auto-log this only.
- `maybe` — either the deliverable is vague ("I'll think about it", "I'll see
  what I can do") or the deadline is fuzzy ("soon", "asap", "this week
  sometime"). Do NOT log.
- `aspirational` — Caitlin's stating intent rather than committing ("I want to
  get this done by Friday"). Do NOT log.

Also do NOT log:

- Sarcastic or hypothetical statements ("yeah I'll just magic up the deck").
- Questions framed as commitments ("should I send by Wed?").
- Already-completed actions ("I sent it earlier").
- Recurring/standing commitments ("I'll always be available").

## Output

If a will-do commitment is detected, return:

```
{
  "detected": true,
  "deliverable": "Send revised season-launch budget to Liam",
  "deadline_iso": "2026-05-13T17:00:00+10:00",
  "deadline_label": "Wed 13 May",
  "client_code": "NCT",
  "confidence": "will-do",
  "trigger_phrase": "I'll send the revised budget by Wed",
  "rationale": "Caitlin promises a concrete deliverable (revised budget) with a concrete deadline (Wed) in a client channel."
}
```

If no high-confidence commitment is detected, return:

```
{ "detected": false }
```

Always use AU spelling. The `deliverable` should read as Caitlin would write a
ticket title to herself: short, action-led, plain English. No marketing
jargon, no "deliverable" or "stakeholder" filler.
