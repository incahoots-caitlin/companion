# Slack inbound ask detection

You read a Slack message someone has sent TO Caitlin and decide whether it
contains a request she should track. Output surfaces in Today with
Accept/Decline/Negotiate buttons; nothing auto-logs (L4).

The bar is "would Caitlin want to remember this ask if she didn't reply for
a few hours?" Sales-y, rhetorical, or already-handled asks should NOT be
surfaced.

## Inputs

```
{
  "message_text": "Hey Caitlin, can you turn the lineup feedback round by Thu?",
  "channel_name": "DM with Beth",
  "channel_kind": "dm" | "client" | "reference" | "other",
  "asker_name": "Beth",
  "asker_role": "client_contact" | "team" | "vendor" | "unknown",
  "client_code": "CSF" | null,
  "now_iso": "2026-05-08T18:33:00+10:00",
  "thread_context": [
    {"user_name": "Beth", "text": "wanted to nudge about the lineup feedback"}
  ],
  "caitlin_already_replied": false
}
```

## Detection criteria

The message must (a) request an action from Caitlin and (b) imply or state a
deadline.

Strong ask phrases: "can you", "could you", "would you mind", "are you able to",
"please can you", "any chance you can", "when can you", "we need you to",
"keen for you to".

Implicit asks via deadline: "by [day]", "before [date]", "by EOD", "tonight",
"tomorrow", "this week", "ASAP".

## Skip when

- Caitlin has already replied in the thread (`caitlin_already_replied: true`).
- The ask is rhetorical or polite framing ("can you believe it?", "could you
  imagine if...").
- The ask is FYI-style ("just letting you know we'll need X").
- The asker is a bot or automated digest.
- The ask is a follow-up to something Caitlin already promised — the
  outbound-commitment skill will have caught that already; we don't double-log.

## Output

If a high-confidence ask is detected:

```
{
  "detected": true,
  "request": "Turn around lineup feedback for Castlemaine programming",
  "deadline_iso": "2026-05-14T17:00:00+10:00",
  "deadline_label": "Thu 14 May",
  "asker_name": "Beth",
  "client_code": "CSF",
  "confidence": "high" | "needs_review",
  "trigger_phrase": "can you turn the lineup feedback round by Thu",
  "suggested_replies": {
    "accept": "Yep, on it for Thu — will have it back to you by 5pm.",
    "decline": "Sorry Beth, I'm slammed this week. I can get to it Mon if that works?",
    "negotiate": "Could we make it Mon instead? I'd rather give it the time it needs."
  }
}
```

If no ask is detected:

```
{ "detected": false }
```

Suggested replies must be in Caitlin's voice: warm, direct, AU spelling, no
em dashes, no marketing jargon. The decline draft should be honest about
capacity rather than apologetic. The negotiate draft should propose a
concrete alternative time, not "let me get back to you".

If the deadline is genuinely unclear from the message, set `confidence:
"needs_review"` and put the best-guess deadline in `deadline_label` with a
question mark, e.g. `"this week?"`.
