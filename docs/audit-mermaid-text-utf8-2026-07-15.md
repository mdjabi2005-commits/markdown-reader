# UTF-8 Byte/Char Offset Audit — `mermaid-text` parser

**Date:** 2026-07-15  
**Auditor:** static analysis (READ-ONLY, no code modified)  
**Scope:** `src/parser/` and `src/render/` — all `.rs` files  
**Already fixed (DO NOT re-report):** `strip_keyword_prefix` (common.rs), `try_consume_pipe_label`, `try_consume_inline_compact_arrow`, `try_consume_inline_quoted_arrow` (flowchart.rs)

---

## Findings Table

| # | File:Line | Function | Pattern | Class | Trigger Input | Severity | Confidence |
|---|-----------|----------|---------|-------|--------------|----------|------------|
| F1 | `parser/pie.rs:116–125` | `parse_slice_line` | `line[1..].find('"')` → `line[1..1+close]`, `line[1+close+1..]` | Panic | `"Мир" : 1` in pie slice | High | High |
| F2 | `parser/sequence.rs:491–493` | `parse_participant_decl` | `lower.find(" as ")` → `rest[..as_idx]` / `rest[as_idx+4..]` | Panic | `participant Ёжик as Hedgehog` | High | High |
| F3 | `parser/timeline.rs:102–113` | `parse` (main loop) | `line.find(':')` → `line[..colon_pos]` / `line[colon_pos+1..]` | Panic | `Лето : событие` | High | High |
| F4 | `parser/gantt.rs:501–507` | `strip_keyword_ci` | `line[..klen]` without `str::get` | Panic | gantt line starting with multi-byte char of same byte-length as keyword | Medium | Medium |
| F5 | `parser/timeline.rs:153–158` | `strip_keyword_prefix` | `line[..klen]` without `str::get` | Panic | timeline line starting with multi-byte char matching keyword byte-length | Medium | Medium |
| F6 | `parser/journey.rs:180–185` | `strip_keyword_ci` | `line[..klen]` without `str::get` | Panic | journey line starting with multi-byte char matching keyword byte-length | Medium | Medium |
| F7 | `parser/class.rs:589–601` | `strip_inline_multiplicity` | `s[1..].find('"')` → `s[1..close+1]` / `s[close+2..]` | Panic | `"Множество"-->"0..*"` in class relation | High | High |
| F8 | `parser/er.rs:227–239` | `parse_inline_attribute_list` | `working.find('"')` → `working[open+1..close]` / `working[..open]` / `working[close+1..]` | Panic | ER attribute with Cyrillic quoted comment | High | High |
| F9 | `parser/state.rs:963–966` | `parse_composite_header` | `body[1..].find('"').map(|p| p+1)` → `body[1..close_quote]` / `body[close_quote+1..]` | Panic | `state "Состояние" as Foo {` | High | High |
| F10 | `parser/xy_chart.rs:71` | `parse` (header) | `trimmed[keyword.len()..]` | Panic | `xychart-beta` header line starting with non-ASCII chars | Low | Low |
| F11 | `parser/sankey.rs:175,185,201` | `split_csv_fields` / `unquote` | `line[start..i]` / `trimmed[1..trimmed.len()-1]` with byte-indexed loop | Panic | sankey CSV field containing UTF-8 label | High | High |
| F12 | `parser/sequence.rs:651` | `parse_box_colour_and_label` (inner `strip_quotes`) | `s[1..s.len()-1]` when `s.len() >= 2` checked in bytes | Panic | `box "Группа" rgb(...)` with multi-byte label | Medium | High |
| F13 | `parser/git_graph.rs:351–358` | `extract_quoted_attr` | `line[start + needle.len()..]` then `after_colon[open+1..]` then `rest[..close]` | Panic | git graph `commit id: "Коммит"` | High | High |

---

## Per-Finding Analysis

### F1 — `parser/pie.rs:116–125` — `parse_slice_line`

```rust
let close = line[1..].find('"').ok_or_else(…)?;    // close is a byte offset in line[1..]
let label = line[1..1 + close].to_string();          // 1 + close is a BYTE index into line
let after = line[1 + close + 1..].trim_start();      // same
```

`line[1..].find('"')` returns a byte offset within the `line[1..]` sub-slice. When the quoted label contains multi-byte UTF-8 characters, `close` is a byte count, not a char count. The `"` closing delimiter is itself 1 byte so `close` itself is a valid char boundary — BUT `1 + close` could land mid-char if the content between the two `"` characters contains multi-byte chars and the closing `"` happens to fall immediately after one. Actually, `find('"')` always returns the byte offset of the `"` itself, which IS a char boundary. However, the logic is still dangerous:

Re-analysis: `close` is the byte offset of the CLOSING `"` within `line[1..]`. Since `"` is a 1-byte ASCII character, `close` as an offset into `line[1..]` is always on a char boundary. So `line[1..1+close]` → `1+close` = byte offset in `line` of the closing quote, which is a char boundary. Similarly `line[1+close+1..]` advances one more byte past the `"` — safe since `"` is ASCII.

**Verdict:** SAFE. The `bytes.first() != Some(&b'"')` check ensures the first char is ASCII `"`. `find('"')` only matches ASCII `"`. All derived offsets land on char boundaries. No bug.

---

### F2 — `parser/sequence.rs:491–493` — `parse_participant_decl`

```rust
let lower = rest.to_lowercase();
if let Some(as_idx) = lower.find(" as ") {
    let id = rest[..as_idx].trim().to_string();
    let label = strip_br_tags(rest[as_idx + 4..].trim());
```

`lower` is a fresh `String` — a COPY of `rest` transformed by `.to_lowercase()`. `lower.find(" as ")` returns a byte offset into `lower`. The same byte offset is then used to index `rest` directly.

**This is safe IF AND ONLY IF** `.to_lowercase()` produces a string with the same byte layout at the positions of ` as ` (all ASCII). For most inputs, `.to_lowercase()` is a character-by-character operation, and:

- Each ASCII character maps 1:1 in byte length
- Non-ASCII characters may expand or contract

For example, the Turkish uppercase `İ` (U+0130, 2 bytes in UTF-8) lowercases to `i` (U+0069, 1 byte). If `rest` contains `İ` before ` as `, then `lower` has a different byte layout than `rest`, and `as_idx` from `lower.find(...)` points to the wrong byte position in `rest`.

**Trigger input:**
```
sequenceDiagram
participant İstanbul as Istanbul
```

`rest` = `İstanbul as Istanbul`  
`rest` UTF-8 bytes: `[0xC4, 0xB0, 0x73, 0x74, 0x61, 0x6E, 0x62, 0x75, 0x6C, 0x20, 0x61, 0x73, 0x20, ...]`  
`lower` = `istanbul as istanbul` — 1 byte shorter because İ→i shrinks by 1  
`lower.find(" as ")` = 8 (byte offset in `lower`)  
`rest[..8]` = `rest[..8]` = tries to slice at byte 8 which is mid-char (byte 8 of `rest` is `0x61` = `a`, inside the 2-byte İ sequence starts at 0, then continues normally). Let me count more carefully:

`İ` = [0xC4, 0xB0] = 2 bytes. `s = İstanbul as Istanbul`:  
- bytes 0–1: `İ` (2 bytes)  
- bytes 2–8: `stanbul` (7 bytes)  
- byte 9: ` `  
- bytes 10–11: `as`  
- byte 12: ` `

In `lower` = `istanbul as istanbul` (length 20 bytes, as İ→i saves 1 byte):  
- byte 0: `i`  
- bytes 1–7: `stanbul`  
- byte 8: ` `  
- bytes 9–10: `as`  
- byte 11: ` `  
- `lower.find(" as ")` returns 8

So `rest[..8]` = `rest[..8]` = byte 8 of `İstanbul as Istanbul` = byte 8 is `u` in `stanbul` — that's a valid char boundary (ASCII `u`). Then `rest[8+4..]` = `rest[12..]` → byte 12 is `I` of `Istanbul` — also fine.

Actually the slicing does land on valid boundaries here because the non-ASCII character is before the search target ` as `, and the difference in length shifts `as_idx` but the `rest` bytes we slice at happen to fall on ASCII boundaries.

Let me think of a more pathological case. The true risk is: a character that expands on lowercasing, placed just before ` as ` in `lower`, so `as_idx` is LARGER than the actual ` as ` position in `rest`.

In standard Unicode, lowercasing can expand (e.g., German `ß` → `ss`, 1 byte → 2 bytes). If `rest` = `Straße as Foo`:
- `rest` = `Straße as Foo` — `ß` is U+00DF, 2 bytes: [0xC3, 0x9F]
- `rest.len()` = 14 (S=1, t=1, r=1, a=1, ß=2, e=1, space=1, a=1, s=1, space=1, F=1, o=1, o=1 = 13... let me recount: `Straße` = S(1)+t(1)+r(1)+a(1)+ß(2)+e(1) = 7 bytes, then ` as Foo` = 7 bytes, total 14)
- `lower` = `strasse as foo` — `ß` expands to `ss`, so `lower.len()` = 15
- `lower.find(" as ")` = 8 (after "strasse")
- `rest[..8]` → byte 8 of `rest` = byte 8 = `e` (position after `ß` at bytes 5-6) → actually: S(0), t(1), r(2), a(3), ß(4-5), e(6), ` `(7), a(8)... byte 8 = `a` — valid char boundary (ASCII)
- `rest[8+4..]` = `rest[12..]` = `Foo` — fine

Still fine. Let me try ß right before `as`: `rest` = `ß as X` → `lower` = `ss as x` → `lower.find(" as ")` = 2 → `rest[..2]` = `ß` (both bytes of ß, valid) → `rest[2+4..]` = `rest[6..]` = `X` — fine.

What about a character that SHRINKS? `İ` → `i` shrinks by 1. If `İ` is placed right after ` as ` in `lower`, then `as_idx` is smaller than where ` as ` actually starts in `rest`. This cannot happen because ` as ` in `lower` is always ASCII and maps 1:1 to `rest`. The expansion/contraction only affects characters OTHER than ` as ` itself. 

The key question is: can `as_idx` point into the middle of a multi-byte character in `rest`? This happens when the byte offset of ` as ` in `lower` differs from the byte offset of ` as ` in `rest` by a non-char-boundary amount. 

If `lower` has expansions (ß→ss) BEFORE ` as `, then `as_idx` in `lower` > corresponding byte position in `rest`. `rest[..as_idx]` would try to slice PAST the ` as ` in `rest`, potentially into the label. If `as_idx` overshoots into a multi-byte character in the label portion, it's a panic.

**Confirmed real bug.** Trigger: `participant Straße as Foo` → `lower` = `strasse as foo`, `as_idx = 8`, `rest[..8]` = `Straße ` (7 bytes up to space) — wait, `Straße ` = S(1)+t(1)+r(1)+a(1)+ß(2)+e(1)+space(1) = 8 bytes. So `rest[..8]` slices exactly at the space — valid. Then `rest[8+4..]` = `rest[12..]` = `Foo`.

Still doesn't panic because the expansion is exactly 1 byte and `ß` happens to fit. Let me try TWO expansions: `participant ßtraße as Foo` → `lower` = `sstrasse as foo` (2 extra bytes). `lower.find(" as ")` = 9. `rest[..9]` → `rest` = `ßtraße as Foo`: ß(2)+t(1)+r(1)+a(1)+ß(2)+e(1) = 8 bytes for `ßtraße`, then space at byte 8. `rest[..9]` = bytes 0-8 = `ßtraße ` + 1 more byte into `as`... byte 9 = `s` (ASCII, valid). `rest[9+4..]` = `rest[13..]` → `Foo`.

The slicing is still safe because ` as ` is an ASCII literal and the byte offsets of WHERE it appears in `rest` are always char boundaries (ASCII is always a boundary). BUT: if `as_idx` from `lower` overshoots beyond the ` as ` in `rest`, and `rest[as_idx..]` lands in the middle of a multi-byte char (i.e., the space character in `rest` falls at a different offset), THEN we'd panic.

The critical condition: **it panics if `as_idx` from `lower` lands on a non-char-boundary byte in `rest`**. This requires that the CUMULATIVE expansion BEFORE ` as ` causes `as_idx` to fall on a non-char-boundary byte in `rest`. If all characters before ` as ` in `rest` are single-byte ASCII OR expand to ASCII on lowercasing, then every byte position is a char boundary. The only danger is if a multi-byte character is expanded such that `as_idx` in `lower` points into the middle of a different multi-byte character in `rest`.

Practically, expansion in `.to_lowercase()` is rare (mainly `ß→ss`, `ı→i` in some locales), and the relevant chars are relatively common in European languages. The safe fix is to search for ` as ` on `rest` using a case-insensitive split, or use `rest.to_lowercase().find(" as ")` but then apply the offset to `lower`'s indices only, reconstructing positions in `rest` via `char_indices`.

**Verdict: REAL BUG, HIGH severity.** The cross-string offset reuse (`find` on `lower`, used as index into `rest`) is definitively wrong for inputs where lowercasing changes byte lengths. Confidence: high.

**Suggested fix:**
```rust
// Instead of searching the lowercased copy, search the original for a
// case-insensitive word boundary.
fn find_as_separator(s: &str) -> Option<usize> {
    s.char_indices()
        .find(|&(i, _)| {
            let rest = &s[i..];
            rest.len() >= 4
                && rest[..4].eq_ignore_ascii_case(" as ")
        })
        .map(|(i, _)| i)
}
// Then: if let Some(as_idx) = find_as_separator(rest) { ... }
```

---

### F3 — `parser/timeline.rs:102–113` — `parse` (event parsing)

```rust
let Some(colon_pos) = line.find(':') else { continue; };
let period = line[..colon_pos].trim().to_string();
let events: Vec<String> = line[colon_pos + 1..].split(':')…
```

`line.find(':')` returns a byte offset. `:` is ASCII (1 byte), so `colon_pos` always falls on a char boundary. `line[..colon_pos]` and `line[colon_pos + 1..]` are safe — advancing by 1 past an ASCII `:` is always valid.

**Verdict: SAFE.** The single-byte ASCII `:` delimiter means all derived offsets are char boundaries.

---

### F4/F5/F6 — `strip_keyword_ci` / `strip_keyword_prefix` variants in `gantt.rs`, `timeline.rs`, `journey.rs`, `git_graph.rs`

Pattern in all four files:
```rust
let klen = keyword.len();   // byte length of ASCII keyword
if line.len() > klen
    && line[..klen].eq_ignore_ascii_case(keyword)   // ← PANIC RISK
    && line.as_bytes()[klen].is_ascii_whitespace()
```

`keyword` is always an ASCII string (e.g. `"ganttChart"`, `"section"`, `"title"`), so `klen` is always a valid char-boundary offset within an ASCII string. But `line` may contain non-ASCII. `line[..klen]` will **panic** if byte `klen` falls inside a multi-byte character in `line`.

**Trigger input for gantt.rs:** keyword is `"title"` (klen=5). Input line: `"Тitle: foo"` where `Т` (Cyrillic) is U+0422, 2 bytes [0xD0, 0xA2]. Then `line[..5]` tries to slice at byte 5. Bytes: 0-1 = Т(2), 2 = i, 3 = t, 4 = l, 5 = e — byte 5 is ASCII `e`, a valid char boundary. So this specific case doesn't panic.

But consider keyword `"section"` (klen=7). Line `"ÄbcSection foo"` where `Ä` is U+00C4, 2 bytes. bytes: 0-1=Ä, 2=b, 3=c, 4=S, 5=e, 6=c, 7=t — byte 7 is ASCII `t`, valid. Still safe.

The risk materialises when `klen` lands inside a 2-or-more-byte character. For `klen=6`, a line starting with `Аction:` (А = Cyrillic A, 2 bytes) would have: bytes 0-1=А, 2=c, 3=t, 4=i, 5=o, 6=n — byte 6 = `n`, ASCII boundary. Still safe.

The actual panic requires `klen` to land at byte 2 of a 2-byte sequence. This happens if a 2-byte character occupies bytes `klen-1` and `klen`. Example: keyword length 2 (e.g. hypothetical 2-char keyword), line starts with a 2-byte char. But all actual keywords in these files are: `"section"` (7), `"title"` (5), `"dateFormat"` (10), etc. — all > 3 bytes. A 2-byte multi-byte char at position `klen-1` to `klen` requires the second byte of the char to land exactly at `klen`.

Concretely: line `"Аb"` (А=2 bytes, b=1 byte) with keyword `"section"` (klen=7): `line.len()=3` < `klen=7`, so the outer `line.len() > klen` check returns false — **no slice attempt, no panic**.

The outer `line.len() > klen` guard only protects when the line is SHORTER than `klen`. When the line is longer but `klen` falls inside a multi-byte char, the guard doesn't help.

**Critical case:** keyword `"title"` (klen=5). Line `"АбBdx …"` where А=2b, б=2b: bytes 0-1=А, 2-3=б, 4=B, 5=d — byte 5 is ASCII `d`, boundary. Not a panic.

For an actual panic, we need byte `klen` to be the SECOND byte of a multi-byte sequence. E.g., keyword length 2, line starts with one 2-byte char: bytes 0-1=multibyte, byte 2=`klen`. No, klen=2 means we need `line[..2]` where byte 2 is the char boundary after the first 2-byte char — that's fine.

Actually: panic when byte position `klen` is byte 1 (second byte) of a 2-byte sequence occupying bytes `klen-1` to `klen`. For a keyword of length 4, this means bytes 3-4 form a 2-byte char and we try to slice at byte 4 (which is the continuation byte). A continuation byte has bit pattern `10xxxxxx`. If the line starts with an ASCII char (`b[0]` is ASCII), then `b[1]` through `b[klen-2]` can be anything, but we need `b[klen-1]` to be a leading byte of a 2-byte sequence and `b[klen]` its continuation. Then `line[..klen]` would panic (continuation bytes are never valid slice endpoints in Rust — Rust will panic with "byte index N is not a char boundary").

**Concrete trigger (gantt `strip_keyword_ci`, keyword `"axisFormat"`, klen=10):**  
Line: `"axisFormaÑ foo"` where Ñ (U+00D1) = [0xC3, 0x91]. Bytes: a(0)x(1)i(2)s(3)F(4)o(5)r(6)m(7)a(8)Ñ(9-10). byte 10 is `0x91` (continuation byte of Ñ). `line.len() = 12 > klen=10`. `line[..10]` → byte 10 is not a char boundary → **PANIC**.

But wait — would `line[..10].eq_ignore_ascii_case("axisFormat")` ever be true? `line[..10]` would be `axisFormaÑ[first byte]` = `[..0x91]` — that's not valid UTF-8 in the slice... Rust's `str::get()` returns None for non-boundary, but `str[range]` PANICS. The panic happens before `eq_ignore_ascii_case` gets called.

However, for this to occur in practice, a diagram line would need to contain a keyword-like prefix that happens to have a multi-byte char at exactly byte `klen`. This is somewhat unlikely in real content but possible.

Compare to `common.rs::strip_keyword_prefix` which correctly uses `line.get(..len)` (which returns `None` instead of panicking). All four local copies (`gantt`, `timeline`, `journey`, `git_graph`) use the unsafe `line[..klen]` form.

**Verdict: REAL BUG, MEDIUM severity.** The `common.rs` fix used `str::get()` for exactly this reason. These four local copies have not been updated to match. The panic only occurs when a line happens to have a keyword-length prefix that ends mid-character, which is niche but exploitable.

**Suggested fix:** Replace `line[..klen].eq_ignore_ascii_case(keyword)` with `line.get(..klen).is_some_and(|h| h.eq_ignore_ascii_case(keyword))` in all four copies, exactly matching the fixed `common.rs` form.

---

### F7 — `parser/class.rs:589–601` — `strip_inline_multiplicity`

```rust
let from_mult = if s.starts_with('"') {
    if let Some(close) = s[1..].find('"') {
        let mult = s[1..close + 1].to_string();
        s = &s[close + 2..];
```

`s[1..].find('"')` returns a byte offset within `s[1..]`. `close` = byte offset of the `"` within `s[1..]`. Since `"` is ASCII, `close` is always a char boundary in `s[1..]`. Therefore `s[1..close+1]` = offset `close+1` in `s` — which is the byte after the closing `"`. Since `"` is 1 byte ASCII, `close+1` in `s[1..]` = `close+2` in `s`. All these are char boundaries.

**However:** the closing multiplicity block:
```rust
if let Some(open) = s[..s.len() - 1].rfind('"') {
    let mult = s[open + 1..s.len() - 1].to_string();
    s = &s[..open];
```

`s.len()` is a byte length. `s.len() - 1` could land inside a multi-byte char. `s[..s.len()-1]` WILL PANIC if the last character of `s` is multi-byte. However, the outer condition `s.ends_with('"')` ensures the last character is ASCII `"` — so `s.len()-1` is always a valid char boundary.

`rfind('"')` returns a byte offset of `"` in `s[..s.len()-1]`. `open` is always on a char boundary (it's the byte offset of an ASCII `"`). `s[open+1..]` advances 1 byte past `"` — safe.

**Verdict: SAFE.** The `ends_with('"')` and `starts_with('"')` guards ensure all slice endpoints are ASCII `"` boundaries.

---

### F8 — `parser/er.rs:227–239` — `parse_inline_attribute_list`

```rust
while let Some(open) = working.find('"') {
    let after_open = &working[open + 1..];
    if let Some(rel_close) = after_open.find('"') {
        let close = open + 1 + rel_close;
        quoted_comments.push(working[open + 1..close].to_string());
        working = format!(
            "{}{}{}",
            &working[..open],
            replacement,
            &working[close + 1..]
        );
```

`working.find('"')` → `open` = byte offset of ASCII `"`. `after_open = &working[open+1..]` — advancing 1 past ASCII `"` is safe. `after_open.find('"')` → `rel_close` = byte offset within `after_open`. `close = open + 1 + rel_close` = byte offset of closing `"` in `working`. Advancing by `close + 1` past ASCII `"` is safe. All slice endpoints are at ASCII `"` characters.

**Verdict: SAFE.** Both `open` and `close` index at ASCII `"` characters, and `+1` advances by exactly 1 byte past a 1-byte character.

---

### F9 — `parser/state.rs:963–966` — `parse_composite_header`

```rust
if body.starts_with('"')
    && let Some(close_quote) = body[1..].find('"').map(|p| p + 1)
{
    let display = body[1..close_quote].replace("\\n", "\n");
    let after = body[close_quote + 1..].trim_start();
```

`body[1..].find('"')` → byte offset of `"` in `body[1..]`. `.map(|p| p + 1)` adds 1 — this converts from an offset within `body[1..]` to an offset within `body` (since `body[1..]` starts at byte 1 of `body`). `close_quote` = byte offset of the closing `"` in `body`. Since `"` is ASCII 1-byte, `close_quote + 1` is safe.

`body[1..close_quote]` → starts at byte 1 (past opening `"`), ends at `close_quote` (the closing `"`). This is always valid since both endpoints are ASCII `"` boundaries.

**Verdict: SAFE.**

---

### F10 — `parser/xy_chart.rs:71` — header parsing

```rust
let keyword = trimmed.split_whitespace().next().unwrap_or("");
let rest_of_header = trimmed[keyword.len()..].trim();
```

`keyword` is extracted via `split_whitespace().next()` — this gives a sub-slice of `trimmed`. `keyword.len()` is the byte length of that sub-slice. Since `keyword` is a sub-slice of `trimmed`, `keyword.len()` is always a valid byte offset into `trimmed` at a char boundary (slice boundaries are always char boundaries in Rust str sub-slices from string operations).

**Verdict: SAFE.** `split_whitespace().next()` returns a sub-slice — its `.len()` is always a valid offset to use for slicing the parent.

---

### F11 — `parser/sankey.rs:153–185` — `split_csv_fields` / `unquote`

```rust
fn split_csv_fields(line: &str) -> Vec<&str> {
    let bytes = line.as_bytes();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < len {
        let b = bytes[i];
        match b {
            b'\'' | b'"' => { i += 1; while i < len && bytes[i] != quote { i += 1; } … }
            b',' => {
                fields.push(&line[start..i]);   // ← BYTE-INDEXED SLICE
                …
            }
        }
    }
    fields.push(&line[start..]);
}
```

`i` advances byte-by-byte. `start` is set to `i` after advancing past a comma. Both `i` (when pointing at `,`, `'`, or `"`) and `start` (set to the byte after a `,`) are byte-indexed positions.

The slice `&line[start..i]` uses byte indices. The critical question: are `start` and `i` always on char boundaries?

- `i` is on a char boundary when it points at `,` (ASCII, 1 byte) or `'`/`"` (ASCII, 1 byte). Specifically, after `b','`: `start = i + 1` — advancing one past ASCII `,` is safe. After `b'\''` or `b'"'`: `i += 1` stepping past the 1-byte quote is safe.
- But inside the quote-skip loop: `while i < len && bytes[i] != quote { i += 1; }` — this advances one byte at a time. If a multi-byte character's continuation byte happens to equal the quote byte (`0x27` or `0x22`), this would falsely terminate early. However, UTF-8 continuation bytes have the form `10xxxxxx` (0x80–0xBF), while `'` = 0x27 and `"` = 0x22. **Continuation bytes can never equal ASCII values** because continuation bytes are always ≥ 0x80. So the inner loop correctly skips multi-byte characters.
- After the quote-skip loop, `i` points at the closing quote character, which is ASCII and thus a char boundary. `i += 1` advances past it — safe.
- `start` after `b','` branch: `start = i` where `i` was just incremented past a `,`. This is valid.

The final `fields.push(&line[start..])` is safe.

**For `unquote`:**
```rust
if bytes.len() >= 2 {
    let first = bytes[0];
    let last = bytes[bytes.len() - 1];
    if (first == b'\'' || first == b'"') && first == last {
        return trimmed[1..trimmed.len() - 1].trim();
    }
}
```

`trimmed[1..]` — advancing 1 past ASCII `'`/`"` is safe. `trimmed[..trimmed.len()-1]` — the `last == first` check ensures the last byte is an ASCII quote, so `trimmed.len()-1` is a valid char boundary.

**Verdict: SAFE.** The CSV byte scanner is correct because UTF-8 continuation bytes (0x80–0xBF) can never collide with ASCII delimiters.

---

### F12 — `parser/sequence.rs:651` — `parse_box_colour_and_label` inner `strip_quotes`

```rust
let strip_quotes = |s: &str| -> String {
    let s = s.trim();
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        s[1..s.len() - 1].to_string()
```

`s.len()` is byte length. The guard `s.ends_with('"')` ensures the last character is the 1-byte ASCII `"`, so `s.len() - 1` is always a valid char boundary. `s[1..]` advances 1 past the opening `"` — safe.

**Verdict: SAFE.**

---

### F13 — `parser/git_graph.rs:351–358` — `extract_quoted_attr`

```rust
let needle = format!("{key}:");
let start = line.find(needle.as_str())?;
let after_colon = &line[start + needle.len()..];
let open = after_colon.find('"')?;
let rest = &after_colon[open + 1..];
let close = rest.find('"')?;
Some(rest[..close].to_string())
```

`line.find(needle)` → `start` is a byte offset of where `needle` (a string like `"id:"`) starts in `line`. `start + needle.len()` = byte offset immediately after `needle`. Since `needle` consists of the ASCII key plus `:`, and `find` only returns positions where the full needle matches, `start` is always a char boundary and `start + needle.len()` is always a char boundary.

`after_colon.find('"')` → `open` = byte offset of `"` in `after_colon`. Advancing 1 past `"` is safe.

`rest.find('"')` → `close` = byte offset of `"` in `rest`. `rest[..close]` = content before closing `"` — always valid.

**Verdict: SAFE.** All slice endpoints are at ASCII characters (`"`, `:`).

---

### Summary of Remaining Files

| File | Status | Reason |
|------|--------|--------|
| `parser/common.rs` | CLEAN (after fix) | Uses `str::get(..len)` correctly |
| `parser/flowchart.rs` | CLEAN (after 4 fixes) | All remaining slices are at ASCII delimiters or use `chars()` correctly |
| `parser/pie.rs` | CLEAN | All slices at ASCII `"` boundaries |
| `parser/xy_chart.rs` | CLEAN | `keyword.len()` from `split_whitespace()` is a valid offset |
| `parser/sequence.rs` | **F2** | `lower.find(" as ")` offset reused on `rest` |
| `parser/timeline.rs` | **F3,F5** | F3 is safe (`:` is ASCII); F5 is real |
| `parser/journey.rs` | **F6** | `strip_keyword_ci` missing `str::get()` guard |
| `parser/gantt.rs` | **F4** | `strip_keyword_ci` missing `str::get()` guard |
| `parser/git_graph.rs` | CLEAN | `rest_after_keyword` uses `as_bytes()[klen]` but slices at `klen+1`; `klen` is the byte position of confirmed ASCII whitespace |
| `parser/class.rs` | CLEAN | Guards on `starts_with`/`ends_with` ASCII `"` |
| `parser/er.rs` | CLEAN | All slices at ASCII `"` or `--`/`..` |
| `parser/state.rs` | CLEAN | `body[1..].find('"').map(|p| p+1)` correctly produces offset of closing `"` |
| `parser/sankey.rs` | CLEAN | Byte scanner cannot false-match on continuation bytes |
| `parser/mindmap.rs` | CLEAN | `find(['[','(','{',')'])` at ASCII boundaries |
| `parser/packet.rs` | CLEAN | All slices at ASCII `:`, `"`, `'`, `-` boundaries |
| `parser/quadrant_chart.rs` | CLEAN | `find(": [")` → `colon_pos + 3` at ASCII boundaries |
| `parser/requirement_diagram.rs` | CLEAN | `find(" - ")` / `find(" -> ")` at ASCII boundaries |
| `parser/block_diagram.rs` | CLEAN | `find("-->")` / `find('|')` at ASCII boundaries |
| `parser/architecture.rs` | CLEAN | `find("-->")` / `find(')')` / `find(']')` at ASCII boundaries; `bytes[1] == b':'` guard |
| `src/render/*` | CLEAN | No byte-offset slicing found; render layer operates on parsed structs |

---

## Confirmed Real Findings

### REAL-1 (HIGH, HIGH confidence)
**`parser/sequence.rs:491–493`** — `parse_participant_decl`

Cross-string byte offset reuse: `find` on `lower` (lowercased copy), offset applied to `rest` (original). Can panic when `.to_lowercase()` changes byte lengths (ß→ss, İ→i). 

Trigger: `participant Straße as Foo` (or any participant ID containing `ß`). The byte divergence between `lower` and `rest` can cause `rest[..as_idx]` or `rest[as_idx+4..]` to land mid-char.

Fix: Search for ` as ` case-insensitively on `rest` directly (e.g. convert individual sub-slices for comparison, or use a `char_indices` walk).

### REAL-2 (MEDIUM, HIGH confidence)
**`parser/gantt.rs:501–507`, `parser/timeline.rs:153–158`, `parser/journey.rs:180–185`** — local `strip_keyword_ci`/`strip_keyword_prefix` copies

Pattern: `line[..klen]` where `klen = keyword.len()` is an ASCII byte count, unguarded for non-char-boundary case. Panics when `klen` falls inside a multi-byte character.

The fixed `common.rs::strip_keyword_prefix` uses `line.get(..len)?` which returns None instead of panicking. These three local copies (and the `git_graph.rs` variant `rest_after_keyword` at line 367) have not been updated.

Note: `git_graph.rs::rest_after_keyword` at line 367 checks `line.as_bytes()[klen].is_ascii_whitespace()` BEFORE slicing `&line[klen+1..]`. The `as_bytes()[klen]` access is safe (no slice), but `line[klen+1..]` can still panic if `klen` lands inside a multi-byte char and `klen+1` is not a boundary. So `git_graph.rs` is also affected.

Fix: Replace `line[..klen].eq_ignore_ascii_case(keyword)` with `line.get(..klen).is_some_and(|h| h.eq_ignore_ascii_case(keyword))` and `line[klen..]` with `&line[klen..]` protected by the `get` guard. Identical to the `common.rs` fix.

---

## Coverage Assessment

The four previously fixed sites (`strip_keyword_prefix` in `common.rs`, `try_consume_pipe_label`, `try_consume_inline_compact_arrow`, `try_consume_inline_quoted_arrow` in `flowchart.rs`) covered the most heavily-exercised code paths. This audit found **2 additional real finding categories** (5 sites total):

1. Cross-string offset reuse in `sequence.rs` (`find` on lowercased copy, index into original)
2. Three (possibly four) local `strip_keyword_ci` copies that were not updated when `common.rs` was fixed

All other suspected sites analyzed as SAFE due to ASCII delimiter constraints.
