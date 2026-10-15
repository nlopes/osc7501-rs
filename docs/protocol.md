# Program Status Protocol

These notes summarize revision **0.4**, dated 2026-10-10, from the
[Rex specification](https://www.superlogical.com/rex/docs/build/program-status)
read on 2026-10-14. The specification remains authoritative.

Programs report what they are doing. The terminal chooses how to display it.
One terminal means one PTY endpoint; its records survive screen switches.

## Wire format

| Token | Bytes |
| --- | --- |
| ESC | `1b` |
| OSC | `1b 5d` |
| ST | `1b 5c` or `07` (BEL) |

```text
report = OSC "7501;" pairs ST
query  = OSC "7501;?" ST
pairs  = pair (":" pair)*
pair   = key "=" value
key    = [a-z]+
value  = [A-Za-z0-9_.,+/=-]*
```

Trim whitespace around keys and values. Skip malformed pairs and unknown keys.
The last syntactically valid occurrence of a key supplies its value. Check
every hard limit and every valid `msg` or `title` pair, including values
overwritten later.

Text uses standard base64, with optional padding, and must decode to valid
UTF-8. Invalid encoding or C0, DEL, or C1 controls reject the whole report.

## Records and ids

```text
id      = segment ("/" segment)*
segment = [A-Za-z0-9_.+-]{1,32}
```

A missing id addresses root. An invalid id discards the update. Each update
replaces every field of its record; omitted fields disappear.

Slash boundaries define ancestry. Clearing `build` removes `build/test`
but keeps `builder`. Clearing root removes every record. Child records can
exist without their parents. A missing app inherits from the nearest ancestor
with an explicit app.

## States and cleanup

| State | Meaning | On shell prompt or attached-process exit |
| --- | --- | --- |
| `idle` | No active work | Host may remove |
| `working` | Work in progress | Remove |
| `blocked` | Waiting for the user | Remove |
| `done` | Work succeeded | Retain |
| `error` | Work failed | Retain |

`clear` is a removal action, represented by `Action::Clear` in Rust.
It is not a stored state. Send idle after cancellation and done or error
when work ends. There is no heartbeat or renewal interval.

The store retains idle on prompt and process exit. A full reset (RIS) clears
all records. A soft reset (DECSTR) or screen switch leaves them intact.

## Fields

| Key | Meaning | Invalid or missing value |
| --- | --- | --- |
| `state` | Current state or clear action | Missing or unknown: ignore report |
| `id` | Record path | Missing: root; invalid: ignore report |
| `kind` | Blocking reason | Unknown: absent; applies only to blocked |
| `progress` | Integer percentage, 0–100 | Invalid: absent; applies only to working or blocked |
| `app` | Stable application identifier | Invalid grammar: absent |
| `title` | Base64 record label | Invalid text: reject report |
| `msg` | Base64 display message | Invalid text: reject report |

Blocking kinds are `permission`, `question`, and `auth`. Omitted progress
means the amount of work is unknown, not zero. Field size limits still apply
when invalid values would otherwise be ignored.

## Capability queries

Detection is optional. A query and an explicit reply look like this:

```text
OSC 7501;? ST
OSC 7501;?:states=idle,working,done,blocked,error:kinds=permission,question,auth ST
```

Missing lists use the fixed defaults shown above. Empty lists advertise no
support. Unknown names are skipped; `clear` is never an advertised state.

Replies ignore other pairs, including report field names and their field
limits. The complete-frame limit still applies. A bare default reply has the
same bytes as a query; the caller uses stream direction and pending-query
state to tell them apart.

The application chooses a timeout. A Primary DA response received before the
status reply can indicate no support when DA was sent after the query.

## Limits

| Item | Hard limit |
| --- | ---: |
| Complete OSC frame, including terminator | 4096 bytes |
| Key | 16 bytes |
| Encoded / decoded message | 2732 / 2048 bytes |
| Encoded / decoded title | 256 / 192 bytes |
| App | 32 bytes |
| Id / segment / number of segments | 128 bytes / 32 bytes / 8 |
| Stored records | At most 256; hosts must support at least 64 |

A report that exceeds a limit is rejected as a whole. At capacity, a store
evicts the record with the oldest update. Reading a record does not affect
eviction order. The parser returns borrowed reports; `osc7501-store` retains
current records.

The producer emits at most 3220 bytes with each supported field present once.
The parser accepts frames up to 4096 bytes, including unknown keys and repeated
pairs. The producer's maximum is not the parser's input limit.

## Display and related sequences

Decoded text is plain, untrusted data. The parser rejects controls; the host
handles direction and invisible formatting in display text. Identify the
originating terminal and limit notifications. Capability replies must contain
capabilities only, never echoed report data.

| Sequence | Relationship |
| --- | --- |
| OSC 9;4 | Separate progress protocol; stop mapping it to root after native OSC 7501 input until RIS |
| OSC 9/99/777 | Notification events, rather than current status records |
| OSC 133 A | Shell prompt boundary |
| OSC 0/2 | Window title |
| OSC 21337/9999 | Separate predecessors, not alternate parse formats |

## Parser choices

These rules make ambiguous or malformed input predictable:

- Whitespace around pairs means ASCII whitespace.
- Base64 must have canonical unused bits and either complete padding or none.
  Partial padding and noncanonical encodings are rejected.
- A recognizable but invalid `id=` never falls back to root. A later valid
  id can replace it.
- Hard limits apply before skipping malformed or overwritten pairs with an
  identifiable key. Id grammar is checked on the effective id.
- `parse_body` budgets the longer, two-byte ST. `parse_sequence` accounts
  for the actual terminator, including BEL at the 4096-byte boundary.
- Only seven-bit OSC/ST and BEL are recognized. The decoder extracts OSC 7501;
  applications with a terminal dispatcher should use `parse_body`.

See the [design decisions](design.md) for Rust API choices and the
[TUI example](../crates/osc7501/examples/tui.rs) for a complete status flow.
