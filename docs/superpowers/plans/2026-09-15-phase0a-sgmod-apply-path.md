# Phase 0a — Fail-closed `.sgmod` apply path — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every path that can write a `.sgmod` package to an ECU refuse unless the package's provenance, integrity, target, voltage and live ECU bytes are all proven, so that removing the two accidental gates later cannot unblock a corrupting write.

**Architecture:** A single `MapProvenance` enum flows from the detector (`EcuMap`) through the generator into integrity-covered `ModAction` fields and is checked by `ModRunner` before any bus traffic. `ModRunner` replaces its `force: bool` with a `TargetFingerprintPolicy` that can only relax chassis/HW-whitelist checks and is refused for flash-writing packages. The `.sgmod` integrity block moves to version 2 covering the target filter. Entry points (REST, MCP, CLI, UI) stop fabricating VINs and voltages; voltage comes from the OpenPort ADC through a new `VehicleInterface` method.

**Tech Stack:** Rust 2021 workspace (tokio, serde, axum 0.8, clap 4, async-trait), `VirtualCanInterface` mock for tests, vanilla JS UI embedded via `include_str!`.

**Spec:** `docs/superpowers/specs/2026-09-15-map-studio-rebuild-phase0-phase1-design.md` (sections 3, 4.1, 4.3, 4.4, 4.5 steps 1–7 and 10). Phase 0b (flasher, vault) is a separate plan.

## Global Constraints

- Gate before every commit: `cargo fmt --all` then `cargo clippy --workspace --all-targets -- -D warnings` then `cargo test --workspace`. Baseline is 98 passing tests; the count only goes up.
- No `unwrap()`/indexing that can panic on request-driven paths (`AGENTS.md` invariant 6). Use `get`, `try_from`, `from_be_bytes`.
- Flash-write voltage floor is 12.5 V (`AGENTS.md` invariant 2). Constant name: `FLASH_WRITE_MIN_VOLTAGE`.
- axum 0.8 returns **422** for JSON that fails serde data validation (unknown field under `deny_unknown_fields`); hand-written `return (StatusCode::BAD_REQUEST, ...)` stays 400.
- Static UI assets are embedded with `include_str!`; a UI edit needs `cargo build` to take effect but no test harness change.
- `CLAUDE.md` is a symlink to `AGENTS.md`; edit `AGENTS.md` only.
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.
- Work on branch `feat/map-studio-rebuild-p0-p1` created from `master`.

## File map

| File | Responsibility after this plan |
|---|---|
| `crates/sterngate-hal/src/mock.rs` | Deterministic ECU stand-in: real `0x23` reply, per-SID fault injection |
| `crates/sterngate-hal/src/interface.rs` | `VehicleInterface` gains `measure_battery_voltage` (default `Ok(None)`) |
| `crates/sterngate-hal/src/openport.rs` | Hardware backend implements the measurement; simulated backend reports `None` |
| `crates/sterngate-core/src/calibrator/map.rs` | `MapProvenance`, `EcuMap.provenance`, `EcuMap::is_rom_backed` |
| `crates/sterngate-core/src/calibrator/detector.rs` | Honest provenance labels; bounds-safe, uniqueness-checked `find_dtc_offset` |
| `crates/sterngate-core/src/calibrator/stage.rs` | Refuses to emit patches for non-ROM-backed maps; no DTC-mask emission |
| `crates/sterngate-core/src/modpack/mod.rs` | `provenance` on flash actions, `FLASH_WRITE_MIN_VOLTAGE`, integrity v2 |
| `crates/sterngate-core/src/modpack/armor.rs` | Raw-JSON branch honours the integrity report |
| `crates/sterngate-protocol/src/modrunner.rs` | `TargetFingerprintPolicy`; fail-closed gates |
| `crates/sterngate-server/src/routes/community_mods.rs` | VIN required, `force` gone, `deny_unknown_fields` |
| `crates/sterngate-server/src/routes/telemetry.rs` | Fills `battery_voltage` from the interface |
| `crates/sterngate-server/src/routes/tuning.rs` | Maps `PreFlightCheckFailed` to 422 |
| `crates/sterngate-mcp/src/tools/community_mods.rs`, `specs.rs` | `force` rejected; `vin`/`battery_voltage` required |
| `crates/sterngate-cli/src/args.rs`, `commands/modcmd.rs` | `--vin` required; voltage from the interface; `--force` narrowed |
| `crates/sterngate-server/static/js/app.js` | Apply sends the active VIN and measured voltage, no `force` |
| `profiles/mods/amg_needle_sweep.sgmod` | Regenerated at integrity v2 |
| `.agents/skills/*/SKILL.md`, `AGENTS.md` | Documentation of the new contracts |

---

### Task 0: Branch

- [ ] **Step 1: Create the feature branch**

```bash
cd /home/kim/projects/sterngate
git checkout master
git checkout -b feat/map-studio-rebuild-p0-p1
cargo test --workspace 2>&1 | grep -E "^test result" | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
```

Expected: `passed=98 failed=0`.

---

### Task 1: Mock fault injection and a real ReadMemoryByAddress reply

**Files:**
- Modify: `crates/sterngate-hal/src/mock.rs`
- Test: `crates/sterngate-hal/src/lib.rs` (existing `mod tests`)

**Interfaces:**
- Produces: `VirtualCanInterface::with_failing_services(sids: &[u8]) -> Self` — every UDS request whose SID is in the set (single-frame or multi-frame) is answered `7F <sid> 31`.
- Produces: multi-frame `0x23` requests are answered `[1+n, 0x63, 0x00 × n]` for `1 ≤ n ≤ 6`, else `7F 23 31`. Later tasks keep `expected_original_data` ≤ 6 bytes because of this.

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `crates/sterngate-hal/src/lib.rs`:

```rust
    /// Drain background broadcast frames until a diagnostic reply arrives.
    async fn recv_diag(sim: &mut VirtualCanInterface) -> CanFrame {
        loop {
            let f = sim.recv().await.unwrap();
            if f.id == 0x7E8 {
                return f;
            }
        }
    }

    #[tokio::test]
    async fn test_virtual_can_read_memory_by_address_returns_requested_length() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();

        // UDS 0x23 with ALFID 0x24: 8-byte request -> First Frame + one Consecutive Frame.
        sim.send(CanFrame::new_standard(
            0x7E0,
            &[0x10, 0x08, 0x23, 0x24, 0x00, 0x1C, 0x10, 0x00],
        ))
        .await
        .unwrap();
        let fc = recv_diag(&mut sim).await;
        assert_eq!(fc.data[0], 0x30, "expected Flow Control");

        // The CF carries the 2-byte length: 3 bytes requested.
        sim.send(CanFrame::new_standard(0x7E0, &[0x21, 0x00, 0x03]))
            .await
            .unwrap();
        let resp = recv_diag(&mut sim).await;
        assert_eq!(&resp.data[..5], &[0x04, 0x63, 0x00, 0x00, 0x00]);
    }

    #[tokio::test]
    async fn test_virtual_can_read_memory_by_address_refuses_more_than_six_bytes() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();
        sim.send(CanFrame::new_standard(
            0x7E0,
            &[0x10, 0x08, 0x23, 0x24, 0x00, 0x1C, 0x10, 0x00],
        ))
        .await
        .unwrap();
        let _fc = recv_diag(&mut sim).await;
        sim.send(CanFrame::new_standard(0x7E0, &[0x21, 0x00, 0x07]))
            .await
            .unwrap();
        let resp = recv_diag(&mut sim).await;
        assert_eq!(&resp.data[..4], &[0x03, 0x7F, 0x23, 0x31]);
    }

    #[tokio::test]
    async fn test_virtual_can_failing_services_answer_nrc() {
        // Single-frame service in the failing set
        let mut sim = VirtualCanInterface::with_failing_services(&[0x22]);
        sim.open().await.unwrap();
        sim.send(CanFrame::new_standard(0x7E0, &[0x03, 0x22, 0xF1, 0x91]))
            .await
            .unwrap();
        let resp = recv_diag(&mut sim).await;
        assert_eq!(&resp.data[..4], &[0x03, 0x7F, 0x22, 0x31]);

        // Multi-frame service in the failing set
        let mut sim = VirtualCanInterface::with_failing_services(&[0x23]);
        sim.open().await.unwrap();
        sim.send(CanFrame::new_standard(
            0x7E0,
            &[0x10, 0x08, 0x23, 0x24, 0x00, 0x1C, 0x10, 0x00],
        ))
        .await
        .unwrap();
        let _fc = recv_diag(&mut sim).await;
        sim.send(CanFrame::new_standard(0x7E0, &[0x21, 0x00, 0x03]))
            .await
            .unwrap();
        let resp = recv_diag(&mut sim).await;
        assert_eq!(&resp.data[..4], &[0x03, 0x7F, 0x23, 0x31]);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p sterngate-hal test_virtual_can_ 2>&1 | tail -20`
Expected: compile error `no function or associated item named 'with_failing_services'`.

- [ ] **Step 3: Implement**

In `crates/sterngate-hal/src/mock.rs`:

1. Add `use std::collections::HashSet;` to the imports.
2. Add the field to the struct (after `received_cfs`):

```rust
    /// UDS service IDs that answer NRC 0x31 instead of their normal reply.
    /// Test-only fault injection so fail-closed paths can be proven on CI.
    failing_services: HashSet<u8>,
```

3. In `new()`, add `failing_services: HashSet::new(),` to the struct literal, and add the constructor right after `new()`:

```rust
    /// A virtual ECU that answers every request for the listed UDS service
    /// IDs with `7F <sid> 31` (RequestOutOfRange).
    pub fn with_failing_services(sids: &[u8]) -> Self {
        let mut sim = Self::new();
        sim.failing_services = sids.iter().copied().collect();
        sim
    }
```

4. Replace the Consecutive-Frame completion block (currently lines 80–87, from `// ISO-TP Consecutive Frame: Acknowledge completion` through the `return Some(...)`) with:

```rust
            // ISO-TP Consecutive Frame: acknowledge completion once all frames received
            let sid = self.last_multi_frame_sid.load(Ordering::Relaxed);
            if self.failing_services.contains(&sid) {
                return Some(CanFrame::new_standard(
                    resp_id as u16,
                    &[0x03, 0x7F, sid, 0x31],
                ));
            }
            let resp_bytes = match sid {
                0x3D => vec![0x02, 0x7D, 0x24, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA],
                0x23 => {
                    // An 8-byte 0x23 request puts [SID, ALFID, addr x4] in the
                    // First Frame; the single CF carries [len_hi, len_lo].
                    let n = if payload.len() >= 3 {
                        usize::from(u16::from_be_bytes([payload[1], payload[2]]))
                    } else {
                        0
                    };
                    if n == 0 || n > 6 {
                        // Only single-frame replies are emitted by this mock.
                        vec![0x03, 0x7F, 0x23, 0x31]
                    } else {
                        let mut r = vec![u8::try_from(1 + n).unwrap_or(0x07), 0x63];
                        r.resize(2 + n, 0x00);
                        r
                    }
                }
                _ => vec![0x03, 0x6E, 0x20, 0x31, 0xAA, 0xAA, 0xAA, 0xAA],
            };
            return Some(CanFrame::new_standard(resp_id as u16, &resp_bytes));
```

5. After the `let service = if pci_type == 0 { ... } else { payload[0] };` block, add:

```rust
        if self.failing_services.contains(&service) {
            return Some(CanFrame::new_standard(
                resp_id as u16,
                &[0x03, 0x7F, service, 0x31],
            ));
        }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p sterngate-hal 2>&1 | tail -8`
Expected: all hal tests pass, including the three new ones.

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result" | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
git add crates/sterngate-hal/src/mock.rs crates/sterngate-hal/src/lib.rs
git commit -m "test(hal): give the virtual ECU a real ReadMemoryByAddress reply and per-service fault injection

ModRunner's byte preconditions could never execute on CI because every
multi-frame 0x23 was answered with a WriteDID response.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Bounds-safe, unique-hit `find_dtc_offset`

**Files:**
- Modify: `crates/sterngate-core/src/calibrator/detector.rs:479-506`
- Test: same file, new `#[cfg(test)] mod tests`

**Interfaces:**
- Produces: `BoschMapDetector::find_dtc_offset(rom: &[u8], p_code: &str) -> Option<(u32, u8)>` (unchanged signature) that never panics, returns `None` for ROMs shorter than the search region, for a mask byte past EOF, and for a pattern that occurs more than once.

- [ ] **Step 1: Write the failing tests**

Append to the end of `crates/sterngate-core/src/calibrator/detector.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_dtc_offset_never_panics_on_short_or_empty_rom() {
        assert!(BoschMapDetector::find_dtc_offset(&[], "P0401").is_none());
        assert!(BoschMapDetector::find_dtc_offset(&vec![0xFF; 0x7FFFF], "P0401").is_none());
        assert!(BoschMapDetector::find_dtc_offset(&vec![0xFF; 0x1F_FFFF], "P0401").is_none());
    }

    #[test]
    fn find_dtc_offset_returns_none_when_mask_byte_past_eof() {
        let mut rom = vec![0xFF; 0x80002];
        rom[0x80000] = 0x04;
        rom[0x80001] = 0x01;
        assert!(BoschMapDetector::find_dtc_offset(&rom, "P0401").is_none());
    }

    #[test]
    fn find_dtc_offset_returns_unique_hit_with_mask() {
        let mut rom = vec![0xFF; 0x10_0000];
        rom[0x90000..0x90003].copy_from_slice(&[0x04, 0x01, 0x03]);
        assert_eq!(
            BoschMapDetector::find_dtc_offset(&rom, "P0401"),
            Some((0x90000, 0x03))
        );
    }

    #[test]
    fn find_dtc_offset_is_none_when_pattern_is_ambiguous() {
        let mut rom = vec![0xFF; 0x10_0000];
        rom[0x90000..0x90003].copy_from_slice(&[0x04, 0x01, 0x03]);
        rom[0xA0000..0xA0003].copy_from_slice(&[0x04, 0x01, 0x03]);
        assert!(BoschMapDetector::find_dtc_offset(&rom, "P0401").is_none());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p sterngate-core find_dtc_offset 2>&1 | tail -15`
Expected: `find_dtc_offset_never_panics_on_short_or_empty_rom` panics with `range start index 524288 out of range`; `..._past_eof` fails (returns `Some((0x80000, 0x01))`); `..._ambiguous` fails (returns the first hit).

- [ ] **Step 3: Replace the function**

Replace the whole `find_dtc_offset` function (lines 478–506) with:

```rust
    /// Locate a raw 2-byte P-code pattern in the calibration region.
    ///
    /// This is an inspection heuristic only: it knows nothing about the Bosch
    /// DTC table structure, so a hit is not evidence of a fault-path entry.
    /// Returns `None` when the ROM is shorter than the search region, when the
    /// mask byte would lie past the end of the ROM, or when the pattern occurs
    /// more than once (ambiguous). Never panics.
    pub fn find_dtc_offset(rom: &[u8], p_code: &str) -> Option<(u32, u8)> {
        let code_num = p_code.trim().trim_start_matches(['P', 'p']);
        let hex_val = u16::from_str_radix(code_num, 16).ok()?;
        let be_bytes = hex_val.to_be_bytes();
        let le_bytes = hex_val.to_le_bytes();

        let cal_start = if rom.len() >= 0x20_0000 {
            0x18_0000
        } else {
            0x08_0000
        };
        let region = rom.get(cal_start..)?;

        let mut hit: Option<usize> = None;
        for (pos, window) in region.windows(2).enumerate() {
            if window == be_bytes || window == le_bytes {
                if hit.is_some() {
                    return None; // ambiguous: the pattern is not unique
                }
                hit = Some(cal_start + pos);
            }
        }
        let abs = hit?;
        let mask = *rom.get(abs + 2)?;
        Some((u32::try_from(abs).ok()?, mask))
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p sterngate-core 2>&1 | tail -6`
Expected: all core tests pass (the four new ones included).

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result" | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
git add crates/sterngate-core/src/calibrator/detector.rs
git commit -m "fix(calibrator): make find_dtc_offset bounds-safe and refuse ambiguous hits

A ROM shorter than 512 KiB reached rom[0x80000..] and panicked on a
network-reachable path; a past-EOF mask byte was invented as 0x01.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: Armor raw-JSON branch honours the integrity report

**Files:**
- Modify: `crates/sterngate-core/src/modpack/armor.rs:59-65`
- Test: same file, existing `mod tests`

- [ ] **Step 1: Write the failing test**

Append inside `mod tests` in `armor.rs`:

```rust
    #[test]
    fn decode_from_armor_rejects_corrupt_plain_json() {
        let mut m = sample_mod();
        m.integrity.payload_crc32 ^= 1;
        let json = m.to_json().unwrap();
        assert!(decode_from_armor(&json).is_err());
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sterngate-core decode_from_armor_rejects 2>&1 | tail -8`
Expected: FAIL, `assertion failed: decode_from_armor(&json).is_err()`.

- [ ] **Step 3: Implement**

Replace the direct-JSON branch body:

```rust
    // 1. Direct JSON check
    if clean_input.starts_with('{') && clean_input.ends_with('}') {
        let mut modpack: SterngateMod = serde_json::from_str(clean_input).map_err(|e| {
            SterngateError::ProfileError(format!("Invalid JSON mod package: {}", e))
        })?;
        let report = modpack.verify_and_repair()?;
        if !report.is_valid {
            return Err(SterngateError::ProfileError(format!(
                "Mod package integrity check failed: {:?}",
                report.warning_messages
            )));
        }
        return Ok(modpack);
    }
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p sterngate-core armor 2>&1 | tail -6` → all armor tests pass.

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result" | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
git add crates/sterngate-core/src/modpack/armor.rs
git commit -m "fix(modpack): reject plain-JSON packages whose integrity check fails

The armored branch already refused; the raw-JSON branch discarded the report.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: One `MapProvenance` from detector to package

**Files:**
- Modify: `crates/sterngate-core/src/calibrator/map.rs`, `calibrator/mod.rs`, `calibrator/detector.rs`, `calibrator/stage.rs`, `modpack/mod.rs`, `crates/sterngate-core/src/lib.rs:17-20` and `:736`, `crates/sterngate-protocol/src/lib.rs:552-566`, `crates/sterngate-cli/src/commands/modcmd.rs:85-91,107-113`
- Test: `map.rs`, `detector.rs`, `modpack/mod.rs`

**Interfaces:**
- Produces: `sterngate_core::MapProvenance { Unverified (default), Synthetic, Fallback, Scanned }` with `is_unverified(&self) -> bool` and `is_scanned(self) -> bool`.
- Produces: `EcuMap.provenance: MapProvenance` (required, serialized as `"provenance": "scanned"` etc.) and `EcuMap::is_rom_backed(&self, rom: &[u8]) -> bool`.
- Produces: `ModAction::PatchFlashMap { .., provenance: MapProvenance }` and `ModAction::DtcMask { .., provenance: MapProvenance }`, serde `default` + `skip_serializing_if = "MapProvenance::is_unverified"` so legacy JSON bytes are unchanged.

- [ ] **Step 1: Write the failing tests**

Append to `crates/sterngate-core/src/calibrator/map.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn map_at(address: u32, raw_bytes: Vec<u8>) -> EcuMap {
        EcuMap {
            name: "t".into(),
            category: MapCategory::Boost,
            provenance: MapProvenance::Scanned,
            address,
            rows: 1,
            cols: 1,
            axis_x: None,
            axis_y: None,
            data: vec![],
            raw_bytes,
            factor: 1.0,
            offset: 0.0,
            unit: String::new(),
            is_16bit: true,
            is_signed: false,
        }
    }

    #[test]
    fn is_rom_backed_rejects_out_of_range_and_mismatch() {
        let mut rom = vec![0xFF; 64];
        rom[10..12].copy_from_slice(&[0x12, 0x34]);
        assert!(map_at(10, vec![0x12, 0x34]).is_rom_backed(&rom));
        assert!(!map_at(10, vec![0x12, 0x35]).is_rom_backed(&rom));
        assert!(!map_at(63, vec![0x12, 0x34]).is_rom_backed(&rom));
        assert!(!map_at(u32::MAX, vec![0x12]).is_rom_backed(&rom));
        assert!(!map_at(10, vec![]).is_rom_backed(&rom));
    }

    #[test]
    fn provenance_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&MapProvenance::Scanned).unwrap(),
            "\"scanned\""
        );
        assert_eq!(MapProvenance::default(), MapProvenance::Unverified);
        assert!(MapProvenance::Unverified.is_unverified());
        assert!(MapProvenance::Scanned.is_scanned());
        assert!(!MapProvenance::Fallback.is_scanned());
    }
}
```

Append inside `mod tests` in `detector.rs` (created in Task 2):

```rust
    fn test_rom_with_svbl() -> Vec<u8> {
        let mut rom = vec![0xFF; 0x20_0000];
        let svbl = 2350u16.to_be_bytes();
        rom[0x1C2000] = svbl[0];
        rom[0x1C2001] = svbl[1];
        rom[0x1C1FFE] = 0x00;
        rom[0x1C1FFF] = 0x00;
        rom[0x1C2002] = 0x00;
        rom[0x1C2003] = 0x00;
        rom
    }

    #[test]
    fn scan_rom_labels_provenance_honestly() {
        let rom = test_rom_with_svbl();
        let maps = BoschMapDetector::scan_rom(&rom);
        let svbl = maps.iter().find(|m| m.name.contains("SVBL")).unwrap();
        assert_eq!(svbl.provenance, MapProvenance::Scanned);
        assert!(svbl.is_rom_backed(&rom));

        let torque = maps.iter().find(|m| m.name == "Torque Limiter").unwrap();
        assert_eq!(torque.provenance, MapProvenance::Fallback);

        for m in maps.iter().filter(|m| !m.name.contains("SVBL") && m.name != "Torque Limiter") {
            assert_eq!(m.provenance, MapProvenance::Synthetic, "{}", m.name);
            assert!(!m.is_rom_backed(&rom), "{}", m.name);
        }
        assert_eq!(maps.iter().filter(|m| m.provenance.is_scanned()).count(), 1);
    }

    #[test]
    fn small_rom_yields_no_scanned_maps() {
        let rom = vec![0xFF; 0x80000];
        let maps = BoschMapDetector::scan_rom(&rom);
        assert!(!maps.is_empty());
        assert!(maps.iter().all(|m| !m.provenance.is_scanned()));
        assert!(maps.iter().all(|m| !m.is_rom_backed(&rom)));
    }
```

Add a new `#[cfg(test)] mod tests` at the end of `crates/sterngate-core/src/modpack/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::calibrator::map::MapProvenance;

    pub(crate) fn sample_metadata() -> ModMetadata {
        ModMetadata {
            mod_id: "w211-test".into(),
            name: "W211 Test".into(),
            version: "1.0.0".into(),
            author: "TunerKim".into(),
            description: "test package".into(),
            category: ModCategory::Performance,
            risk_level: ModRiskLevel::Moderate,
            instructions: None,
            created_at: "2026-09-15T12:00:00Z".into(),
        }
    }

    pub(crate) fn sample_target(min_battery_voltage: f64) -> ModTargetFilter {
        ModTargetFilter {
            chassis: vec!["W211".into()],
            ecu_name: "EDC16".into(),
            tx_id: 0x7E0,
            rx_id: 0x7E8,
            compatible_hw_ids: vec![],
            compatible_sw_ids: vec![],
            min_battery_voltage,
            requires_engine_off: true,
        }
    }

    pub(crate) fn flash_patch(provenance: MapProvenance) -> ModAction {
        ModAction::PatchFlashMap {
            map_name: "Torque Limiter".into(),
            address_offset: 0x1C1000,
            data: vec![0x0B, 0xB8, 0x10],
            expected_original_data: Some(vec![0x00, 0x00, 0x00]),
            description: "test patch".into(),
            provenance,
        }
    }

    #[test]
    fn provenance_defaults_to_unverified_and_keeps_legacy_integrity() {
        // A package whose actions carry the default provenance serializes
        // exactly like a package written before the field existed.
        let m = SterngateMod::create(
            sample_metadata(),
            sample_target(12.5),
            vec![flash_patch(MapProvenance::Unverified)],
            vec![],
        )
        .unwrap();
        let json = m.to_json().unwrap();
        assert!(!json.contains("provenance"));

        let mut parsed = SterngateMod::from_json(&json).unwrap();
        match &parsed.actions[0] {
            ModAction::PatchFlashMap { provenance, .. } => {
                assert_eq!(*provenance, MapProvenance::Unverified);
            }
            other => panic!("unexpected action {other:?}"),
        }
        assert!(parsed.verify_and_repair().unwrap().is_valid);

        // A labelled package round-trips its label.
        let m2 = SterngateMod::create(
            sample_metadata(),
            sample_target(12.5),
            vec![flash_patch(MapProvenance::Synthetic)],
            vec![],
        )
        .unwrap();
        let json2 = m2.to_json().unwrap();
        assert!(json2.contains("\"provenance\": \"synthetic\""));
        assert!(SterngateMod::from_json(&json2).unwrap().verify_and_repair().unwrap().is_valid);
    }
}
```

- [ ] **Step 2: Run to verify compile failure**

Run: `cargo test -p sterngate-core 2>&1 | grep -E "^error" | head -5`
Expected: `cannot find type 'MapProvenance'` / `struct 'EcuMap' has no field named 'provenance'`.

- [ ] **Step 3: Implement the types**

In `crates/sterngate-core/src/calibrator/map.rs`, after the `MapCategory` impl:

```rust
/// Where a detected map's `address` and `raw_bytes` came from.
///
/// Only `Scanned` maps may ever be turned into flash writes. Every other
/// value marks data with no proven relation to the ROM it claims to describe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MapProvenance {
    /// Field absent or origin unknown (legacy packages). Never executable.
    #[default]
    Unverified,
    /// Hard-coded placeholder; address and bytes were never read from the ROM.
    Synthetic,
    /// A scan ran, found nothing, and a hard-coded default was substituted.
    Fallback,
    /// `raw_bytes` were read from `rom[address..]` by a pattern scan.
    Scanned,
}

impl MapProvenance {
    /// True for the serde default, used by `skip_serializing_if`.
    pub const fn is_unverified(&self) -> bool {
        matches!(self, Self::Unverified)
    }

    /// True only for maps located in the target's own ROM.
    pub const fn is_scanned(self) -> bool {
        matches!(self, Self::Scanned)
    }
}
```

Add `pub provenance: MapProvenance,` to `EcuMap` directly after `pub category: MapCategory,`. Add to `impl EcuMap`:

```rust
    /// Independent evidence check: true only when `raw_bytes` is non-empty and
    /// equals the ROM slice at `address`. The provenance label is not trusted.
    pub fn is_rom_backed(&self, rom: &[u8]) -> bool {
        if self.raw_bytes.is_empty() {
            return false;
        }
        let Ok(start) = usize::try_from(self.address) else {
            return false;
        };
        let Some(end) = start.checked_add(self.raw_bytes.len()) else {
            return false;
        };
        rom.get(start..end)
            .is_some_and(|slice| slice == self.raw_bytes.as_slice())
    }
```

In `crates/sterngate-core/src/calibrator/mod.rs`: `pub use map::{EcuMap, MapAxis, MapCategory, MapProvenance};`

In `crates/sterngate-core/src/lib.rs` lines 17–20, add `MapProvenance` to the `calibrator::{...}` re-export list.

In `crates/sterngate-core/src/modpack/mod.rs`, add `use crate::calibrator::map::MapProvenance;` and extend the two variants:

```rust
    /// Patch an ECU flash calibration map (e.g. Torque Limiter, Boost Target, SVBL)
    PatchFlashMap {
        map_name: String,
        address_offset: u32,
        data: Vec<u8>,
        /// Optional expected original bytes at offset (precondition check)
        expected_original_data: Option<Vec<u8>>,
        description: String,
        /// Origin of `address_offset`/`data`. Absent in legacy packages, which
        /// deserialize to `Unverified` and are refused by the runner.
        #[serde(default, skip_serializing_if = "MapProvenance::is_unverified")]
        provenance: MapProvenance,
    },
    /// Suppress or disable a specific Diagnostic Trouble Code in ECU flash memory
    DtcMask {
        p_code: String,
        address_offset: u32,
        original_mask: u8,
        disable_mask: u8,
        description: String,
        /// Origin of `address_offset`. Same rules as `PatchFlashMap`.
        #[serde(default, skip_serializing_if = "MapProvenance::is_unverified")]
        provenance: MapProvenance,
    },
```

- [ ] **Step 4: Label the detector**

In `detector.rs` add `use super::map::MapProvenance;` (extend the existing `use super::map::{...}`) and add a `provenance` field to every `EcuMap` literal:

| Literal (by current line) | Value |
|---|---|
| SVBL scan hit (53–67) | `MapProvenance::Scanned` |
| SVBL fallback (78–92) | `MapProvenance::Fallback` |
| Torque limiter scan hit (174–193) | `MapProvenance::Scanned` |
| Torque limiter default tail (214–233) | `MapProvenance::Fallback` |
| Driver's Wish (260–284), Boost Target (318–342), Smoke Limiter (370–394), Rail Pressure (428–452), EGR Hysteresis (461–475) | `MapProvenance::Synthetic` |

Place the field directly after `category:` in each literal.

- [ ] **Step 5: Thread provenance through the generator and fix the other construction sites**

In `stage.rs`: add `use super::map::MapProvenance;`. Every `ModAction::PatchFlashMap { ... }` literal (both `actions.push` and `rollback_actions.push`, in `generate_stage1` and `generate_stage2`) gets `provenance: map.provenance,` as its last field. Every `ModAction::DtcMask { ... }` literal (in `generate_stage2` and `generate_dtc_kill`, actions and rollbacks) gets `provenance: MapProvenance::Synthetic,` (Task 7 removes these literals).

In `crates/sterngate-core/src/lib.rs` test `test_map_percentage_modification_and_clamping` (line 736), add `provenance: MapProvenance::Scanned,` after `category`.

In `crates/sterngate-protocol/src/lib.rs` test `test_mod_runner_flash_map_and_dtc_mask` (lines 552–566): add `use sterngate_core::MapProvenance;` to that test's `use` list and `provenance: MapProvenance::Synthetic,` to both action literals (Task 5 rewrites this test).

In `crates/sterngate-cli/src/commands/modcmd.rs`, the two destructuring patterns at lines 85–91 and 107–113 must add `..` as the last pattern element:

```rust
                    ModAction::PatchFlashMap {
                        map_name,
                        address_offset,
                        data,
                        expected_original_data,
                        description,
                        ..
                    } => {
```

and

```rust
                    ModAction::DtcMask {
                        p_code,
                        address_offset,
                        original_mask,
                        disable_mask,
                        description,
                        ..
                    } => {
```

- [ ] **Step 6: Run the tests**

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | head -20`
Expected: everything passes (no behaviour changed yet; only labels and a JSON field were added). The `test_bosch_checksum_solver_and_stage_generator` test still passes because the runner and generator do not check provenance yet.

- [ ] **Step 7: Gate and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result" | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
git add -A crates/
git commit -m "feat(core): record map provenance from the detector into .sgmod flash actions

Every EcuMap now says whether its bytes were scanned from the ROM, fell
back to a default, or were fabricated; PatchFlashMap/DtcMask carry the
label inside the integrity-covered payload.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: `ModRunner` fail-closed gates and `TargetFingerprintPolicy`

**Files:**
- Modify: `crates/sterngate-protocol/src/modrunner.rs` (whole `impl ModRunner`), `crates/sterngate-protocol/src/lib.rs` (re-export + tests), `crates/sterngate-core/src/modpack/mod.rs` (constant + re-export in `crates/sterngate-core/src/lib.rs`), callers: `crates/sterngate-server/src/routes/community_mods.rs:85-94,142`, `crates/sterngate-mcp/src/tools/community_mods.rs:80-88`, `crates/sterngate-cli/src/commands/modcmd.rs:176-208`
- Test: `crates/sterngate-protocol/src/lib.rs`

**Interfaces:**
- Produces: `sterngate_core::FLASH_WRITE_MIN_VOLTAGE: f64 = 12.5` (in `modpack/mod.rs`, re-exported from the crate root).
- Produces: `sterngate_protocol::TargetFingerprintPolicy { Enforce, BypassUnsafe }`.
- Produces: `ModRunner::apply_mod(interface, modpack, vin, battery_voltage: f64, policy: TargetFingerprintPolicy) -> Result<ModExecutionReport>` with these gates, in order: integrity → provenance (all `PatchFlashMap`/`DtcMask` in actions and rollback must be `Scanned`) → `BypassUnsafe` refused if the package writes flash → voltage `>= max(target.min, 12.5 if writes_flash)` regardless of policy → overlapping flash ranges refused → chassis (policy) → F191 whitelist, read failure refuses when a whitelist exists (policy) → per-action byte preconditions (never bypassable) → writes; bitmask `WriteDid` refuses when the current value cannot be read.
- Consumes: `MapProvenance` (Task 4), `VirtualCanInterface::with_failing_services` (Task 1).

- [ ] **Step 1: Write the failing tests**

In `crates/sterngate-protocol/src/lib.rs` `mod tests`, add these helpers and tests. Also change the two existing `apply_mod(..., false)` calls in `test_mod_runner_execution_and_safety_checks` to `apply_mod(..., TargetFingerprintPolicy::Enforce)`, and **replace** `test_mod_runner_flash_map_and_dtc_mask` with the positive-path version below.

```rust
    use sterngate_core::{
        MapProvenance, ModAction, ModCategory, ModMetadata, ModRiskLevel, ModTargetFilter,
        SterngateError, SterngateMod,
    };

    fn runner_metadata() -> ModMetadata {
        ModMetadata {
            mod_id: "w211-runner-test".into(),
            name: "W211 Runner Test".into(),
            version: "1.0.0".into(),
            author: "TunerKim".into(),
            description: "runner gate tests".into(),
            category: ModCategory::Performance,
            risk_level: ModRiskLevel::Moderate,
            instructions: None,
            created_at: "2026-09-15T12:00:00Z".into(),
        }
    }

    fn runner_target(compatible_hw_ids: Vec<String>, min_v: f64) -> ModTargetFilter {
        ModTargetFilter {
            chassis: vec!["W211".into()],
            ecu_name: "EDC16".into(),
            tx_id: 0x7E0,
            rx_id: 0x7E8,
            compatible_hw_ids,
            compatible_sw_ids: vec![],
            min_battery_voltage: min_v,
            requires_engine_off: true,
        }
    }

    fn runner_mod(actions: Vec<ModAction>, hw_ids: Vec<String>, min_v: f64) -> SterngateMod {
        SterngateMod::create(runner_metadata(), runner_target(hw_ids, min_v), actions, vec![])
            .unwrap()
    }

    fn patch(address: u32, provenance: MapProvenance, expected: Option<Vec<u8>>) -> ModAction {
        ModAction::PatchFlashMap {
            map_name: "Torque Limiter".into(),
            address_offset: address,
            data: vec![0x0B, 0xB8, 0x10],
            expected_original_data: expected,
            description: "+18% Torque Limiter".into(),
            provenance,
        }
    }

    fn dtc_mask(original_mask: u8, provenance: MapProvenance) -> ModAction {
        ModAction::DtcMask {
            p_code: "P0401".into(),
            address_offset: 0x1CE000,
            original_mask,
            disable_mask: 0x00,
            description: "DTC Off: P0401".into(),
            provenance,
        }
    }

    /// DID 0x0201 is one the virtual ECU serves (`62 02 01 01`); a bitmask
    /// write must be able to read the current value first.
    fn write_did(bitmask: Option<Vec<u8>>) -> ModAction {
        ModAction::WriteDid {
            did: 0x0201,
            data: vec![0x00],
            bitmask,
            expected_original_data: None,
            description: "seatbelt chime".into(),
        }
    }

    const VIN_W211: &str = "WDB2112061A777777";

    fn err_text(r: Result<ModExecutionReport, SterngateError>) -> String {
        match r {
            Ok(_) => panic!("expected an error"),
            Err(e) => e.to_string(),
        }
    }

    #[tokio::test]
    async fn test_synthetic_provenance_refused_before_bus_traffic() {
        // Interface deliberately NOT opened: any bus exchange would surface as
        // a different error, so a provenance message proves the gate fires first.
        let mut iface = VirtualCanInterface::new();
        let mut m = runner_mod(vec![patch(0x1C1000, MapProvenance::Synthetic, Some(vec![0; 3]))], vec![], 12.5);
        let msg = err_text(ModRunner::apply_mod(&mut iface, &mut m, VIN_W211, 12.8, TargetFingerprintPolicy::Enforce).await);
        assert!(msg.contains("provenance"), "{msg}");

        let mut m = runner_mod(vec![patch(0x1C1000, MapProvenance::Unverified, Some(vec![0; 3]))], vec![], 12.5);
        let msg = err_text(ModRunner::apply_mod(&mut iface, &mut m, VIN_W211, 12.8, TargetFingerprintPolicy::Enforce).await);
        assert!(msg.contains("provenance"), "{msg}");
    }

    #[tokio::test]
    async fn test_patch_flash_map_read_failure_refuses() {
        let mut iface = VirtualCanInterface::with_failing_services(&[0x23]);
        iface.open().await.unwrap();
        let mut m = runner_mod(vec![patch(0x1C1000, MapProvenance::Scanned, Some(vec![0; 3]))], vec![], 12.5);
        let msg = err_text(ModRunner::apply_mod(&mut iface, &mut m, VIN_W211, 12.8, TargetFingerprintPolicy::Enforce).await);
        assert!(msg.contains("original bytes"), "{msg}");
    }

    #[tokio::test]
    async fn test_patch_flash_map_without_expected_bytes_refused() {
        let mut iface = VirtualCanInterface::new();
        iface.open().await.unwrap();
        let mut m = runner_mod(vec![patch(0x1C1000, MapProvenance::Scanned, None)], vec![], 12.5);
        let msg = err_text(ModRunner::apply_mod(&mut iface, &mut m, VIN_W211, 12.8, TargetFingerprintPolicy::Enforce).await);
        assert!(msg.contains("expected_original_data"), "{msg}");
    }

    #[tokio::test]
    async fn test_patch_flash_map_mismatch_refuses() {
        let mut iface = VirtualCanInterface::new();
        iface.open().await.unwrap();
        // The mock returns zeros; the package claims the ECU holds 0B B8 10.
        let mut m = runner_mod(vec![patch(0x1C1000, MapProvenance::Scanned, Some(vec![0x0B, 0xB8, 0x10]))], vec![], 12.5);
        let msg = err_text(ModRunner::apply_mod(&mut iface, &mut m, VIN_W211, 12.8, TargetFingerprintPolicy::Enforce).await);
        assert!(msg.contains("expected original bytes"), "{msg}");
    }

    #[tokio::test]
    async fn test_patch_flash_map_length_mismatch_refused() {
        let mut iface = VirtualCanInterface::new();
        iface.open().await.unwrap();
        let mut m = runner_mod(vec![patch(0x1C1000, MapProvenance::Scanned, Some(vec![0; 2]))], vec![], 12.5);
        let msg = err_text(ModRunner::apply_mod(&mut iface, &mut m, VIN_W211, 12.8, TargetFingerprintPolicy::Enforce).await);
        assert!(msg.contains("exactly"), "{msg}");
    }

    #[tokio::test]
    async fn test_dtc_mask_read_failure_and_mismatch_refuse() {
        let mut iface = VirtualCanInterface::with_failing_services(&[0x23]);
        iface.open().await.unwrap();
        let mut m = runner_mod(vec![dtc_mask(0x00, MapProvenance::Scanned)], vec![], 12.5);
        let msg = err_text(ModRunner::apply_mod(&mut iface, &mut m, VIN_W211, 12.8, TargetFingerprintPolicy::Enforce).await);
        assert!(msg.contains("current mask"), "{msg}");

        let mut iface = VirtualCanInterface::new();
        iface.open().await.unwrap();
        let mut m = runner_mod(vec![dtc_mask(0xFF, MapProvenance::Scanned)], vec![], 12.5);
        let msg = err_text(ModRunner::apply_mod(&mut iface, &mut m, VIN_W211, 12.8, TargetFingerprintPolicy::Enforce).await);
        assert!(msg.contains("expected 0xFF"), "{msg}");
    }

    #[tokio::test]
    async fn test_mod_runner_flash_map_and_dtc_mask() {
        // Positive path: scanned provenance, preconditions that match the mock.
        let mut iface = VirtualCanInterface::new();
        iface.open().await.unwrap();
        let mut m = runner_mod(
            vec![
                patch(0x1C1000, MapProvenance::Scanned, Some(vec![0; 3])),
                dtc_mask(0x00, MapProvenance::Scanned),
            ],
            vec![],
            12.5,
        );
        let exec = ModRunner::apply_mod(&mut iface, &mut m, VIN_W211, 12.8, TargetFingerprintPolicy::Enforce)
            .await
            .unwrap();
        assert!(exec.success);
        assert_eq!(exec.steps_completed, 2);
        assert_eq!(exec.total_steps, 2);
        assert!(exec.actions_executed[0].contains("Torque Limiter"));
        assert!(exec.actions_executed[1].contains("P0401"));
        assert!(exec.git_commit_sha.is_some());
    }

    #[tokio::test]
    async fn test_hw_whitelist_read_failure_refuses() {
        let mut iface = VirtualCanInterface::with_failing_services(&[0x22]);
        iface.open().await.unwrap();
        let mut m = runner_mod(vec![write_did(None)], vec!["0281012224".into()], 12.0);
        let msg = err_text(ModRunner::apply_mod(&mut iface, &mut m, VIN_W211, 12.8, TargetFingerprintPolicy::Enforce).await);
        assert!(msg.contains("hardware ID could not be read"), "{msg}");

        let report = ModRunner::inspect_compatibility(&mut iface, &m, Some(VIN_W211), Some(12.8))
            .await
            .unwrap();
        assert!(!report.matched_vehicle);
    }

    #[tokio::test]
    async fn test_bitmask_write_read_failure_refuses() {
        let mut iface = VirtualCanInterface::with_failing_services(&[0x22]);
        iface.open().await.unwrap();
        let mut m = runner_mod(vec![write_did(Some(vec![0x02]))], vec![], 12.0);
        let msg = err_text(ModRunner::apply_mod(&mut iface, &mut m, VIN_W211, 12.8, TargetFingerprintPolicy::Enforce).await);
        assert!(msg.contains("bitmask"), "{msg}");

        // Positive control on a healthy mock.
        let mut iface = VirtualCanInterface::new();
        iface.open().await.unwrap();
        let mut m = runner_mod(vec![write_did(Some(vec![0x02]))], vec![], 12.0);
        assert!(ModRunner::apply_mod(&mut iface, &mut m, VIN_W211, 12.8, TargetFingerprintPolicy::Enforce).await.is_ok());
    }

    #[tokio::test]
    async fn test_overlapping_flash_ranges_refused() {
        let mut iface = VirtualCanInterface::new();
        iface.open().await.unwrap();
        let mut m = runner_mod(
            vec![
                patch(0x1C1000, MapProvenance::Scanned, Some(vec![0; 3])),
                patch(0x1C1002, MapProvenance::Scanned, Some(vec![0; 3])),
            ],
            vec![],
            12.5,
        );
        let msg = err_text(ModRunner::apply_mod(&mut iface, &mut m, VIN_W211, 12.8, TargetFingerprintPolicy::Enforce).await);
        assert!(msg.contains("overlap"), "{msg}");
    }

    #[tokio::test]
    async fn test_bypass_policy_never_bypasses_voltage_or_flash_packages() {
        let mut iface = VirtualCanInterface::new();
        iface.open().await.unwrap();

        // Voltage floor is enforced under BypassUnsafe.
        let mut m = runner_mod(vec![write_did(None)], vec![], 12.0);
        let msg = err_text(ModRunner::apply_mod(&mut iface, &mut m, VIN_W211, 11.0, TargetFingerprintPolicy::BypassUnsafe).await);
        assert!(msg.contains("voltage"), "{msg}");

        // BypassUnsafe is refused outright for packages that write flash.
        let mut m = runner_mod(vec![patch(0x1C1000, MapProvenance::Scanned, Some(vec![0; 3]))], vec![], 12.5);
        let msg = err_text(ModRunner::apply_mod(&mut iface, &mut m, VIN_W211, 12.8, TargetFingerprintPolicy::BypassUnsafe).await);
        assert!(msg.contains("not permitted"), "{msg}");

        // BypassUnsafe still relaxes the chassis check for DID writes.
        let mut m = runner_mod(vec![write_did(None)], vec![], 12.0);
        assert!(ModRunner::apply_mod(&mut iface, &mut m, "WDB2040011A999999", 12.8, TargetFingerprintPolicy::BypassUnsafe).await.is_ok());
    }

    #[tokio::test]
    async fn test_flash_write_floor_overrides_author_declared_minimum() {
        let mut iface = VirtualCanInterface::new();
        iface.open().await.unwrap();
        // Author declares 12.0 V for a flash-writing package; the runner still requires 12.5 V.
        let mut m = runner_mod(vec![patch(0x1C1000, MapProvenance::Scanned, Some(vec![0; 3]))], vec![], 12.0);
        let msg = err_text(ModRunner::apply_mod(&mut iface, &mut m, VIN_W211, 12.2, TargetFingerprintPolicy::Enforce).await);
        assert!(msg.contains("12.5"), "{msg}");
    }
```

- [ ] **Step 2: Run to verify compile failure**

Run: `cargo test -p sterngate-protocol 2>&1 | grep -E "^error" | head -5`
Expected: `cannot find type 'TargetFingerprintPolicy'`.

- [ ] **Step 3: Add the constant in core**

In `crates/sterngate-core/src/modpack/mod.rs`, after the `use` lines:

```rust
/// Minimum measured battery voltage for any package that writes flash memory
/// (`PatchFlashMap`, `DtcMask`). Mirrors the flashing worker's erase interlock.
pub const FLASH_WRITE_MIN_VOLTAGE: f64 = 12.5;
```

In `crates/sterngate-core/src/lib.rs`, add `FLASH_WRITE_MIN_VOLTAGE` to the `pub use modpack::{...}` list.

- [ ] **Step 4: Rewrite `modrunner.rs`**

Replace the file contents from the `use` block through the end of `impl ModRunner` with:

```rust
use serde::{Deserialize, Serialize};
use tracing::info;

use sterngate_core::{
    ModAction, ModValidationReport, Result, SterngateError, SterngateMod, VehicleGarage,
    FLASH_WRITE_MIN_VOLTAGE,
};
use sterngate_hal::VehicleInterface;

use crate::uds::UdsClient;

/// Report returned after executing a community mod
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModExecutionReport {
    pub mod_id: String,
    pub mod_name: String,
    pub success: bool,
    pub steps_completed: usize,
    pub total_steps: usize,
    pub actions_executed: Vec<String>,
    pub git_commit_sha: Option<String>,
    pub live_hw_id: Option<String>,
    pub message: String,
}

/// How strictly the vehicle fingerprint (chassis, hardware whitelist) is enforced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetFingerprintPolicy {
    /// Refuse on any chassis or hardware-whitelist mismatch.
    Enforce,
    /// Relax the chassis and hardware-whitelist checks only. Never relaxes the
    /// voltage floor, map provenance or byte preconditions, and is refused
    /// outright for packages that write flash memory.
    BypassUnsafe,
}

pub struct ModRunner;

impl ModRunner {
    fn writes_flash(modpack: &SterngateMod) -> bool {
        modpack
            .actions
            .iter()
            .chain(modpack.rollback_actions.iter())
            .any(|a| {
                matches!(
                    a,
                    ModAction::PatchFlashMap { .. } | ModAction::DtcMask { .. }
                )
            })
    }

    /// Every flash action must carry `Scanned` provenance. Runs before any bus traffic.
    fn check_provenance(modpack: &SterngateMod) -> Result<()> {
        for action in modpack.actions.iter().chain(modpack.rollback_actions.iter()) {
            let (name, provenance) = match action {
                ModAction::PatchFlashMap {
                    map_name,
                    provenance,
                    ..
                } => (map_name.as_str(), *provenance),
                ModAction::DtcMask {
                    p_code, provenance, ..
                } => (p_code.as_str(), *provenance),
                _ => continue,
            };
            if !provenance.is_scanned() {
                return Err(SterngateError::PreFlightCheckFailed(format!(
                    "action '{name}' has map provenance {provenance:?}; only maps scanned from the target ECU's own ROM may be flashed"
                )));
            }
        }
        Ok(())
    }

    /// Flash ranges within one package must not overlap: a later action's
    /// precondition would otherwise be checked against bytes an earlier action changes.
    fn check_no_overlap(modpack: &SterngateMod) -> Result<()> {
        let mut ranges: Vec<(u32, u32, String)> = Vec::new();
        for action in &modpack.actions {
            let (start, len, name) = match action {
                ModAction::PatchFlashMap {
                    address_offset,
                    data,
                    map_name,
                    ..
                } => (*address_offset, data.len(), map_name.clone()),
                ModAction::DtcMask {
                    address_offset,
                    p_code,
                    ..
                } => (*address_offset, 1, p_code.clone()),
                _ => continue,
            };
            let len = u32::try_from(len).map_err(|_| {
                SterngateError::PreFlightCheckFailed(format!("action '{name}' is too large"))
            })?;
            let end = start.checked_add(len).ok_or_else(|| {
                SterngateError::PreFlightCheckFailed(format!(
                    "action '{name}' address range overflows"
                ))
            })?;
            for (s, e, other) in &ranges {
                if start < *e && *s < end {
                    return Err(SterngateError::PreFlightCheckFailed(format!(
                        "flash ranges of '{name}' and '{other}' overlap; refusing the package"
                    )));
                }
            }
            ranges.push((start, end, name));
        }
        Ok(())
    }

    async fn read_live_hw_id(uds: &mut UdsClient<'_>) -> Option<String> {
        match uds.read_data_by_identifier(0xF191).await {
            Ok(resp) if resp.len() >= 4 => {
                Some(String::from_utf8_lossy(&resp[3..]).trim().to_string())
            }
            _ => None,
        }
    }

    /// Inspect and test compatibility of a mod against live vehicle without executing changes
    pub async fn inspect_compatibility(
        interface: &mut dyn VehicleInterface,
        modpack: &SterngateMod,
        vin: Option<&str>,
        battery_voltage: Option<f64>,
    ) -> Result<ModValidationReport> {
        let report = modpack.clone().verify_and_repair()?;
        if !report.is_valid {
            return Ok(report);
        }

        let mut uds = UdsClient::new(interface, modpack.target.tx_id, modpack.target.rx_id);

        let live_hw_id = if modpack.target.compatible_hw_ids.is_empty() {
            None
        } else {
            let _ = uds.diagnostic_session_control(0x03).await;
            Self::read_live_hw_id(&mut uds).await
        };

        let mut final_report =
            modpack.check_compatibility(vin, live_hw_id.as_deref(), battery_voltage);

        if !modpack.target.compatible_hw_ids.is_empty() && live_hw_id.is_none() {
            final_report.matched_vehicle = false;
            final_report.warning_messages.push(
                "Mod declares a hardware whitelist but the ECU hardware ID (DID F191) could not be read"
                    .into(),
            );
        }

        Ok(final_report)
    }

    /// Safely apply a community mod package to the connected vehicle.
    ///
    /// Gate order: integrity, map provenance, policy admissibility, voltage
    /// floor, range overlap, chassis, hardware whitelist, per-action byte
    /// preconditions, then writes. Only the chassis and hardware-whitelist
    /// checks consult `policy`.
    pub async fn apply_mod(
        interface: &mut dyn VehicleInterface,
        modpack: &mut SterngateMod,
        vin: &str,
        battery_voltage: f64,
        policy: TargetFingerprintPolicy,
    ) -> Result<ModExecutionReport> {
        // 1. Verify and auto-repair Reed-Solomon FEC
        let validation = modpack.verify_and_repair()?;
        if !validation.is_valid {
            return Err(SterngateError::ProfileError(format!(
                "Mod package integrity check failed: {:?}",
                validation.warning_messages
            )));
        }

        // 1b. Map provenance gate (before any bus traffic)
        Self::check_provenance(modpack)?;

        // 1c. A fingerprint bypass is never admissible for flash writes
        let writes_flash = Self::writes_flash(modpack);
        if writes_flash && policy == TargetFingerprintPolicy::BypassUnsafe {
            return Err(SterngateError::PreFlightCheckFailed(
                "--force is not permitted for packages containing flash writes (PatchFlashMap/DtcMask)"
                    .into(),
            ));
        }

        // 2. Safety interlock: voltage (policy-independent)
        let required = if writes_flash {
            modpack.target.min_battery_voltage.max(FLASH_WRITE_MIN_VOLTAGE)
        } else {
            modpack.target.min_battery_voltage
        };
        if battery_voltage.is_nan() || battery_voltage < required {
            return Err(SterngateError::PreFlightCheckFailed(format!(
                "Battery voltage ({:.1}V) is below required minimum ({:.1}V) for mod '{}'",
                battery_voltage, required, modpack.metadata.name
            )));
        }

        // 2b. Flash ranges must not overlap
        Self::check_no_overlap(modpack)?;

        // 3. Vehicle chassis fingerprint check
        if policy == TargetFingerprintPolicy::Enforce && !modpack.target.matches_chassis(vin) {
            return Err(SterngateError::ProtocolError(format!(
                "Vehicle chassis mismatch: Mod '{}' requires chassis {:?}, but connected VIN is '{}'",
                modpack.metadata.name, modpack.target.chassis, vin
            )));
        }

        let mut uds = UdsClient::new(interface, modpack.target.tx_id, modpack.target.rx_id);

        // Enter Extended Diagnostic Session (0x10 03)
        let _ = uds.diagnostic_session_control(0x03).await;

        // 4. ECU hardware ID whitelist check (fail closed when a whitelist exists)
        let live_hw_id = Self::read_live_hw_id(&mut uds).await;
        if policy == TargetFingerprintPolicy::Enforce
            && !modpack.target.compatible_hw_ids.is_empty()
        {
            match live_hw_id.as_deref() {
                None => {
                    return Err(SterngateError::PreFlightCheckFailed(format!(
                        "mod '{}' declares a hardware whitelist {:?} but the ECU hardware ID could not be read; refusing",
                        modpack.metadata.name, modpack.target.compatible_hw_ids
                    )));
                }
                Some(hw) if !modpack.target.matches_hardware(hw) => {
                    return Err(SterngateError::ProtocolError(format!(
                        "Incompatible ECU Hardware ID '{}'. Mod '{}' only supports: {:?}",
                        hw, modpack.metadata.name, modpack.target.compatible_hw_ids
                    )));
                }
                Some(_) => {}
            }
        }

        // 5. Preconditions: prove the live bytes before writing anything (never bypassable)
        for action in &modpack.actions {
            match action {
                ModAction::WriteDid {
                    did,
                    expected_original_data: Some(expected),
                    ..
                } => {
                    let resp = uds.read_data_by_identifier(*did).await.map_err(|e| {
                        SterngateError::PreFlightCheckFailed(format!(
                            "DID 0x{did:04X}: could not read current value for precondition ({e}); refusing"
                        ))
                    })?;
                    let current = resp.get(3..).unwrap_or(&[]);
                    if current.len() < expected.len() || current[..expected.len()] != expected[..] {
                        return Err(SterngateError::PreFlightCheckFailed(format!(
                            "Precondition check failed for DID 0x{did:04X}: expected current bytes {expected:02X?}, but vehicle returned {current:02X?}. Mod execution halted to prevent configuration corruption."
                        )));
                    }
                }
                ModAction::PatchFlashMap {
                    map_name,
                    address_offset,
                    data,
                    expected_original_data,
                    ..
                } => {
                    let Some(expected) = expected_original_data
                        .as_deref()
                        .filter(|e| !e.is_empty())
                    else {
                        return Err(SterngateError::PreFlightCheckFailed(format!(
                            "map '{map_name}' @0x{address_offset:06X}: no expected_original_data; blind flash patches are refused"
                        )));
                    };
                    if expected.len() != data.len() {
                        return Err(SterngateError::PreFlightCheckFailed(format!(
                            "map '{map_name}' @0x{address_offset:06X}: a patch must replace exactly the bytes it verified (expected {} bytes, data {} bytes)",
                            expected.len(),
                            data.len()
                        )));
                    }
                    let len = u16::try_from(expected.len()).map_err(|_| {
                        SterngateError::PreFlightCheckFailed(format!(
                            "map '{map_name}': precondition longer than 65535 bytes"
                        ))
                    })?;
                    let current = uds
                        .read_memory_by_address(*address_offset, len)
                        .await
                        .map_err(|e| {
                            SterngateError::PreFlightCheckFailed(format!(
                                "map '{map_name}' @0x{address_offset:06X}: could not read original bytes ({e}); refusing to patch unverified memory"
                            ))
                        })?;
                    if current.len() != expected.len() || current[..] != expected[..] {
                        return Err(SterngateError::PreFlightCheckFailed(format!(
                            "Precondition check failed for map '{map_name}' at 0x{address_offset:06X}: expected original bytes {expected:02X?}, but vehicle returned {current:02X?}. Aborting flash patch."
                        )));
                    }
                }
                ModAction::DtcMask {
                    p_code,
                    address_offset,
                    original_mask,
                    ..
                } => {
                    let current = uds
                        .read_memory_by_address(*address_offset, 1)
                        .await
                        .map_err(|e| {
                            SterngateError::PreFlightCheckFailed(format!(
                                "DTC {p_code} mask @0x{address_offset:06X}: could not read current mask ({e}); refusing"
                            ))
                        })?;
                    match current.first() {
                        Some(b) if current.len() == 1 && *b == *original_mask => {}
                        Some(b) if current.len() == 1 => {
                            return Err(SterngateError::PreFlightCheckFailed(format!(
                                "DTC {p_code} mask @0x{address_offset:06X} is 0x{b:02X}, expected 0x{original_mask:02X}; refusing"
                            )));
                        }
                        _ => {
                            return Err(SterngateError::PreFlightCheckFailed(format!(
                                "DTC {p_code} mask @0x{address_offset:06X}: expected 1 byte, got {}",
                                current.len()
                            )));
                        }
                    }
                }
                ModAction::WriteDid { .. } | ModAction::Routine { .. } => {}
            }
        }

        // 6. Atomic pre-mod Git garage snapshot
        let garage = VehicleGarage::new(VehicleGarage::default_path());
        let pre_note = format!(
            "Pre-mod baseline snapshot before applying '{}' (ID: {})",
            modpack.metadata.name, modpack.metadata.mod_id
        );
        let _ = garage.save_coding(
            vin,
            &modpack.target.ecu_name,
            "PRE_MOD_SNAPSHOT",
            None,
            &pre_note,
        );

        // 7. Execute actions
        let total_steps = modpack.actions.len();
        let mut steps_completed = 0;
        let mut actions_executed = Vec::new();

        for action in &modpack.actions {
            match action {
                ModAction::WriteDid {
                    did,
                    data,
                    bitmask,
                    description,
                    ..
                } => {
                    let write_payload = if let Some(mask) = bitmask {
                        let resp = uds.read_data_by_identifier(*did).await.map_err(|e| {
                            SterngateError::PreFlightCheckFailed(format!(
                                "DID 0x{did:04X}: bitmask write requires the current value but the read failed ({e}); refusing to clobber unmasked bits"
                            ))
                        })?;
                        let current_bytes = resp.get(3..).unwrap_or(&[]);
                        if current_bytes.len() < mask.len() {
                            return Err(SterngateError::PreFlightCheckFailed(format!(
                                "DID 0x{did:04X}: bitmask covers {} bytes but the ECU returned {}; refusing",
                                mask.len(),
                                current_bytes.len()
                            )));
                        }
                        let mut merged = data.clone();
                        for (i, m) in mask.iter().enumerate() {
                            let cur = current_bytes.get(i).copied().unwrap_or(0);
                            let new_val = data.get(i).copied().unwrap_or(0);
                            let final_byte = (new_val & m) | (cur & !m);
                            if i < merged.len() {
                                merged[i] = final_byte;
                            } else {
                                merged.push(final_byte);
                            }
                        }
                        merged
                    } else {
                        data.clone()
                    };

                    info!(
                        "Applying Mod Write DID 0x{:04X} ({} bytes)...",
                        did,
                        write_payload.len()
                    );
                    uds.write_data_by_identifier(*did, &write_payload).await?;
                    steps_completed += 1;
                    actions_executed.push(format!("Write DID 0x{:04X}: {}", did, description));
                }
                ModAction::Routine {
                    routine_id,
                    subfunction,
                    data,
                    description,
                } => {
                    info!(
                        "Applying Mod Routine 0x{:04X} (subfunction: {})...",
                        routine_id, subfunction
                    );
                    uds.routine_control(*subfunction, *routine_id, data).await?;
                    steps_completed += 1;
                    actions_executed.push(format!("Routine 0x{:04X}: {}", routine_id, description));
                }
                ModAction::PatchFlashMap {
                    map_name,
                    address_offset,
                    data,
                    description,
                    ..
                } => {
                    info!(
                        "Applying Flash Map Patch '{}' at 0x{:06X} ({} bytes)...",
                        map_name,
                        address_offset,
                        data.len()
                    );
                    uds.write_memory_by_address(*address_offset, data).await?;
                    steps_completed += 1;
                    actions_executed.push(format!("Patch Map '{}': {}", map_name, description));
                }
                ModAction::DtcMask {
                    p_code,
                    address_offset,
                    disable_mask,
                    description,
                    ..
                } => {
                    info!(
                        "Applying DTC {} suppression mask (0x{:02X}) at 0x{:06X}...",
                        p_code, disable_mask, address_offset
                    );
                    uds.write_memory_by_address(*address_offset, &[*disable_mask])
                        .await?;
                    steps_completed += 1;
                    actions_executed.push(format!("DTC Mask {}: {}", p_code, description));
                }
            }
        }

        // 8. Atomic post-mod Git garage snapshot
        let post_note = format!(
            "Applied community mod '{}' v{} by {} ({})",
            modpack.metadata.name,
            modpack.metadata.version,
            modpack.metadata.author,
            modpack.metadata.mod_id
        );
        let _ = garage.save_coding(
            vin,
            &modpack.target.ecu_name,
            &format!("MOD_{}", modpack.metadata.mod_id.to_uppercase()),
            None,
            &post_note,
        );

        let git_commit_sha = garage
            .get_history(vin)
            .ok()
            .and_then(|h| h.first().map(|c| c.hash.clone()));

        Ok(ModExecutionReport {
            mod_id: modpack.metadata.mod_id.clone(),
            mod_name: modpack.metadata.name.clone(),
            success: true,
            steps_completed,
            total_steps,
            actions_executed,
            git_commit_sha,
            live_hw_id,
            message: format!(
                "✓ Mod '{}' applied successfully! {}/{} steps executed and recorded to vehicle Git history.",
                modpack.metadata.name, steps_completed, total_steps
            ),
        })
    }
}
```

Keep the module doc comment at the top of the file. In `crates/sterngate-protocol/src/lib.rs` change the re-export to `pub use modrunner::{ModExecutionReport, ModRunner, TargetFingerprintPolicy};`.

- [ ] **Step 5: Keep callers compiling**

Server `crates/sterngate-server/src/routes/community_mods.rs`: delete the `force: bool` field from `ModApplyPayload` (lines 92–93) and change the call to
`ModRunner::apply_mod(&mut **iface, &mut modpack, vin, voltage, TargetFingerprintPolicy::Enforce).await` with `use sterngate_protocol::{ModRunner, TargetFingerprintPolicy};`. (Unknown JSON fields are still ignored at this point; Task 6 makes them a 422.)

MCP `crates/sterngate-mcp/src/tools/community_mods.rs`: delete the `force` extraction (lines 81–84) and call `ModRunner::apply_mod(&mut mock_iface, &mut modpack, vin, battery_voltage, TargetFingerprintPolicy::Enforce)`; import `TargetFingerprintPolicy` from `sterngate_protocol`.

MCP test `test_mcp_community_mods_suite` (`crates/sterngate-mcp/src/lib.rs` ~line 655): it creates a bitmask mod on DID `0x01B0`, which the virtual ECU answers with NRC 0x31; it only passed because the runner ignored the failed read. Change the create arguments to `"did": "0x0201"`, `"data": "00"`, `"bitmask": "01"` (a DID the mock serves). The `mod_id` assertion is unaffected.

Existing protocol tests `test_mod_runner_execution_and_safety_checks` and `test_mod_runner_flash_map_and_dtc_mask` carry function-local `use sterngate_core::{ModAction, ...}` lines; delete them now that the module-level `use` above provides the same names.

CLI `crates/sterngate-cli/src/commands/modcmd.rs`: import `sterngate_protocol::{ModRunner, TargetFingerprintPolicy}`; in `ModCommands::Apply`, compute

```rust
            let policy = if force {
                TargetFingerprintPolicy::BypassUnsafe
            } else {
                TargetFingerprintPolicy::Enforce
            };
```

and pass `policy` instead of `force` to `apply_mod`.

- [ ] **Step 6: Run the tests**

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | head -30`
Expected: all protocol tests pass including the eleven new/rewritten ones; server, MCP and CLI compile; no other test changes behaviour (server/MCP tests apply `WriteDid` packages without whitelists).

- [ ] **Step 7: Gate and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result" | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
git add -A crates/
git commit -m "fix(protocol): make every ModRunner gate fail closed and narrow force to a fingerprint policy

Provenance is checked before any bus traffic, byte preconditions refuse
on read failure or mismatch, the hardware whitelist refuses when the ID
cannot be read, bitmask writes refuse without the current value, flash
ranges may not overlap, the 12.5 V floor applies to every flash-writing
package, and BypassUnsafe is refused for such packages.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: `force` leaves the network contract

**Files:**
- Modify: `crates/sterngate-server/src/routes/community_mods.rs:85-91`, `crates/sterngate-server/src/lib.rs:1159-1165` (test payload), `crates/sterngate-mcp/src/tools/community_mods.rs:67-90`, `crates/sterngate-mcp/src/tools/specs.rs:563-567`, `crates/sterngate-mcp/src/lib.rs:696-701` (test payload), `crates/sterngate-cli/src/args.rs:605-607`, `crates/sterngate-cli/src/commands/modcmd.rs:195-197`, `crates/sterngate-server/static/js/app.js:2080,2502`
- Test: `crates/sterngate-server/src/lib.rs`, `crates/sterngate-mcp/src/lib.rs`

**Interfaces:**
- Produces: `POST /api/v1/mods/apply` accepts only `content`, `vin`, `battery_voltage`; any other field → **422**.
- Produces: MCP `sterngate_apply_community_mod` returns `Err` containing `force` when the argument is present; the tool spec has no `force` property.

- [ ] **Step 1: Write the failing tests**

In `crates/sterngate-server/src/lib.rs` `mod tests`, add (reuse the state/setup lines from `test_community_mods_endpoints` at lines 1091–1099 for `state`):

```rust
    #[tokio::test]
    async fn test_mods_apply_rejects_force_field() {
        let mut iface = Box::new(VirtualCanInterface::new());
        let _ = iface.open().await;
        let profile =
            VehicleProfile::load_from_file("../../profiles/mercedes/w211_om646_edc16.json")
                .unwrap();
        let flasher = Arc::new(FlashingWorker::new());
        let state = Arc::new(AppState::new(iface, profile, flasher));

        let payload = json!({
            "content": "{}",
            "vin": "WDB2112061A000001",
            "battery_voltage": 12.8,
            "force": true
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/mods/apply")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&payload).unwrap()))
            .unwrap();
        let resp = create_router(state).oneshot(req).await.unwrap();
        // axum maps serde data errors (unknown field) to 422.
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
```

In `crates/sterngate-mcp/src/lib.rs` `mod tests`, add:

```rust
    #[tokio::test]
    async fn test_mcp_apply_mod_rejects_force_argument() {
        let res = tools::handle_tool_call(
            "sterngate_apply_community_mod",
            &json!({
                "mod_content": "{}",
                "vin": "WDB2112061A123456",
                "battery_voltage": 12.8,
                "force": true
            }),
        )
        .await;
        let err = res.unwrap_err();
        assert!(err.contains("force"), "{err}");
    }

    #[test]
    fn test_mcp_apply_mod_spec_has_no_force_property() {
        let tools = tools::get_tools_list();
        let apply = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "sterngate_apply_community_mod")
            .unwrap();
        assert!(apply["inputSchema"]["properties"].get("force").is_none());
    }
```

Also remove `"force": false` from the apply payloads in `test_community_mods_endpoints` (server lib.rs ~line 1163) and `test_mcp_community_mods_suite` (mcp lib.rs ~line 700).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sterngate-server test_mods_apply_rejects_force_field 2>&1 | tail -5 && cargo test -p sterngate-mcp force 2>&1 | tail -8`
Expected: server test fails with status 200/400 ≠ 422; MCP `rejects_force_argument` fails (returns a parse error not mentioning force, or Ok); `spec_has_no_force_property` fails.

- [ ] **Step 3: Implement**

Server: annotate the payload

```rust
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModApplyPayload {
    content: String,
    #[serde(default)]
    vin: Option<String>,
    #[serde(default)]
    battery_voltage: Option<f64>,
}
```

MCP tool (`community_mods.rs`, at the top of the `"sterngate_apply_community_mod"` arm, before `mod_content` is read):

```rust
            if arguments.get("force").is_some() {
                return Err(
                    "'force' is not accepted over MCP: fingerprint bypass is CLI-only (--force) and never applies to flash writes"
                        .into(),
                );
            }
```

MCP spec (`specs.rs` lines 563–567): delete the whole `"force": { ... }` property and change the tool description to:
`"Apply a verified community mod or tuning parameter package to the connected vehicle. Enforces map provenance, integrity, chassis, HW ID whitelist, the 12.5 V floor for flash writes and live byte preconditions; creates an atomic Git garage snapshot; applies DID writes with bitmask preservation. No bypass flag exists over MCP."`

CLI `args.rs` line 605–607 doc comment → `/// Relax the chassis and hardware-whitelist fingerprint checks only. Voltage, map provenance and byte preconditions stay enforced; refused for packages that write flash memory.`
CLI `modcmd.rs` line 196 banner → `println!("  ⚠️  FORCED BYPASS OF CHASSIS / HW-ID FINGERPRINT (voltage, map provenance and byte preconditions remain enforced)");`

UI `app.js`: delete the line `force: false` at both call sites (2080 and 2502) and the trailing comma on the preceding line where needed so the object literal stays valid.

- [ ] **Step 4: Run the tests**

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED" | head`
Expected: all pass. Run `node --check crates/sterngate-server/static/js/app.js` → no output.

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result" | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
git add -A crates/
git commit -m "fix(api): remove the force flag from the REST and MCP mod-apply contracts

A single JSON boolean could switch off every runner gate from any network
client; unknown fields are now a 422 and MCP rejects the key explicitly.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: Producer hardening — no package for maps that are not in the ROM

**Files:**
- Modify: `crates/sterngate-core/src/calibrator/stage.rs`, `crates/sterngate-core/src/modpack/mod.rs` (`create`), `crates/sterngate-core/src/lib.rs:802-826` (test), `crates/sterngate-server/src/routes/tuning.rs` (error mapping), `crates/sterngate-server/src/lib.rs:1237-1282` (test), `crates/sterngate-mcp/src/lib.rs:744-790` (test), `crates/sterngate-protocol/src/lib.rs` (floor test)
- Test: `stage.rs`, `modpack/mod.rs`

**Interfaces:**
- Produces: `StageGenerator::generate_stage1/2` return `Err(PreFlightCheckFailed)` naming the first map that is not `Scanned` and ROM-backed; `generate_dtc_kill` always returns `Err(PreFlightCheckFailed)`; Stage 2 emits no `DtcMask`.
- Produces: `SterngateMod::create` returns `Err(PreFlightCheckFailed)` when any action or rollback writes flash and `target.min_battery_voltage` is NaN or `< FLASH_WRITE_MIN_VOLTAGE`.
- Produces: tuning routes answer **422** for `PreFlightCheckFailed`, 500 otherwise.

- [ ] **Step 1: Write the failing tests**

Append a `#[cfg(test)] mod tests` to `stage.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::calibrator::map::{EcuMap, MapCategory, MapProvenance};

    fn test_rom_with_svbl() -> Vec<u8> {
        let mut rom = vec![0xFF; 0x20_0000];
        let svbl = 2350u16.to_be_bytes();
        rom[0x1C2000] = svbl[0];
        rom[0x1C2001] = svbl[1];
        for i in [0x1C1FFE, 0x1C1FFF, 0x1C2002, 0x1C2003] {
            rom[i] = 0x00;
        }
        rom
    }

    #[test]
    fn stage1_refuses_when_any_targeted_map_is_not_scanned() {
        let rom = test_rom_with_svbl();
        let err = StageGenerator::generate_stage1(&rom, "W211", "EDC16CP31", "t").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Torque Limiter"), "{msg}");
        assert!(msg.contains("Fallback"), "{msg}");
    }

    #[test]
    fn stage2_refuses_synthetic_maps_and_emits_no_dtc_mask() {
        let rom = test_rom_with_svbl();
        let err = StageGenerator::generate_stage2(&rom, "W211", "EDC16CP31", "t").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("provenance"), "{msg}");
    }

    #[test]
    fn dtc_kill_refuses_until_detector_rebuild() {
        let rom = test_rom_with_svbl();
        let err = StageGenerator::generate_dtc_kill(&rom, "W211", "EDC16", &["P0401".into()], "t")
            .unwrap_err();
        assert!(err.to_string().contains("unsupported"), "{err}");
    }

    #[test]
    fn require_rom_backed_rejects_mislabelled_scanned_map() {
        let rom = test_rom_with_svbl();
        let map = EcuMap {
            name: "Forged".into(),
            category: MapCategory::Boost,
            provenance: MapProvenance::Scanned,
            address: 0x1C2000,
            rows: 1,
            cols: 1,
            axis_x: None,
            axis_y: None,
            data: vec![0.0],
            raw_bytes: vec![0x12, 0x34],
            factor: 1.0,
            offset: 0.0,
            unit: "mbar".into(),
            is_16bit: true,
            is_signed: false,
        };
        assert!(StageGenerator::require_rom_backed(&map, &rom).is_err());
    }

    #[test]
    fn require_rom_backed_accepts_true_scan_hit() {
        let rom = test_rom_with_svbl();
        let maps = BoschMapDetector::scan_rom(&rom);
        let svbl = maps.iter().find(|m| m.name.contains("SVBL")).unwrap();
        assert!(StageGenerator::require_rom_backed(svbl, &rom).is_ok());
    }
}
```

Append inside `mod tests` in `modpack/mod.rs` (Task 4 created it):

```rust
    #[test]
    fn create_rejects_flash_writes_below_12v5() {
        let err = SterngateMod::create(
            sample_metadata(),
            sample_target(12.0),
            vec![flash_patch(MapProvenance::Scanned)],
            vec![],
        )
        .unwrap_err();
        assert!(err.to_string().contains("12.5"), "{err}");

        // Rollback actions count too.
        assert!(SterngateMod::create(
            sample_metadata(),
            sample_target(12.0),
            vec![],
            vec![flash_patch(MapProvenance::Scanned)],
        )
        .is_err());

        // NaN is not a voltage.
        assert!(SterngateMod::create(
            sample_metadata(),
            sample_target(f64::NAN),
            vec![flash_patch(MapProvenance::Scanned)],
            vec![],
        )
        .is_err());
    }

    #[test]
    fn create_allows_did_writes_at_12v0() {
        let action = ModAction::WriteDid {
            did: 0x0110,
            data: vec![0x01, 0x2C],
            bitmask: None,
            expected_original_data: None,
            description: "vmax".into(),
        };
        assert!(SterngateMod::create(sample_metadata(), sample_target(12.0), vec![action], vec![]).is_ok());
    }
```

Rewrite the stage part of `test_bosch_checksum_solver_and_stage_generator` in `crates/sterngate-core/src/lib.rs` (lines 803–827): replace the Stage 1 and Stage 2 blocks with

```rust
        // Stage generation refuses until every targeted map is located in this ROM
        let stage1_err =
            StageGenerator::generate_stage1(&rom, "W211 E280 CDI", "EDC16CP31", "Sterngate Team")
                .unwrap_err();
        assert!(stage1_err.to_string().contains("Torque Limiter"));
        assert!(
            StageGenerator::generate_stage2(&rom, "W211 E280 CDI", "EDC16CP31", "Sterngate Team")
                .is_err()
        );
```

and drop the now-unused `ModCategory`/`ModRiskLevel`/`ModAction` assertions from that test. Keep the checksum-solver assertions unchanged.

Server `test_server_tuning_endpoints` (lib.rs ~1237–1282): change the Stage 1 and Stage 2 assertions to

```rust
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let stage1_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(!stage1_res["success"].as_bool().unwrap());
        assert!(stage1_res["error"].as_str().unwrap().contains("Torque Limiter"));
```

(and the equivalent for stage 2 asserting only the 422 and `success == false`). Add after them a DTC-kill request:

```rust
        let dtc_payload = json!({ "rom_base64": rom_b64, "p_codes": ["P0401"] });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/tuning/dtc/kill")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&dtc_payload).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
```

MCP `test_mcp_tuning_suite` (lib.rs ~744–790): stage 1, stage 2 and `sterngate_kill_dtc` calls become

```rust
        let stage1_err = tools::handle_tool_call(
            "sterngate_generate_stage_tune",
            &json!({ "rom_base64": rom_b64, "stage": 1, "chassis": "W211 E280 CDI", "ecu_name": "EDC16CP31" }),
        )
        .await
        .unwrap_err();
        assert!(stage1_err.contains("Torque Limiter"), "{stage1_err}");

        let stage2_err = tools::handle_tool_call(
            "sterngate_generate_stage_tune",
            &json!({ "rom_base64": rom_b64, "stage": 2, "chassis": "W211 E280 CDI", "ecu_name": "EDC16CP31" }),
        )
        .await
        .unwrap_err();
        assert!(stage2_err.contains("provenance"), "{stage2_err}");

        let dtc_err = tools::handle_tool_call(
            "sterngate_kill_dtc",
            &json!({ "rom_base64": rom_b64, "p_codes": ["P0401", "P2002"] }),
        )
        .await
        .unwrap_err();
        assert!(dtc_err.contains("unsupported"), "{dtc_err}");
```

Protocol `test_flash_write_floor_overrides_author_declared_minimum` (Task 5) can no longer build a 12.0 V flash package through `create`; rewrite it to forge one the way an attacker would:

```rust
    #[tokio::test]
    async fn test_flash_write_floor_overrides_author_declared_minimum() {
        let mut iface = VirtualCanInterface::new();
        iface.open().await.unwrap();
        // create() refuses < 12.5 V for flash writes, so forge the target after
        // creation; integrity does not cover the target yet (Task 8 changes that).
        let mut m = runner_mod(vec![patch(0x1C1000, MapProvenance::Scanned, Some(vec![0; 3]))], vec![], 12.5);
        m.target.min_battery_voltage = 12.0;
        let msg = err_text(ModRunner::apply_mod(&mut iface, &mut m, VIN_W211, 12.2, TargetFingerprintPolicy::Enforce).await);
        assert!(msg.contains("12.5"), "{msg}");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sterngate-core stage 2>&1 | tail -20`
Expected: compile error `no function or associated item named 'require_rom_backed'`; `create_rejects_flash_writes_below_12v5` fails.

- [ ] **Step 3: Implement the generator gate**

In `stage.rs`, add inside `impl StageGenerator` (before `generate_stage1`):

```rust
    /// A map may only become a flash patch when the detector located it in
    /// this ROM and the bytes it carries are the bytes at that address.
    pub(crate) fn require_rom_backed(map: &EcuMap, rom: &[u8]) -> Result<()> {
        if !map.provenance.is_scanned() || !map.is_rom_backed(rom) {
            return Err(SterngateError::PreFlightCheckFailed(format!(
                "refusing to emit flash patch for '{}' at 0x{:06X}: provenance {:?}, rom_backed={} (map was not located in this ROM)",
                map.name,
                map.address,
                map.provenance,
                map.is_rom_backed(rom)
            )));
        }
        Ok(())
    }
```

with `use super::map::{EcuMap, MapProvenance};` replacing the Task 4 import (keep `MapProvenance` only if still referenced; otherwise import just `EcuMap`).

In `generate_stage1` and `generate_stage2`, inside **every named match arm** (`"Torque Limiter"`, `"Driver's Wish (Fahrpedal)"`, `"Turbo Boost Target (Ladedruck-Soll)"`, `"Single Value Boost Limiter (SVBL)"`, `"Rail Pressure Target (Raildruck)"`, `"EGR Hysteresis (Abgasrückführung)"`), insert `Self::require_rom_backed(&map, rom)?;` as the first statement, before `map.modify_percentage(...)` / the zeroing. The `_ => {}` arm stays untouched.

In `generate_stage2`: add after `let maps = BoschMapDetector::scan_rom(rom);`

```rust
        if maps.is_empty() {
            return Err(SterngateError::ProfileError(
                "No calibration maps could be detected in the provided ROM".into(),
            ));
        }
```

and delete the two DTC-suppression blocks (lines 258–291, `// Add DTC suppression for EGR (P0401) and DPF (P2002)` through the end of the `P2002` block). Update the Stage 2 `name`/`description` strings to drop "P0401/P2002 DTC Suppressed" wording: name `"{} Stage 2 Race (+60 HP / +120 Nm, DPF/EGR Delete)"` stays; description becomes `"Stage 2 performance tune for {} {}. Requires physical DPF delete downpipe and EGR blanking plate. Bypasses DPF regeneration."`. In `routes/tuning.rs` line 178 change the summary to `"+25% Peak Torque, +200 mbar Boost, +80 bar Rail, DPF Off, EGR Hysteresis Off"`.

Replace `generate_dtc_kill` entirely:

```rust
    /// DTC suppression is unsupported until the detector understands the
    /// Bosch fault-path table. A raw 2-byte pattern hit is not evidence of a
    /// DTC entry, so no flash write may be minted from it.
    pub fn generate_dtc_kill(
        _rom: &[u8],
        _chassis: &str,
        _ecu_name: &str,
        p_codes: &[String],
        _author: &str,
    ) -> Result<SterngateMod> {
        Err(SterngateError::PreFlightCheckFailed(format!(
            "DTC fault-path table location is unsupported until the detector rebuild; refusing to emit flash writes for {p_codes:?} at assumed addresses"
        )))
    }
```

Remove the now-unused `use crate::calibrator::detector::BoschMapDetector` import only if the compiler reports it unused (it is still used by the stage generators).

- [ ] **Step 4: Implement the constructor floor**

In `modpack/mod.rs` `SterngateMod::create`, before `let canonical_payload = ...`:

```rust
        let writes_flash = actions
            .iter()
            .chain(rollback_actions.iter())
            .any(|a| matches!(a, ModAction::PatchFlashMap { .. } | ModAction::DtcMask { .. }));
        if writes_flash
            && (target.min_battery_voltage.is_nan()
                || target.min_battery_voltage < FLASH_WRITE_MIN_VOLTAGE)
        {
            return Err(SterngateError::PreFlightCheckFailed(format!(
                "packages containing flash writes must declare min_battery_voltage >= {FLASH_WRITE_MIN_VOLTAGE:.1} V (got {:.1})",
                target.min_battery_voltage
            )));
        }
```

- [ ] **Step 5: Map `PreFlightCheckFailed` to 422 in the tuning routes**

In `crates/sterngate-server/src/routes/tuning.rs`, add `use sterngate_core::SterngateError;` and a helper:

```rust
fn generation_error(prefix: &str, e: &SterngateError) -> axum::response::Response {
    let status = if matches!(e, SterngateError::PreFlightCheckFailed(_)) {
        StatusCode::UNPROCESSABLE_ENTITY
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    (
        status,
        Json(serde_json::json!({
            "success": false,
            "error": format!("{prefix}: {e}"),
        })),
    )
        .into_response()
}
```

and use it in the three `Err(e) =>` arms of `tuning_stage1`, `tuning_stage2` and `tuning_dtc_kill`: `Err(e) => generation_error("Failed to generate Stage 1 package", &e),` etc.

- [ ] **Step 6: Run the tests**

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | head -20`
Expected: all pass. `cargo run -q -p sterngate-cli -- tune stage1 --rom /dev/null` is not part of the test suite; the CLI prints the refusal.

- [ ] **Step 7: Gate and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result" | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
git add -A crates/
git commit -m "fix(calibrator): refuse to mint flash patches for maps not located in the ROM

Stage 1/2 abort on the first non-scanned map, DTC-kill is unsupported
until the detector rebuild, and create() refuses flash packages below
the 12.5 V floor. Tuning routes answer 422 for these refusals.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 8: `.sgmod` integrity version 2 covers the target filter

**Files:**
- Modify: `crates/sterngate-core/src/modpack/mod.rs` (`ModIntegrity`, `create`, `canonical_payload_bytes`, `verify_and_repair`), `crates/sterngate-protocol/src/lib.rs` (floor test), `profiles/mods/amg_needle_sweep.sgmod` (regenerated)
- Test: `modpack/mod.rs`

**Interfaces:**
- Produces: `MOD_INTEGRITY_VERSION: u8 = 2`; `ModIntegrity.version: u8` (`#[serde(default)]`, so legacy files read as 0); `SterngateMod::canonical_payload_bytes(target: &ModTargetFilter, actions: &[ModAction], rollback_actions: &[ModAction]) -> Result<Vec<u8>>`; `verify_and_repair` returns `is_valid == false` with a warning containing `integrity version` for any version ≠ 2 **before** attempting FEC, and repairs `target` as well as the action lists.

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `modpack/mod.rs`:

```rust
    fn created_flash_mod() -> SterngateMod {
        SterngateMod::create(
            sample_metadata(),
            sample_target(12.5),
            vec![flash_patch(MapProvenance::Scanned)],
            vec![],
        )
        .unwrap()
    }

    #[test]
    fn integrity_covers_target_filter() {
        for mutate in [
            (|m: &mut SterngateMod| m.target.tx_id = 0x7E1) as fn(&mut SterngateMod),
            |m| m.target.chassis.clear(),
            |m| m.target.min_battery_voltage = 9.0,
            |m| m.target.compatible_hw_ids.push("0281099999".into()),
        ] {
            let mut m = created_flash_mod();
            mutate(&mut m);
            // Corrupt enough bytes that FEC cannot silently repair the lie.
            m.integrity.fec_parity_bytes.clear();
            assert!(!m.verify_and_repair().unwrap().is_valid);
        }
    }

    #[test]
    fn fec_repairs_single_symbol_target_corruption() {
        let mut m = created_flash_mod();
        m.target.tx_id = 0x7E1; // canonical text "2016" -> "2017": one symbol
        let report = m.verify_and_repair().unwrap();
        assert!(report.is_valid);
        assert!(matches!(report.fec_status, FecStatus::Repaired { corrected_byte_count: 1, .. }));
        assert_eq!(m.target.tx_id, 0x7E0);
    }

    #[test]
    fn legacy_integrity_version_is_refused() {
        let legacy = r#"{
  "metadata": {"mod_id": "amg_needle_sweep_20260914", "name": "AMG Needle Sweep", "version": "1.0.0", "author": "CommunityTuner", "description": "Enables needle sweep on ignition", "category": "retrofit", "risk_level": "low", "instructions": null, "created_at": "2026-09-14T12:33:40Z"},
  "target": {"chassis": ["W211"], "ecu_name": "IC_211", "tx_id": 2016, "rx_id": 2024, "compatible_hw_ids": [], "compatible_sw_ids": [], "min_battery_voltage": 12.0, "requires_engine_off": true},
  "actions": [{"type": "write_did", "did": 432, "data": [2], "bitmask": null, "expected_original_data": null, "description": "Configure DID 0x01B0 on IC_211"}],
  "rollback_actions": [],
  "integrity": {"payload_crc32": 1479663775, "payload_sha256": "6e7b5bf2fede5951d756e44ee4fa6e3f677757bae4e5d4ac685c31e8f1b65e95", "fec_scheme": "ReedSolomon_GF256", "fec_parity_bytes": [180,64,66,164,58,116,220,2,143,192,75,73,227,68,30,27], "block_size": 239, "parity_size": 16}
}"#;
        let mut m = SterngateMod::from_json(legacy).unwrap();
        assert_eq!(m.integrity.version, 0);
        let report = m.verify_and_repair().unwrap();
        assert!(!report.is_valid);
        assert!(report.warning_messages[0].contains("integrity version 0"));
    }

    #[test]
    fn created_packages_carry_version_2() {
        assert_eq!(created_flash_mod().integrity.version, MOD_INTEGRITY_VERSION);
        assert_eq!(MOD_INTEGRITY_VERSION, 2);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sterngate-core integrity 2>&1 | tail -15`
Expected: compile error `no field 'version' on type 'ModIntegrity'` / `MOD_INTEGRITY_VERSION` not found.

- [ ] **Step 3: Implement**

In `modpack/mod.rs`:

```rust
/// Integrity format version. Version 2 covers `target`, `actions` and
/// `rollback_actions`. Any other value is refused: the target filter of
/// older packages was never integrity-protected.
pub const MOD_INTEGRITY_VERSION: u8 = 2;

/// Cryptographic and forward error correction integrity block
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModIntegrity {
    /// Absent in legacy packages (reads as 0, refused).
    #[serde(default)]
    pub version: u8,
    pub payload_crc32: u32,
    pub payload_sha256: String,
    pub fec_scheme: String,
    pub fec_parity_bytes: Vec<u8>,
    pub block_size: usize,
    pub parity_size: usize,
}

/// Field order is the integrity format: changing it, or any field set of
/// `ModTargetFilter`/`ModAction`, requires bumping `MOD_INTEGRITY_VERSION`.
#[derive(Serialize)]
struct CanonicalPayloadRef<'a> {
    target: &'a ModTargetFilter,
    actions: &'a [ModAction],
    rollback_actions: &'a [ModAction],
}

#[derive(Deserialize)]
struct CanonicalPayloadOwned {
    target: ModTargetFilter,
    actions: Vec<ModAction>,
    rollback_actions: Vec<ModAction>,
}
```

`create`: call `Self::canonical_payload_bytes(&target, &actions, &rollback_actions)?` and set `version: MOD_INTEGRITY_VERSION` in the `ModIntegrity` literal.

`canonical_payload_bytes`:

```rust
    /// Canonical serialization of the target filter, actions and rollback steps.
    pub fn canonical_payload_bytes(
        target: &ModTargetFilter,
        actions: &[ModAction],
        rollback_actions: &[ModAction],
    ) -> Result<Vec<u8>> {
        serde_json::to_vec(&CanonicalPayloadRef {
            target,
            actions,
            rollback_actions,
        })
        .map_err(|e| SterngateError::ProfileError(format!("Failed serializing mod payload: {}", e)))
    }
```

`verify_and_repair`: insert at the very top

```rust
        if self.integrity.version != MOD_INTEGRITY_VERSION {
            let reason = format!(
                "unsupported .sgmod integrity version {} (expected {}); the target filter is not integrity-protected, regenerate the package",
                self.integrity.version, MOD_INTEGRITY_VERSION
            );
            return Ok(ModValidationReport {
                is_valid: false,
                fec_status: FecStatus::Unrecoverable {
                    reason: reason.clone(),
                },
                crc32_verified: false,
                sha256_verified: false,
                matched_vehicle: false,
                compatibility_notes: vec![],
                warning_messages: vec![reason],
            });
        }
```

then compute `payload_bytes` with the three-argument function and, in the `FecStatus::Repaired` arm, deserialize `CanonicalPayloadOwned` and assign `self.target`, `self.actions`, `self.rollback_actions`.

Protocol test `test_flash_write_floor_overrides_author_declared_minimum`: the target is now integrity-covered, so the forged package must recompute its integrity the way a forger would. Add this helper to the protocol tests and use it:

```rust
    /// Recompute integrity over a mutated package (what a forger must do).
    fn resign(m: &mut SterngateMod) {
        use sha2::Digest;
        let payload = SterngateMod::canonical_payload_bytes(&m.target, &m.actions, &m.rollback_actions).unwrap();
        m.integrity.payload_crc32 = crc32fast::hash(&payload);
        let mut hasher = sha2::Sha256::new();
        Digest::update(&mut hasher, &payload);
        m.integrity.payload_sha256 = format!("{:x}", Digest::finalize(hasher));
        m.integrity.fec_parity_bytes = sterngate_core::ReedSolomonCodec::default_codec().encode(&payload);
    }
```

and in the test, after `m.target.min_battery_voltage = 12.0;` call `resign(&mut m);`.

- [ ] **Step 4: Regenerate the in-repo package**

```bash
cargo run -q -p sterngate-cli -- mod create --name "AMG Needle Sweep" --author CommunityTuner \
  --description "Enables needle sweep on ignition" --chassis W211 --ecu IC_211 --did 0x01B0 --data 02 \
  --category retrofit --risk safe --min-voltage 12.0 --out profiles/mods/amg_needle_sweep.sgmod > /dev/null
grep -n '"version": 2' profiles/mods/amg_needle_sweep.sgmod
cargo run -q -p sterngate-cli -- mod list | grep -i "needle"
```

Expected: the grep prints the version line; `mod list` shows `✓ Clean` for the package.

- [ ] **Step 5: Run the tests**

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | head`
Expected: all pass (armor round-trip, server/MCP create→inspect→apply flows create at v2).

- [ ] **Step 6: Gate and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result" | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
git add -A crates/ profiles/mods/amg_needle_sweep.sgmod
git commit -m "feat(modpack): integrity v2 covers the target filter; legacy packages are refused

A bit-flip or edit in tx_id, chassis, the HW whitelist or the voltage
minimum was invisible to CRC, SHA and Reed-Solomon. Version is checked
before FEC so an old parity block cannot 'repair' the new layout.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 9: No fabricated VIN or voltage at any entry point

**Files:**
- Modify: `crates/sterngate-hal/src/interface.rs`, `crates/sterngate-hal/src/openport.rs` (trait impl), `crates/sterngate-server/src/routes/community_mods.rs:125`, `crates/sterngate-server/src/routes/telemetry.rs:31-41`, `crates/sterngate-mcp/src/tools/community_mods.rs:72-80`, `crates/sterngate-mcp/src/tools/specs.rs` (apply tool `required` and descriptions), `crates/sterngate-cli/src/args.rs:602-604`, `crates/sterngate-cli/src/commands/modcmd.rs:176-208`, `crates/sterngate-server/static/js/app.js:2060-2100`
- Test: `crates/sterngate-hal/src/lib.rs`, `crates/sterngate-server/src/lib.rs`, `crates/sterngate-mcp/src/lib.rs`, `crates/sterngate-cli/src/args.rs`, `crates/sterngate-cli/src/commands/modcmd.rs`

**Interfaces:**
- Produces: `VehicleInterface::measure_battery_voltage(&mut self) -> Result<Option<f32>>` with default `Ok(None)` meaning "this adapter cannot measure"; the OpenPort hardware backend returns a Pin-16 reading; the simulated backend returns `Ok(None)`.
- Produces: `POST /api/v1/mods/apply` → 400 without a non-empty `vin`; `GET /api/v1/telemetry` `battery_voltage` is the measured value or `null`.
- Produces: MCP apply requires `vin` and `battery_voltage` (schema `required` and runtime errors).
- Produces: `sterngate mod apply <input> --vin <VIN>` (required); voltage from `measure_battery_voltage`, refusing with "cannot measure" otherwise; `modcmd::voltage_for_apply(reading, iface_name) -> anyhow::Result<f64>`.

- [ ] **Step 1: Write the failing tests**

HAL (`crates/sterngate-hal/src/lib.rs` tests):

```rust
    #[tokio::test]
    async fn test_virtual_can_cannot_measure_voltage() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();
        assert_eq!(sim.measure_battery_voltage().await.unwrap(), None);
    }

    #[tokio::test]
    async fn test_simulated_openport_is_not_a_measurement() {
        let (mut op, _feed) = OpenPortInterface::new_simulated(12.65);
        op.open().await.unwrap();
        assert_eq!(op.measure_battery_voltage().await.unwrap(), None);
    }
```

Server (`crates/sterngate-server/src/lib.rs` tests):

```rust
    #[tokio::test]
    async fn test_mods_apply_requires_vin() {
        let mut iface = Box::new(VirtualCanInterface::new());
        let _ = iface.open().await;
        let profile =
            VehicleProfile::load_from_file("../../profiles/mercedes/w211_om646_edc16.json")
                .unwrap();
        let flasher = Arc::new(FlashingWorker::new());
        let state = Arc::new(AppState::new(iface, profile, flasher));

        for payload in [
            json!({ "content": "{}", "battery_voltage": 12.8 }),
            json!({ "content": "{}", "vin": "   ", "battery_voltage": 12.8 }),
        ] {
            let req = Request::builder()
                .method("POST")
                .uri("/api/v1/mods/apply")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap();
            let resp = create_router(state.clone()).oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
            let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert!(v["error"].as_str().unwrap().contains("VIN"));
        }
    }
```

MCP (`crates/sterngate-mcp/src/lib.rs` tests):

```rust
    #[tokio::test]
    async fn test_mcp_apply_mod_requires_vin_and_voltage() {
        let no_vin = tools::handle_tool_call(
            "sterngate_apply_community_mod",
            &json!({ "mod_content": "{}", "battery_voltage": 12.8 }),
        )
        .await
        .unwrap_err();
        assert!(no_vin.contains("vin"), "{no_vin}");

        let no_volts = tools::handle_tool_call(
            "sterngate_apply_community_mod",
            &json!({ "mod_content": "{}", "vin": "WDB2112061A123456" }),
        )
        .await
        .unwrap_err();
        assert!(no_volts.contains("battery voltage"), "{no_volts}");

        let apply = tools::get_tools_list();
        let spec = apply
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "sterngate_apply_community_mod")
            .unwrap();
        let required: Vec<&str> = spec["inputSchema"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(required.contains(&"vin") && required.contains(&"battery_voltage"));
    }
```

CLI args (`crates/sterngate-cli/src/args.rs`, new module at the end of the file):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn mod_apply_requires_vin() {
        assert!(Cli::try_parse_from(["sterngate", "mod", "apply", "m.sgmod"]).is_err());
        let cli = Cli::try_parse_from([
            "sterngate",
            "mod",
            "apply",
            "m.sgmod",
            "--vin",
            "WDB2112061A123456",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Mod { action: ModCommands::Apply { ref vin, .. } }) if vin == "WDB2112061A123456"
        ));
    }
}
```

(Check the exact variant name of the `mod` subcommand in `Commands` — `grep -n "ModCommands" crates/sterngate-cli/src/args.rs` — and adjust `Commands::Mod { action: .. }` to match.)

CLI voltage helper (`crates/sterngate-cli/src/commands/modcmd.rs`, new module at the end):

```rust
#[cfg(test)]
mod tests {
    use super::voltage_for_apply;
    use sterngate_core::SterngateError;

    #[test]
    fn voltage_for_apply_refuses_when_unmeasurable() {
        let err = voltage_for_apply(Ok(None), "can0").unwrap_err();
        assert!(err.to_string().contains("cannot measure"));
        let err = voltage_for_apply(Err(SterngateError::HalError("usb".into())), "openport").unwrap_err();
        assert!(err.to_string().contains("usb"));
        let v = voltage_for_apply(Ok(Some(12.7)), "openport").unwrap();
        assert!((v - 12.7).abs() < 1e-6);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --workspace 2>&1 | grep -E "^error|FAILED" | head`
Expected: compile errors for `measure_battery_voltage` and `voltage_for_apply`; the server and MCP tests fail (200/Ok instead of refusal).

- [ ] **Step 3: HAL trait and OpenPort**

`crates/sterngate-hal/src/interface.rs`:

```rust
use async_trait::async_trait;
use sterngate_core::{CanFrame, Result};

#[async_trait]
pub trait VehicleInterface: Send + Sync {
    async fn open(&mut self) -> Result<()>;
    async fn send(&mut self, frame: CanFrame) -> Result<()>;
    async fn recv(&mut self) -> Result<CanFrame>;
    async fn close(&mut self) -> Result<()>;
    fn name(&self) -> &str;
    fn is_connected(&self) -> bool;

    /// Measure the vehicle battery voltage in volts, if this adapter has a
    /// sensor. `Ok(None)` means "cannot measure": callers that gate writes on
    /// voltage must refuse rather than assume. A simulated reading is not a
    /// measurement.
    async fn measure_battery_voltage(&mut self) -> Result<Option<f32>> {
        Ok(None)
    }
}
```

`crates/sterngate-hal/src/openport.rs`, inside `impl VehicleInterface for OpenPortInterface` (after `is_connected`):

```rust
    async fn measure_battery_voltage(&mut self) -> Result<Option<f32>> {
        let is_hardware = matches!(self.backend, Some(OpenPortBackend::Hardware { .. }));
        if is_hardware {
            self.read_battery_voltage().await.map(Some)
        } else {
            // The simulated constant is a test fixture, not a measurement.
            Ok(None)
        }
    }
```

- [ ] **Step 4: Server**

`routes/community_mods.rs`: delete line 125 (`let vin = payload.vin.as_deref().unwrap_or(...)`) and insert the following **directly after the `is_locked` 423 guard, before `decode_from_armor`** (so a missing VIN is reported even when the content is unparseable):

```rust
    let Some(vin) = payload
        .vin
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "error": "Refusing to apply: no target VIN supplied. The chassis fingerprint is meaningless without the connected vehicle's VIN.",
            })),
        )
            .into_response();
    };
```

`routes/telemetry.rs`, replace lines 31–41 with:

```rust
    let mut iface = state.interface.lock().await;
    // Only a real adapter reading may satisfy the flashing interlock; adapters
    // without a sensor report None and the UI shows the voltage as unknown.
    let battery_voltage = iface
        .measure_battery_voltage()
        .await
        .ok()
        .flatten()
        .map(f64::from);
    let mut snap = TelemetrySnapshot {
        timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
        battery_voltage,
        ..Default::default()
    };
```

- [ ] **Step 5: MCP**

`tools/community_mods.rs` apply arm:

```rust
            let vin = arguments
                .get("vin")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or("Missing required 'vin': the chassis fingerprint needs the connected vehicle's VIN")?;
            let battery_voltage = arguments
                .get("battery_voltage")
                .and_then(|v| v.as_f64())
                .ok_or("Refusing to apply: no measured battery voltage supplied ('battery_voltage')")?;
```

`tools/specs.rs` apply tool: `"required": ["mod_content", "vin", "battery_voltage"]`; `vin` description → `"Target vehicle VIN as read from the connected vehicle (required)"`; `battery_voltage` description → `"Measured battery voltage from real hardware (required; no default)"`.

- [ ] **Step 6: CLI**

`args.rs` `ModCommands::Apply`:

```rust
    /// Safely apply a community mod to the vehicle
    Apply {
        /// Path to .sgmod file, or '-' for stdin, or raw armored text
        input: String,
        /// VIN of the connected vehicle (required: the chassis fingerprint is checked against it)
        #[arg(long)]
        vin: String,
        /// Relax the chassis and hardware-whitelist fingerprint checks only. Voltage, map
        /// provenance and byte preconditions stay enforced; refused for packages that write flash.
        #[arg(long)]
        force: bool,
    },
```

`modcmd.rs`: add the helper at module level

```rust
/// Turn an adapter voltage reading into the value `apply_mod` requires, or
/// refuse. Only a real measurement may clear the interlock.
pub(crate) fn voltage_for_apply(
    reading: sterngate_core::Result<Option<f32>>,
    iface_name: &str,
) -> Result<f64> {
    match reading {
        Ok(Some(v)) => Ok(f64::from(v)),
        Ok(None) => anyhow::bail!(
            "Refusing to apply: interface `{iface_name}` cannot measure battery voltage (Tactrix OpenPort Pin 16 ADC required)"
        ),
        Err(e) => anyhow::bail!("Refusing to apply: battery voltage read failed: {e}"),
    }
}
```

and rewrite the `Apply` arm's beginning:

```rust
        ModCommands::Apply { input, vin, force } => {
            let mut modpack = load_mod_input(&input)?;
            let mut iface = open_interface(&cli.can_interface).await;
            let battery_voltage =
                voltage_for_apply(iface.measure_battery_voltage().await, &cli.can_interface)?;
            let policy = if force {
                TargetFingerprintPolicy::BypassUnsafe
            } else {
                TargetFingerprintPolicy::Enforce
            };
            println!("============================================================");
            println!("  APPLYING COMMUNITY MOD: {}", modpack.metadata.name);
            println!("============================================================");
            println!("  Target VIN:     {}", vin);
            println!("  Interface:      {}", cli.can_interface);
            println!("  Battery:        {:.2} V (measured)", battery_voltage);
            println!("  Risk Level:     {}", modpack.metadata.risk_level.as_str());
            if force {
                println!("  ⚠️  FORCED BYPASS OF CHASSIS / HW-ID FINGERPRINT (voltage, map provenance and byte preconditions remain enforced)");
            }

            let report =
                ModRunner::apply_mod(iface.as_mut(), &mut modpack, &vin, battery_voltage, policy)
                    .await?;
```

Delete the garage-first-vehicle fallback (old lines 178–186) and the `let battery_voltage = 13.2;` line; drop the now-unused `VehicleGarage` import if the compiler reports it.

- [ ] **Step 7: UI**

In `app.js` `applyCommunityMod()` (lines 2060–2100): after the `if (!content) return;` line add

```js
  if (!activeVehicleVin) {
    alert('Scan the connected vehicle first: applying a mod requires its VIN.');
    return;
  }
```

and replace the request body with

```js
      body: JSON.stringify({
        content: content,
        vin: activeVehicleVin,
        // Sent only when actually measured; the server refuses otherwise.
        battery_voltage: (lastTelemetrySnap && lastTelemetrySnap.battery_voltage !== null && lastTelemetrySnap.battery_voltage !== undefined)
          ? lastTelemetrySnap.battery_voltage
          : null
      })
```

Run `node --check crates/sterngate-server/static/js/app.js`.

- [ ] **Step 8: Run the tests**

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | head`
Expected: all pass. Existing server/MCP mod flows already send a VIN and voltage.

- [ ] **Step 9: Gate and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result" | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
git add -A crates/
git commit -m "fix(apply): require the connected VIN and a measured voltage at every entry point

The CLI no longer assumes 13.2 V; voltage comes from the OpenPort ADC
through VehicleInterface::measure_battery_voltage, adapters without a
sensor are refused, and REST/MCP/UI stop substituting a W211 VIN.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 10: Documentation of the new contracts

**Files:**
- Modify: `.agents/skills/community-mods/SKILL.md`, `.agents/skills/sterngate-mcp/SKILL.md:76-77`, `.agents/skills/ecu-tuning/SKILL.md:95-110`, `.agents/skills/sterngate-ops/SKILL.md:584-587`, `.agents/skills/safe-flashing/SKILL.md` (section that documents `measured_voltage`, ~line 118), `AGENTS.md:62-64,82`, `README.md` (only if it lists `mod apply` without `--vin`: `grep -n "mod apply" README.md`)

- [ ] **Step 1: community-mods skill**

Add a section `## Safety contract (Phase 0)` after the existing action-type documentation containing, verbatim:

```markdown
## Safety contract (Phase 0)

- `PatchFlashMap` and `DtcMask` actions carry `"provenance"`; only `"scanned"` (bytes located in the target ECU's own ROM) is executable. Absent or any other value is refused before any bus traffic.
- Every flash patch must carry `expected_original_data` of exactly the patched length; the runner reads the live bytes and refuses on read failure, short reply or mismatch. `DtcMask` requires the live byte to equal `original_mask`.
- Packages that write flash require `min_battery_voltage >= 12.5`; `SterngateMod::create` refuses lower values and `ModRunner` enforces `max(min_battery_voltage, 12.5)` regardless of the CLI `--force` flag.
- `--force` (CLI only) relaxes the chassis and hardware-whitelist checks and nothing else. It is refused for packages containing flash writes. The REST API and MCP have no bypass: sending `force` returns HTTP 422 / an MCP error.
- `integrity.version` is 2 and covers `target`, `actions` and `rollback_actions`. Packages with any other version are refused everywhere (inspect included); regenerate them with `sterngate mod create`.
- Applying requires the connected vehicle's VIN (`--vin`, `vin`) and a measured battery voltage. The CLI reads the Tactrix OpenPort Pin-16 ADC and refuses on SocketCAN or mock adapters, which cannot measure.
- `sterngate tune stage1|stage2|dtc-kill` refuse on every ROM until the detector rebuild locates real maps; this is intended.
```

Update the CLI example at line 75 to keep `--vin` (already present) and add a note that `--vin` is required.

- [ ] **Step 2: MCP skill**

Replace lines 76–77 with:

```markdown
| `sterngate_inspect_community_mod` | `mod_content`, `vin` (opt), `battery_voltage` (opt) | Validates integrity (version 2), chassis compatibility and Reed-Solomon repair. |
| `sterngate_apply_community_mod` | `mod_content`, `vin`, `battery_voltage` (all required) | Applies a package with provenance, integrity, voltage floor and live byte preconditions enforced. No `force` argument exists; sending one is an error. |
```

- [ ] **Step 3: ecu-tuning skill**

After the Stage 1 workflow block (lines 95–110) add:

```markdown
> **Phase 0 state:** `stage1`, `stage2` and `dtc-kill` currently refuse on every ROM: the detector only locates the SVBL by scanning, every other map is a placeholder, and no flash patch may be minted for a map that was not located in the ROM. Generation resumes with the Phase 2 detector rebuild validated against the SDflash reference corpus.
```

- [ ] **Step 4: ops and safe-flashing skills**

`sterngate-ops/SKILL.md` line 587: keep `--vin` and add the line `# --vin is required; voltage is read from the OpenPort ADC (refused on can0/mock)` above it.

`safe-flashing/SKILL.md`, in the section around line 118, add a bullet: `- ``sterngate mod apply`` and ``POST /api/v1/mods/apply`` follow the same rule: the CLI reads the OpenPort Pin-16 ADC through ``VehicleInterface::measure_battery_voltage`` and refuses on adapters without a sensor; the REST route requires ``battery_voltage`` and the connected VIN.`

- [ ] **Step 5: AGENTS.md**

Line 62–64 bullets: append to the `BoschMapDetector` bullet `Every detected map carries a `MapProvenance` (`scanned`/`fallback`/`synthetic`); only scanned, ROM-backed maps may become flash patches.` Append to the `SterngateMod` bullet `Integrity version 2 covers the target filter; flash-writing packages require ≥ 12.5 V.` Line 82 `ModRunner` bullet: append `Fail-closed: provenance gate before bus traffic, exact byte preconditions, hardware-whitelist read failure refuses, `TargetFingerprintPolicy::BypassUnsafe` (CLI `--force`) relaxes only chassis/HW-ID and is refused for flash writes.`

- [ ] **Step 6: Verify and commit**

```bash
cargo test --workspace 2>&1 | grep -E "^test result" | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
git add .agents/skills AGENTS.md README.md
git commit -m "docs: record the fail-closed .sgmod apply contract in the skills and handbook

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

## Self-review

- **Spec coverage.** Spec 4.1 rows: mock (T1), provenance types (T4), detector honesty (T2, T4), producer gate (T7), package constructor (T5 constant, T7 floor, T8 integrity), armor (T3), runner (T5), entry points (T6, T9), voltage source (T9), docs (T10). Spec 4.3 gate order is the order in T5's `apply_mod`. Spec 4.4 tests: all listed for these units are present except "synthetic provenance refused with `0x10` in the failing set", replaced by the stronger unopened-interface variant. Flasher and vault rows belong to Plan 0b.
- **Placeholders.** None: every step carries code, a command and an expected result.
- **Type consistency.** `TargetFingerprintPolicy::{Enforce, BypassUnsafe}` used identically in T5, T6, T9; `MapProvenance::{Unverified, Synthetic, Fallback, Scanned}` in T4, T5, T7; `FLASH_WRITE_MIN_VOLTAGE` defined in T5 and used in T7; `canonical_payload_bytes(target, actions, rollback)` defined in T8 and used by the T8 `resign` helper; `measure_battery_voltage` and `voltage_for_apply` signatures match between T9 code and tests.
- **Known intermediate states.** After T5 the REST payload silently ignores `force` (fixed in T6); after T7 the protocol floor test mutates an uncovered target (fixed in T8). Both are called out inline.
