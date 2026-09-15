# Map Studio rebuild — Phase 0 (fail-closed apply/flash path) and Phase 1 (CFF reference corpus)

Date: 2026-09-15. Status: draft for review. Supersedes nothing; follows the 14 Sep 2026 Map Studio audit.

## 1. Why

The audit found that Map Studio & Tuning is a simulation: `BoschMapDetector` fabricates six of seven maps at fixed offsets, the checksum solver is not Bosch's, and the only things keeping the resulting `.sgmod` packages off a real ECU are two accidents (a chassis-string mismatch and a DID F191/F192 mismatch). The user chose a full rebuild plus an "undo an applied mod" feature.

Two facts change the plan since the audit:

1. **A real firmware corpus exists locally.** `/home/kim/Downloads/SDflash_2015.12-2019.12` (70 GB, temporary location) holds 15,879 official Mercedes `.CFF` flash containers. Both EDC16 engine families relevant here are covered for the W211/S211:
   - `PKW/CR3_UP/` — **OM646** 2.2 CDI, Bosch **EDC16C31**, the family in `profiles/mercedes/w211_om646_edc16.json` and the README's "S211 Estate OM646". 73 software versions for BR=211: 25 full images (five segments, ~1.25 MB) and 48 calibration-only datasets (one segment of 256 KB or 320–512 KB).
   - `PKW/CR4/` — **OM642** 3.0 V6 CDI, Bosch **EDC16CP31**. 31 software versions for BR=211, all full images (five segments, ~1.48 MB), plus byte-identical twins under `PKW/MSG/`.
   Both families are big-endian PowerPC (MPC5xx) with 16-bit big-endian calibration tables. The containers are unencrypted and fully decoded (section 5).
2. **The format was reverse-engineered and adversarially verified** on all 15,879 files. A Rust parser can be written to a specification with test vectors, not to guesses.

The user will also be able to read the live ECU via Tactrix OpenPort (a later phase, A0). A live dump can then be validated by diffing against the corpus image with the same software number.

## 2. Scope of this spec

- **Phase 0** — make the `.sgmod` apply path and the flashing worker fail closed, so that removing the accidental gates later cannot unblock a corrupting write. No detector work. No ROM data needed.
- **Phase 1** — a `CffFile` parser, an `EcuImage` sparse-address model, a read-only local corpus with a metadata manifest, and a `sterngate corpus` CLI. Firmware bytes never enter the repository.

Out of scope (later specs): Phase 2 real map detector (validated against the corpus), Phase 3 live ROM dump over OpenPort, feature B "undo an applied mod", SecurityAccess for `ModRunner`, F191→F192 alignment in `ModRunner`, `FlashState::Failed` remaining locked after erase.

## 3. Decisions taken (override any of these in review)

| # | Decision | Rationale |
|---|---|---|
| D1 | `--force` (CLI only) relaxes chassis and HW-ID whitelist checks **only**, and is refused outright for any package containing `PatchFlashMap` or `DtcMask`. Voltage, provenance and byte preconditions are never bypassable. | Integrity is a checksum, not a signature; a forged package plus `--force` would reproduce the original hole. |
| D2 | `force` is removed from the REST payload and the MCP tool; the REST payload gets `deny_unknown_fields` (axum returns **422** for unknown fields). | One JSON boolean must not switch off every gate over the network. |
| D3 | Battery voltage for `mod apply` comes from the OpenPort Pin-16 ADC through a new trait method; SocketCAN/mock/simulated adapters cannot measure and are refused. No manual override flag in Phase 0. | A constant that always passes the interlock is no interlock. |
| D4 | One `MapProvenance` enum (`Unverified` default, `Synthetic`, `Fallback`, `Scanned`) lives in `calibrator/map.rs`, is a required field on `EcuMap` and an integrity-covered optional field on `PatchFlashMap`/`DtcMask`. In Phase 0 only the SVBL and torque-limiter scan hits are stamped `Scanned` (found by scanning and verified ROM-backed); every Stage package also touches a `Synthetic` map, so no package can be emitted until the Phase 2 detector replaces the heuristics. | Phase 2 defines what "scanned" means; until then every generated flash patch is non-executable. |
| D5 | `StageGenerator` refuses to emit a package if any map it would patch is not `Scanned` and ROM-backed (abort whole package, no partial). `generate_dtc_kill` returns an error until the detector understands the DTC table; Stage 2 no longer emits `DtcMask`. | A unique 2-byte pattern hit is not evidence of a DTC table. |
| D6 | `.sgmod` integrity version 2 covers `target` + `actions` + `rollback_actions`; version ≠ 2 is refused everywhere (inspect included). `ModMetadata` stays uncovered. The one in-repo package is regenerated. | Accepting legacy anywhere preserves an unprotected-target format. |
| D7 | Flasher preflight compares the ECU's **F192** supplier number with exact equality (after trim/uppercase, both ≥ 8 chars); read failure or disconnect fails closed. `ModRunner` stays on F191 in Phase 0 but its read failure now refuses when a whitelist is declared. | Bosch numbers differ in the last digits between hardware variants; a prefix rule accepts sibling variants. |
| D8 | The ECU checksum routine id (`0x0202`) and OK status (`0x00`) remain constants, flagged as unverified on real firmware. ECUReset failure after a verified checksum is `Completed` with a warning. | Loud post-write failure is the fail-closed direction; the manifest fields come with Phase 3. |
| D9 | The corpus is a separate read-only root (`--corpus` > `STERNGATE_CFF_CORPUS` > `./firmware_corpus`), never the vault. Vault entries gain `stageable: false` for containers and `vault_stage` refuses them. The manifest (hashes, part numbers, chassis) stays local and gitignored. | Today a `.cff` in the vault would be flashed as raw bytes. |
| D10 | Only `.CFF` is parsed. `.SMR-F` (a different, XOR-obfuscated container) and `.bin` are rejected with distinct errors. | No W211 CR3/CR4 software exists only as SMR-F. |
| D11 | Lint gate as it really is: `[workspace.lints]` declares pedantic but no crate opts in, so the effective gate is default clippy with `-D warnings`. New code still uses `try_from`/`from_be_bytes` so a later opt-in does not regress. | Verified: `cargo clippy --workspace --all-targets -- -D warnings` passes today; ~120 pedantic warnings exist. |

## 4. Phase 0 — fail-closed apply and flash path

### 4.1 Units and responsibilities

| Unit | File | Owns |
|---|---|---|
| Mock fault injection | `crates/sterngate-hal/src/mock.rs` | `with_failing_services(&[u8])` → NRC 0x31 for listed SIDs; a real `0x23` reply (`63` + n zero bytes, n ≤ 6, else NRC); `0x34/0x36/0x37/0x28/0x85` answers; `0x2E` echoes the DID from the first frame; `3E 80` suppressed; tester Flow Control ignored |
| Provenance types | `crates/sterngate-core/src/calibrator/map.rs`, `modpack/mod.rs` | `MapProvenance`; `EcuMap.provenance` (required, no serde default); `EcuMap::is_rom_backed(&self, rom)`; `provenance` on `PatchFlashMap`/`DtcMask` with `#[serde(default, skip_serializing_if = "MapProvenance::is_unverified")]` so legacy bytes stay identical |
| Detector honesty | `calibrator/detector.rs` | Labels: SVBL hit `Scanned`, SVBL fallback `Fallback`, torque hit `Scanned`, torque tail `Fallback`, the five never-read-the-ROM finders `Synthetic`. `find_dtc_offset` bounds-safe (`rom.get`), returns `None` on short ROMs, past-EOF mask, or ambiguous (non-unique) hits |
| Producer gate | `calibrator/stage.rs` | `require_rom_backed(map, rom)` before every patch; `generate_stage2` gets the missing empty-maps guard; `generate_dtc_kill` → `Err("DTC table location unsupported until detector rebuild")`; `min_battery_voltage` 12.5 |
| Package constructor | `modpack/mod.rs` | `FLASH_WRITE_MIN_VOLTAGE = 12.5` (exported); `create` refuses flash-writing packages below it (`is_nan() || < floor`); `MOD_INTEGRITY_VERSION = 2`; `ModIntegrity.version` (`#[serde(default)]`); `canonical_payload_bytes(target, actions, rollback)`; `verify_and_repair` checks version **before** FEC, repairs `target` too |
| Armor | `modpack/armor.rs` | Raw-JSON branch of `decode_from_armor` returns `Err` on an invalid report, like the armored branch |
| Runner | `crates/sterngate-protocol/src/modrunner.rs` | `TargetFingerprintPolicy { Enforce, BypassUnsafe }` replaces `force: bool`; step 1b provenance gate before any bus traffic; voltage floor `max(target.min, 12.5)` for flash-writing packages regardless of policy; `BypassUnsafe` refused for flash-writing packages; `PatchFlashMap` requires non-empty `expected_original_data` of the same length as `data`, read must succeed and match exactly; `DtcMask` read must succeed and equal `original_mask`; HW-ID read failure refuses when a whitelist is declared; bitmask `WriteDid` read failure refuses; overlapping address ranges within one package refused; `inspect_compatibility` mirrors the fail-closed rules |
| Entry points | `server/src/routes/community_mods.rs`, `mcp/src/tools/community_mods.rs` + `specs.rs`, `cli/src/args.rs` + `commands/modcmd.rs`, `static/js/app.js` | VIN required everywhere (server 400, MCP error, CLI `--vin` required, UI sends `activeVehicleVin` or disables); `force` removed from REST/MCP (D2); CLI voltage from the trait (D3); MCP requires `battery_voltage` |
| Voltage source | `crates/sterngate-hal/src/interface.rs`, `openport.rs`, `server/src/routes/telemetry.rs` | `async fn measure_battery_voltage(&mut self) -> Result<Option<f32>>` defaulted to `Ok(None)`; OpenPort hardware backend overrides, simulated backend returns `None`; telemetry sampler fills `battery_voltage` from it so the Web UI apply path works on real hardware |
| Flasher | `crates/sterngate-protocol/src/flasher.rs`, `uds.rs`, `isotp.rs` | See 4.2 |
| Vault | `crates/sterngate-core/src/flash.rs`, `server/src/routes/flashing.rs` | `FirmwareVaultEntry.stageable` (`#[serde(default)]`, true only for raw `.bin/.rom/.fls`); `.cff` sniffed and marked non-stageable with empty ids; `find_upgrade_recommendation` skips non-stageable; `vault_stage` returns 400 for a container |

### 4.2 Flasher hardening (worker, UDS, ISO-TP)

Ordered so that loud failures never replace silent ones on real hardware before the transport is safe:

1. **ISO-TP no-panic** (`isotp.rs`): every received-frame index becomes a length-checked accessor returning `IsoTpError`; FF shorter than 8 bytes, FF announcing < 8 bytes, short CF, FC shorter than 3 bytes are errors. Release profile is `panic = "abort"`, so today one short frame on 0x7E8 mid-erase kills the process.
2. **P2\* pending** (`uds.rs`, `isotp.rs`): `set_timeout`/`timeout` on the channel; `diagnostic_session_control` parses P2/P2\* (widen to `u64` before `* 10`); on the first NRC 0x78 the receive timeout becomes P2\* + 500 ms and is restored on every exit path (guard type). The 100 ms sleep goes.
3. **One change**: whole-sequence interface lock + keep-alive + every step uses `?` + mock answers:
   - `execute_flash` takes the interface lock once after preflight and one `UdsClient` for all steps through `0x11`.
   - `S3KeepAlive`: `tester_present_suppressed` (fire-and-forget `3E 80`) sent when ≥ 1500 ms have elapsed since the last exchange, checked before every request and each `0x36` block; a stray `7E` reply is discarded in `send_request`.
   - `run_programming_sequence` returns `Result`; erase/checksum routine replies are parsed (`71`, echoed sub-function and routine id, status byte required, status ≠ 0 → `ChecksumMismatch`) and the checksum runs before ECUReset; `RequestDownload` is built from the manifest (address, length; preflight requires `flash_length == rom.len()`); `maxNumberOfBlockLength` is parsed and the chunk size clamped to it and to the ISO-TP limit; each `0x36` reply must echo the block counter.
   - On error: state `Failed` (never stuck `Locked`), message says "ECU untouched" before erase or "FLASH FAILED AFTER ERASE — keep ignition on" after it.
   - Routes that lock the interface without a 423 guard (`mods_inspect`, read DTCs, clear DTCs, scan) get the guard, otherwise they hang for the whole flash.
   - Mock gains the answers listed in 4.1 so `sterngate-mcp` full-flash test stays green.
4. **Flow Control BlockSize / WAIT / OVFLW** honoured in `send_payload` (N_WFTmax = 8).
5. **Preflight HW identity** (D7): `read_supplier_hw_id` (F192, ASCII or BCD-rendered) + `hw_id_matches` exact; `inspect_rom` starts with `can_flash = false`, a connected ECU that does not answer is `Unknown`; MCP `sterngate_verify_flash_staging` runs the real preflight or reports "not evaluated" instead of a fabricated PASSED line.

A `ScriptedInterface` test double (responders keyed by request bytes, records every frame, multi-frame replies after the tester's FC, configurable `is_connected`) lives in `flasher.rs` tests; timing tests use `tokio::test(start_paused = true)`.

### 4.3 Data flow after Phase 0

```
.sgmod JSON/armor ──decode_from_armor──▶ verify_and_repair (version==2, CRC, SHA, FEC over target+actions+rollback)
        │ invalid ──▶ Err (all entry points)
        ▼
apply_mod(iface, mod, vin(required), volts(measured), policy)
  1b provenance: any PatchFlashMap/DtcMask not Scanned ──▶ Err before bus traffic
  1c policy: BypassUnsafe && writes_flash ──▶ Err
  2  volts < max(target.min, 12.5 if writes_flash) ──▶ Err (policy ignored)
  3  chassis (bypassable)   4 F191 whitelist: read failure with whitelist ──▶ Err (Enforce)
  5  per action, before first write: read original bytes; missing/short/mismatch ──▶ Err
  6  writes
```

### 4.4 Tests (failing-first, per unit)

- Mock: `0x23` returns requested length; failing-services returns NRC; `3E 80` suppressed; FC ignored; `0x34/0x36/0x37` answers.
- Core: `find_dtc_offset` never panics on empty/short ROMs, returns `None` past EOF and on ambiguous hits; `scan_rom` labels exactly one `Scanned` map on the existing test ROM; `is_rom_backed` rejects out-of-range/mismatch/empty; Stage 1/2 refuse when any targeted map is not ROM-backed; `require_rom_backed` rejects a mislabelled `Scanned` map; `dtc_kill` refuses; `create` rejects flash writes below 12.5 V and NaN; integrity covers `target` (mutating `tx_id`, `chassis`, `min_battery_voltage`, `compatible_hw_ids` invalidates); legacy version refused; FEC repairs a one-symbol `target` corruption; provenance default keeps legacy bytes identical; armor raw-JSON branch rejects a corrupt package.
- Runner: read failure refuses (`PatchFlashMap`, `DtcMask`, bitmask `WriteDid`, HW whitelist); short read refuses; missing `expected_original_data` refuses; matching precondition applies (≤ 6 bytes); synthetic provenance refused with `0x10` in the failing set (proves no bus traffic); `BypassUnsafe` never bypasses voltage/preconditions and is refused for flash packages; author-declared 9.0 V floor still refused at 12.0 V; overlapping ranges refused.
- Server/MCP/CLI: `force:true` → 422; missing VIN → 400/error; missing voltage → 400/error; MCP spec has no `force`; `Cli::try_parse_from` requires `--vin`; tuning stage1/stage2/kill_dtc tests rewritten to assert refusal (tuning routes map `PreFlightCheckFailed` to 422); vault marks `.cff` non-stageable and `vault_stage` refuses it.
- Flasher (ScriptedInterface): each ISO-TP malformed frame is an error not a panic; P2\* wait and restore; keep-alive sent when idle > 1500 ms and never between FF and last CF; security-access NRC aborts before erase; RequestDownload NRC aborts after erase with the post-erase message; block echo mismatch aborts; checksum status ≠ 0 / NRC / missing byte all abort before `0x11`; happy path completes with the exact frame order; BS honoured; HW mismatch/read error/disconnect fail closed; exact match passes; `inspect_rom` connected-but-silent is not flashable.
- HAL: `VirtualCanInterface` and simulated OpenPort report `Ok(None)` for voltage.

Baseline today: 98 tests pass; every step keeps the workspace green.

### 4.5 Implementation order (one commit per step)

1. Mock fault injection + real `0x23`; `find_dtc_offset` bounds fix; armor raw-JSON branch.
2. Unified `MapProvenance` (types, detector labels, `stage.rs` threading, `modcmd.rs` destructuring, test literal).
3. Runner hardening series: provenance gate, `PatchFlashMap`/`DtcMask`/HW-whitelist/bitmask fail-closed, overlap check; positive-path test rewritten (`Scanned`, expected `[0;3]`, mask `0x00`).
4. `TargetFingerprintPolicy` with D1 amendments; `FLASH_WRITE_MIN_VOLTAGE` enforced in the runner; server `deny_unknown_fields` (422 test); MCP/CLI/UI/test updates.
5. Producer hardening: `require_rom_backed`, no `DtcMask` emission, `create` floor; tuning tests in core/server/MCP rewritten to assert refusal.
6. Integrity v2; regenerate `profiles/mods/amg_needle_sweep.sgmod`; community-mods skill sample.
7. Entry-point inputs: VIN required; voltage trait + OpenPort override + CLI + MCP + telemetry wiring + UI.
8. Flasher in the order of 4.2.
9. Vault fail-closed half of D9. This step creates the `cff` module with only `sniff` (prologue magic + stub `0x05ED`) and the `stageable` field; the rest of the module is Phase 1.
10. Docs: `community-mods`, `sterngate-mcp`, `ecu-tuning`, `safe-flashing`, `sterngate-ops` skills; AGENTS.md/CLAUDE.md module lists.

Behavioural consequences to state plainly in the docs: Stage 1/2 and DTC-kill generation refuse on every ROM until Phase 2; `sterngate mod apply` refuses on SocketCAN/mock (no voltage measurement); all previously generated `.sgmod` files are refused (integrity v1).

## 5. Phase 1 — CFF reference corpus

### 5.1 The container format (verified on 15,879 files; all integers little-endian unless marked BE)

**Prologue** (0x000 …): five LF-terminated `KEY:value` lines in fixed order — `CFF-TRANSLATOR-VERSION` (02.01.03 or 02.01.01), `DATE` (`D.M.YYYY`, no zero padding), `FINGERPRINT` (dotted quad; identical across re-translations of the same dataset, so a dedupe key), `CFF` (family label; may be a comma list; equals the directory name in 12k files but not all), `LANGUAGE:ORIGINAL`. 96–204 bytes, then NUL padding to 0x400.

**Stub** (0x400, 16 bytes): u16 magic `0x05ED`, u16 year, u8 month, u8 day (must equal `DATE`), 10 constant bytes `00 00 00 00 01 11 11 11 11 11`.

**Main header**: u32 `header_size` at 0x410; `BASE = 0x414`; u32 `flags` at BASE (only `0x001FFFF7` or `0x001FFFFF`); u16 extended flags at 0x418 (always 0); then one field per set bit in ascending bit order, int32 each except bit 20 (int16). String and table offsets are relative to BASE. Fields: 0 name, 1 translator command line (optional `+sc CCC|C`, `+ss` blob of 128 or 20 `$xx` tokens, `+sk A<number>`), 2 int (=2), 3 int (=3, only with `0x1FFFFF`), 4 author, 5 ISO timestamp, 6 SW version, 7 tool version string, 8 tool version int (independent of 7), 9 comment, 10 `ecu_count` (=1), 11 ECU table, 12 `block_count` (=1), 13 block table, 14 CTF struct, 15 `string_block_size`, 16 number of family labels (= comma count of the `CFF` line), 17 their name table, 18/19 count+table (unknown), 20 int16 (6/255/4/1). `header_end = BASE + header_size`.

**Offset tables**: array of int32 offsets relative to the table start; `tbl[0] == 4 * count` always.

**ECU entry**: u32 flags (`0xC1D`, `0xC1F`, `0x1D`), **no** u16 prefix, int32 fields by bit from +4, offsets relative to the entry; bit 0 name (`<dataset>_<sw>_001`). Informational only.

**Block entry**: u32 flags + u16 extended (0), fields by bit (int32 except bit 11 = int16), offsets relative to the entry. Shapes: NORMAL (bit 4 `data_length`, bit 9 `segment_count`, bit 10 segment table; 15,076 files), STUB (no bits 4/9/10; empty payload; 793), EXTERNAL (bit 6 external file name, no bit 4; empty payload; 10, companion may be missing). Other fields: bit 0 block name (`<sw>_<dataset>`), bits 14/15 ident checks (u16 flags `0x19`/`0x1B`, bit 1 adds an int32; value struct `{u16 type ∈ {2,5}, u32 len, u32 off}`), bits 16/17 security table (class string then the `+ss` blob), bit 18 kind (free string: UNKNOWN/CODE/DATA/BOOT/…), bit 20 project name, bits 11–13 `(int16 4, int32 1, off → u16[1] ∈ {1,0,240})`, bit 24 rare.

**Segment entry**: u16 flags (`0x000B` or `0x001B`), **u32** address, **u32** length, int32 name offset (relative to entry; `0x0E` or `0x12`), and for `0x001B` one extra int32. Names are usually `0x%08X` but not always: never derive the address from the name, never assume a stride.

**CTF struct** (BASE + bit 14): u16 flags `0x00BF`; bit 0 `0x2774`; bit 3 = `!crc32(file[header_end .. header_end + string_block_size])`.

**Ident strings** (header_end …, `string_block_size` bytes): offset table + NUL-terminated UTF-8 strings (`ORIGINAL`, ``, ECU name, version, description such as `OM646 DE 22 LA - 75kW -, BR W 211, EU4 o. DPF`, a `KEY=value\r\n` block with `HW_PARTNUMBER`, `DC-Nr. (LU-Nr.)`, `HW-Variante`, `HW-Stand`, `ED-Nummer`, `DPF`, and `TNR: … / SWV: …`). 0 or 1 slack byte follows; the size field is authoritative.

**Payload**: `payload_start = 0x414 + header_size + string_block_size` (always even). Segments are concatenated in table order with no framing: `file_offset(i) = payload_start + Σ length[0..i)`. Invariant: NORMAL → `Σ length == data_length == file_len − 4 − payload_start`; STUB/EXTERNAL → `payload_start == file_len − 4`. Payload bytes are in ECU byte order (opaque to the parser).

**Trailer**: last 4 bytes u32 LE = `!crc32(file[0 .. len−4])` (zlib polynomial), i.e. `crc32(whole file) == 0xFFFFFFFF`.

**Not CFF**: `.SMR-F` starts with `52 90 D4 30 67 14 7E 47 81 F2 3C 4B 73 F0 F7 37`; rejected with `CffError::NotCff("SMR-F")`.

**What the payload is** (family knowledge, used by tests and Phase 2, not by the parser): CR4 W211 images carry five download runs covering four Bosch blocks (A code `0x040000`, B calibration overlay `0x170000`, C main calibration `0x190000`, D code `0x404000`), each with a header chain (`+0x0C` links A→D→C→B→0, `+0x08` size, `+0x10` Bosch SW number string), a `0x90`-byte trailer whose last BE u32 is `crc32` of the block over a 0xFF-filled image, and additive header checksums (`Σ words[+0x00..+0x2B] = 0xD01FE500`). Block bounds are family-specific (CR4_NFZ has B at `0x160000`, C at `0x180000`) and must be discovered from the chain, not hard-coded. CR3 (OM646) full images use `0x008000/0x06FF00/0x820000/0x87FF00/0x8FDF00`; dataset-only containers carry one segment at `0x880000` (256 KB), `0x8C0000`, `0x020000` or `0x800000`. Code is BE PowerPC (blr/mflr census), calibration tables are BE `[nx][ny][x-axis][y-axis][data]`. There is no `0281…` Bosch hardware string in any payload, so `FirmwareSignatures::extract` yields no HW id on real images; identity comes from the container ident strings.

### 5.2 Units

| Unit | Location | Public API (Phase 1) |
|---|---|---|
| Parser | `crates/sterngate-core/src/cff/parser.rs` | `CffFile::parse(&[u8]) -> Result<CffFile, CffError>` (strict: CRC, CTF CRC, stub date, known flags, partition invariant, bounds); `CffFile::inspect(&[u8])` (lenient for indexing: records `container_crc_valid`, `ident_crc_valid`, unknown flags as warnings; still errors on structural truncation); `sniff(&[u8]) -> bool`; types `CffFile { prologue: CffPrologue, header: CffHeader, ecu: CffEcuEntry, block: CffBlock, ident_strings: Vec<String>, ident_fields: BTreeMap<String,String>, payload_start, file_size, file_sha256, container_crc_valid, ident_crc_valid, warnings }`, `CffBlock { name, kind, shape: CffBlockShape::{Normal{data_length}, Stub, External{file_name}}, segments: Vec<CffSegment>, ident_checks, security, raw: BTreeMap<u8,i64> }`, `CffSegment { address: u32, length: u32, file_offset: usize, sha256: String, name: String, flags: u16, extra: Option<i32> }`; `CffFile::segment_bytes(&self, bytes, i) -> Result<&[u8]>`; every unknown-bit field kept raw. `ecu_count` and `block_count` must both be 1 (the corpus has no counterexample); anything else is `UnsupportedFlags`. |
| Image | `crates/sterngate-core/src/cff/image.rs` | `EcuImage` sparse address map built from segments: `from_cff(&CffFile, bytes)`, `read(addr, len) -> Option<&[u8]>`, `materialize(range, fill=0xFF) -> Vec<u8>`, `crc32_range(range) -> u32`, `segments()`. No byte-swapping. |
| Errors | `crates/sterngate-core/src/error.rs`, `cff/error.rs` | `CffError { NotCff(&'static str), Truncated{offset, need}, BadMagic, BadOffset{what, offset}, UnsupportedFlags{what, value}, PartitionMismatch{..}, CrcMismatch{stored, computed}, IdentCrcMismatch, DateMismatch, TooManyEntries{what, count} }` (Clone + PartialEq + Eq); `SterngateError::Cff(#[from] CffError)`. No panics: every read bounds-checked; counts capped (segments ≤ 4096, tables ≤ 64 KB). |
| Corpus | `crates/sterngate-core/src/cff/corpus.rs` | `CffCorpus::default_root()` (env `STERNGATE_CFF_CORPUS`, else `firmware_corpus`), `root_from(Option<&str>)`; `index(root) -> CffCorpusManifest` walks `*.cff`/`*.CFF` case-insensitively, `inspect`s each, never aborts on one bad file, reads `akttab.csv` when present at `<root>/PKW/akttab.csv` or `<root>/akttab.csv` (latin-1, `;`, backslash and slash paths, case-insensitive resolution, paths relative to the directory holding the index) to attach `hw_partnos`, `chassis` conditions and ECU label; `write_manifest`/`load_manifest` (`<root>/corpus_manifest.json`); `find_by_sw`, `find_by_fingerprint`, `find_by_segment_sha256`. Entry fields: `relative_path`, `file_size`, `file_sha256`, `family` (`CFF` line), `sw_number` (file stem), `hw_partnos`, `chassis`, `ecu_label`, `fingerprint`, `date`, `description`, `hw_fields` (the `KEY=value` block), `segments[{address,length,sha256}]`, `container_crc_valid`, `ident_crc_valid`, `shape`. |
| Synthesizer | `crates/sterngate-core/src/cff/synth.rs` | `#[doc(hidden)] pub CffBuilder` emitting the exact layout of 5.1 (prologue, stub, bitflag header, ECU entry, block, segment table, CTF CRC, ident strings, payload, trailer CRC) with knobs for shape, flags, hostile counts, flipped bytes. Always compiled so server/CLI tests can use it. |
| CLI | `crates/sterngate-cli/src/args.rs`, `commands/corpus.rs` | `sterngate corpus index [--root] [--rebuild]`, `list [--sw] [--hw] [--chassis] [--family]`, `inspect <file>`, `extract <file> --segment N --out <path> [--manifest-out <path>]` (strict parse; sidecar `FlashPackageManifest` with `flash_start_address`/`flash_length` from the segment, `expected_hw_id` from the container `HW_PARTNUMBER`, never hard-coded fallbacks); global `--corpus <path>`. |
| Vault | `crates/sterngate-core/src/flash.rs` | Uses `cff::sniff` (shared with Phase 0 step 9). |
| Docs | `.agents/skills/{safe-flashing,sterngate-ops,sterngate-mcp,vehicle-profiles,ecu-tuning,bench-recovery}/SKILL.md`, `AGENTS.md`/`CLAUDE.md`, README | Corpus section, `stageable`, CBF vs CFF distinction, `sterngate corpus` examples; `.gitignore` gains `*.CFF`, `*.smr-f`, `*.SMR-F`, `/firmware_corpus/`. |

Not in Phase 1: Bosch block-chain discovery as public API, table decoding, SMR-F de-obfuscation, any server route or MCP tool for the corpus.

### 5.3 Data flow

```
SDflash tree (read-only) ──corpus index──▶ corpus_manifest.json (metadata only, gitignored)
.CFF bytes ──CffFile::parse──▶ segments (addr,len,file_offset,sha256) ──EcuImage──▶ read/materialize/crc32_range
                                     └──corpus extract──▶ raw segment .bin + FlashPackageManifest sidecar (into the vault, stageable)
```

### 5.4 Error handling

Strict `parse` is the only path that yields bytes for extraction or staging. `inspect` exists so a corpus index can describe broken or unusual files without hiding them. Every error names the offset and what was expected. Truncated or bit-flipped inputs must never panic (prefix fuzz test over every length of a synthetic file plus, when the corpus is present, over real W211 files).

### 5.5 Tests

Layer 1, hermetic (always): parser round-trip through `CffBuilder`; every-prefix no-panic; bad magic; SMR-F magic rejected; flipped payload byte → `CrcMismatch`; flipped ident byte → `IdentCrcMismatch`; stub/DATE mismatch; unknown header/segment flags; hostile counts; STUB/EXTERNAL shapes; segment offsets/hashes; `EcuImage` read across a gap fills 0xFF and `crc32_range` matches a hand-computed value; corpus index skips non-CFF, flags a broken file, round-trips the manifest, resolves akttab paths case-insensitively; CLI arg parsing; `extract` writes nothing on failure; vault marks a synthetic `.cff` non-stageable.

Layer 2, evidence (self-skipping unless `STERNGATE_CFF_CORPUS` is set): parse all 31 CR4 W211 and 73 CR3 W211 files; assert the test vectors from the format work (for `PKW/CR4/0164480002_001.CFF`: size 1,484,124, `payload_start 0x1258`, five segments `(0x40000,0x68B00) (0xBFF00,0x100) (0x170000,0x84600) (0x1FCF00,0x100) (0x404000,0x7C000)`, per-segment SHA-256 as recorded, trailer `9E 28 ED 50`, CTF CRC `0x2D33D94D`); assert the four CR4 Bosch block CRCs match on the 0xFF-filled `EcuImage` (`0x40000..=0xBFFFB → 0x06FCD653`, etc.); assert CR4/MSG twins have identical segment hashes and fingerprints; survey the whole tree with a cap (`STERNGATE_CFF_CORPUS_MAX`, default 500) and assert zero panics and ≥ 99 % container CRC validity. Golden metadata lives in `crates/sterngate-core/tests/fixtures/cff_golden.json` (hashes and offsets only).

## 6. Risks and open questions

- **Engine confirmation.** The repo says S211 OM646 (CR3/EDC16C31) while the UI default chassis is `W211 E280 CDI` (OM642/CR4). Phase 2 needs the actual family; both are in the corpus.
- **Header field semantics** for header bits 2/3/18–20 and block bits 1–3/11–13/21–24 are unknown; they are exposed raw and never trusted.
- **Corpus location is temporary** (`~/Downloads`). The env var/flag handles the move; the manifest stores relative paths.
- **`+ss` signature.** Whether the ECU verifies the 128-byte blob during download is unknown; irrelevant until Phase 3 flashing from corpus images, which this spec does not enable beyond `extract` + the existing vault path.
- **Checksum routine id/status** (D8) unverified on real EDC16.
- **Copyright.** No SDflash bytes, and no excerpt longer than 64 bytes, enter the repository; tests carry hashes and offsets only.

## 7. Deliverables

Phase 0: the ten commits of 4.5, each green on `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Phase 1: `cff` module with parser/image/corpus/synth, `sterngate corpus`, `.gitignore` and skill updates, evidence tests run once against the local tree with the survey output pasted into the PR description. Feature branch `feat/map-studio-rebuild-p0-p1`, merged to master after review. Two implementation plans follow this spec: one for Phase 0 (steps 1–10 of 4.5) and one for Phase 1 (5.2), so each can be reviewed and executed on its own.
