# Program Status Protocol: implementation contract

Source: [Rex specification](https://www.superlogical.com/rex/docs/build/program-status),
read 2026-10-14. Baseline: revision **0.4**, dated 2026-10-10.
This is an engineering extraction; the linked specification remains authoritative.

## 1. Motivation

Expose structured application state to terminal hosts. Consumers choose the UI;
producers should describe work, rather than request a particular notification.

## 2. In Rex

Rex currently consumes reports for session indicators, tab status, and Lua events.
Those are host integrations, not additional wire fields or library dependencies.

## 3. Conventions

Normative requirements use RFC 2119/8174 capitals. One terminal means one PTY
endpoint. Records belong to that endpoint, independently of the active screen.

| Token | Bytes |
| --- | --- |
| ESC | `1b` |
| OSC | `1b 5d` |
| ST | `1b 5c` or `07` |

## 4. Syntax

```text
report = OSC "7501;" pairs ST
query  = OSC "7501;?" ST
pairs  = pair (":" pair)*
pair   = key "=" value
key    = [a-z]+
value  = [A-Za-z0-9_.,+/=-]*
```

Trim surrounding key/value whitespace. Skip malformed pairs. Unknown keys have
no effect. The final syntactically valid occurrence of a key determines its
value. Validate every hard bound and every syntactically valid `msg`/`title`
occurrence before exposing a report, even when later occurrences overwrite it.

Text uses standard base64, optionally padded, containing valid UTF-8. Reject the
entire report for decoding failures or C0, DEL, or C1 controls in decoded text.

## 5. Records and ids

```text
id      = segment ("/" segment)*
segment = [A-Za-z0-9_.+-]{1,32}
```

Missing id addresses root; invalid id discards the update. Updates replace all
fields of a record. An omitted field is removed. Slash boundaries define
ancestry for clearing and application inheritance; parents can be absent.
Root and child records coexist. Alternate-screen changes do not clear them.

## 6. States

| Wire value | Rust representation | Shell prompt / attached-process exit |
| --- | --- | --- |
| `idle` | `Action::Set(State::Idle)` | Host may remove |
| `working` | `Action::Set(State::Working)` | Host must remove |
| `blocked` | `Action::Set(State::Blocked)` | Host must remove |
| `done` | `Action::Set(State::Done)` | Retained |
| `error` | `Action::Set(State::Error)` | Retained |
| `clear` | `Action::Clear` | Removal operation, not a stored state |

Cancellation should produce idle. Successful/failed exit should leave done/error.
Clear removes the addressed subtree; clearing root removes every record.
There is no renewal interval or heartbeat.

## 7. Keys

| Key | Representation | Applicability / invalid-value behavior |
| --- | --- | --- |
| `state` | `Action` | Required; unknown/missing discards report |
| `id` | `Option<RecordId>` | Missing=root; invalid discards report |
| `kind` | `Option<Kind>` | Blocked only; unknown is absent |
| `progress` | `Option<Progress>` | Working/blocked only; integer 0–100; invalid is absent |
| `app` | `Option<AppId>` | Stable identifier; invalid grammar is absent |
| `title` | `Option<EncodedText>` | Record label, validated before publication |
| `msg` | `Option<EncodedText>` | Display text, never interpreted as structured data |

Blocking kinds: `permission` (approval), `question` (answer), `auth`
(authentication). Unknown progress is indeterminate, not zero. Missing app is
resolved by the consumer from the nearest ancestor providing an explicit app.

## 8. Feature detection

```text
OSC 7501;? ST
OSC 7501;?:states=idle,working,done,blocked,error:kinds=permission,question,auth ST
```

Detection is optional. Missing lists imply these fixed defaults; explicit empty
lists remain empty. Retain future tokens for callers to inspect. `clear` is not
an advertised state. Ignore reply extensions, including their report-field
names and sizes. Apply the complete-frame cap to replies. A bare query and a
default reply are byte-identical, so the caller supplies direction/pending-query
context.
Timeout is an application choice. Primary DA sent after the query can provide
a negative result when its response arrives before the status reply.

## 9. Security

Decoded text is plain, untrusted text. Control rejection belongs in the parser;
direction/invisible-format sanitization belongs in outside-grid presentation.
Feature replies must contain capabilities only, never echo report data.
Host UI should identify the originating terminal and limit notification rates.

## 10. Limits

| Item | Hard cap |
| --- | ---: |
| Complete OSC including terminator | 4096 bytes |
| Key | 16 bytes |
| Encoded / decoded message | 2732 / 2048 bytes |
| Encoded / decoded title | 256 / 192 bytes |
| App | 32 bytes |
| Id / each segment / segment count | 128 bytes / 32 bytes / 8 |
| Stored records | 256 maximum; at least 64 supported |

Over-cap reports are rejected atomically. A terminal store at capacity evicts
the record with the oldest update, not the oldest read. Storage policy is
outside this parser; no records are retained by either implemented crate.

The source also notes that the largest legal report is under 3300 bytes. This
producer emits at most 3220 bytes with the supported fields present once. The
parser accepts frames up to the 4096-byte cap, including unknown keys and
repeated pairs allowed by the syntax above.

## 11. Relationship to other sequences

OSC 9;4 is a separate progress protocol. Optional mapping to root should stop
after native OSC 7501 input until RIS, to avoid replacing richer reports.
OSC 9/99/777 notifications are events, not current record state. OSC 133 A
provides the prompt-lifetime boundary. OSC 0/2 remains a window-title channel.
OSC 21337/9999 are separate predecessors and are not alternative parse formats.
RIS clears all records; DECSTR does not.

## 12. Examples

The source's single-program flow maps to working → blocked(auth) → working →
done. Its deployment flow maps to independent region ids inheriting root app,
with permission-blocked and done siblings, followed by subtree clears.
The shell example maps success/failure to done/error. Equivalent Rust emission
is demonstrated in `crates/osc7501/examples/tui.rs`.

## 13. Revision history

| Revision | Date | Integration consequence |
| --- | --- | --- |
| 0.1 | 2026-09-28 | Initial protocol |
| 0.2 | 2026-10-06 | Normative language clarification |
| 0.3 | 2026-10-07 | Indeterminate progress; optional probing |
| 0.4 | 2026-10-10 | Capability lists; control/app clarification; no terminfo requirement |

## Explicit implementation choices

- Whitespace means ASCII whitespace around pair boundaries; Unicode whitespace
  is outside the stated byte grammar.
- Standard base64 uses canonical unused trailing bits and either complete
  padding or no padding. Partially padded and noncanonical forms are rejected.
- A recognizable `id=` with an invalid value remains an invalid id, never an
  absent id. This prioritizes the explicit no-root-fallback rule over generic
  malformed-pair skipping. A later valid id can replace it.
- Hard caps take precedence over skipping a malformed or overridden pair with
  an identifiable key. Invalid id grammar is evaluated on the effective id.
- `parse_body` budgets the longer ST; full-frame parsing accounts for either
  terminator exactly. Seven-bit OSC/ST and BEL are the specified encodings.
- The scanner is a focused OSC extractor, not a general terminal emulator.
  Consumers already parsing terminal escape sequences should use `parse_body`.
