# Phase 0b — Flasher, UDS/ISO-TP hardening, vault fail-closed — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the flashing worker, the UDS/ISO-TP transport and the firmware vault fail closed: no silently ignored step, no panic on a malformed CAN frame, a real hardware-identity gate, keep-alive in every long session, no container flashed as raw bytes, and every voltage that gates a write measured by the server or CLI itself.

**Architecture:** A `ScriptedInterface` test double drives every transport-level test. `IsoTpChannel` gets length-checked frame accessors, Flow-Control BlockSize/WAIT handling and a settable timeout; `UdsClient` learns P2\*, suppressed TesterPresent and reply parsers; `FlashingWorker::execute_flash` becomes one locked sequence where every step uses `?`, guarded by an `S3KeepAlive` and ending in `Failed` with an honest message. Preflight compares F192 exactly. The vault marks containers non-stageable via a new `cff::sniff`. Voltage for flashing and applying is measured through `VehicleInterface::measure_battery_voltage` where the adapter can, with client values only as a cross-check.

**Tech Stack:** Rust 2021 workspace (tokio with paused-time tests, serde, axum 0.8, clap 4, async-trait), `VirtualCanInterface` mock, `ScriptedInterface` test double.

**Spec:** `docs/superpowers/specs/2026-09-15-map-studio-rebuild-phase0-phase1-design.md` sections 3 (D3, D7, D8, D9, D11), 4.1 rows "Flasher" and "Vault", 4.2, 4.4 (flasher/vault cases), 4.5 steps 8–9, 6 (client-supplied voltage). Also the Phase 0a residuals recorded in the branch review: inspect surfaces, ModRunner keep-alive, OpenPort reopen.

## Global Constraints

- Gate before every commit: `cargo fmt --all` then `cargo clippy --workspace --all-targets -- -D warnings` then `cargo test --workspace`. Baseline 151 passing; the count only goes up.
- No `unwrap()`/indexing that can panic on request or CAN paths (`AGENTS.md` invariant 6; release profile is `panic = "abort"`). Use `get`, `first`, `try_from`, `from_be_bytes`, `checked_*`.
- Flash-write and erase voltage floor is 12.5 V (`FLASH_WRITE_MIN_VOLTAGE` in `sterngate_core`).
- TesterPresent `3E 80` at least every 2000 ms in an extended or programming session (invariant 3); this plan uses a 1500 ms interval.
- HTTP 423 for any interface-touching route while `FlashingWorker` is locked (invariant 4).
- Flash constants: `FLASH_TX_ID = 0x7E0`, `FLASH_RX_ID = 0x7E8`, `ERASE_ROUTINE_ID = 0xFF00`, `CHECKSUM_ROUTINE_ID = 0x0202`, `CHECKSUM_STATUS_OK = 0x00`, `ISOTP_MAX_PAYLOAD = 4095`, `S3_KEEPALIVE_INTERVAL = 1500 ms`, `P2_STAR_MARGIN = 500 ms`, `N_WFT_MAX = 8`.
- Timing tests use `#[tokio::test(start_paused = true)]` so `tokio::time::sleep` advances the paused clock deterministically; keep-alive and P2\* logic must use `tokio::time::Instant`.
- `CLAUDE.md` is a symlink to `AGENTS.md`; edit `AGENTS.md` only.
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.
- Branch `feat/map-studio-rebuild-p0b` from `master` (currently `0408eb2`).

## File map

| File | Responsibility after this plan |
|---|---|
| `crates/sterngate-protocol/src/test_support.rs` (new, `#[cfg(test)]`) | `ScriptedInterface`: replies to UDS requests from a script, records sent frames, optional delays, connected flag |
| `crates/sterngate-protocol/src/isotp.rs` | No unchecked indexing; `set_timeout`/`timeout`; honours FC BlockSize, WAIT and OVFLW |
| `crates/sterngate-protocol/src/uds.rs` | P2\* from session control; `tester_present_suppressed`; stray `7E` discarded; `S3KeepAlive`; `parse_routine_status`; `parse_request_download` |
| `crates/sterngate-protocol/src/flasher.rs` | One locked programming sequence with `?` everywhere, keep-alive, manifest-driven download, block echo, checksum before reset, `Failed` with pre/post-erase message; exact F192 preflight; `inspect_rom` fail-closed |
| `crates/sterngate-hal/src/mock.rs` | Answers `0x34/0x36/0x37/0x28/0x85`, echoes the DID for `0x2E`, suppresses `3E 80`, ignores tester Flow Control, sends multi-frame ASCII replies for F192/F194 |
| `crates/sterngate-hal/src/openport.rs` | Reopen after close rebuilds the RX channel; close clears the voltage cache |
| `crates/sterngate-core/src/cff/mod.rs` (new) | `sniff(&[u8]) -> bool` (Phase 1 adds the parser) |
| `crates/sterngate-core/src/flash.rs` | `FirmwareVaultEntry.stageable`; containers never stageable; recommendation skips non-stageable |
| `crates/sterngate-core/src/modpack/mod.rs` | `ERASE_MEMORY_ROUTINE`, `SterngateMod::writes_flash`, `flash_write_refusals`; `check_compatibility` applies the flash floor and the refusals |
| `crates/sterngate-protocol/src/modrunner.rs` | Uses the core refusals; keep-alive ticks around the garage commit and writes |
| `crates/sterngate-server/src/routes/common.rs` | `resolve_flash_voltage(iface, payload)` shared by stage/vault/apply |
| `crates/sterngate-server/src/routes/{flashing,community_mods,diagnostics}.rs` | 423 guards; server-side voltage; `vault_stage` refuses containers |
| `crates/sterngate-mcp/src/tools/flashing.rs` | `sterngate_verify_flash_staging` runs the real preflight; demo manifest ids match the virtual ECU |
| `crates/sterngate-cli/src/commands/flash.rs` | Voltage from the opened interface; no 12.6 fallback; no second OpenPort handle |
| `.gitignore`, `.agents/skills/*/SKILL.md`, `AGENTS.md`, `README.md` | Container patterns; documentation of the new contracts |

---

### Task 0: Branch

- [ ] **Step 1: Create the feature branch**

```bash
cd /home/kim/projects/sterngate
git checkout master && git checkout -b feat/map-studio-rebuild-p0b
cargo test --workspace 2>&1 | grep -E "^test result" | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
```

Expected: `passed=151 failed=0`.

---

### Task 1: `ScriptedInterface` and a panic-free ISO-TP receive path

**Files:**
- Create: `crates/sterngate-protocol/src/test_support.rs`
- Modify: `crates/sterngate-protocol/src/lib.rs` (add `#[cfg(test)] mod test_support;`), `crates/sterngate-protocol/src/isotp.rs`
- Test: `crates/sterngate-protocol/src/isotp.rs` (new `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `crate::test_support::ScriptedInterface` with `new()`, `disconnected()`, `rule(sid, &[&[u8]])`, `rule_once(sid, &[&[u8]])`, `rule_delayed(sid, Duration, &[&[u8]])` (answers `7F <sid> 78` at once, then the replies after the delay: a slow ECU that honours P2\*), `raw_frames(&[&[u8]])` (frames delivered before any request), `sent_frames() -> Vec<CanFrame>`, `sent_services() -> Vec<u8>`; implements `VehicleInterface` (rx id 0x7E8, tx id 0x7E0).
- Produces: `IsoTpChannel::recv_payload` returns `Err(SterngateError::IsoTpError(_))` (never panics) for an empty frame, a First Frame shorter than 8 bytes or announcing fewer than 8 bytes, a short Consecutive Frame, or a Flow Control shorter than 3 bytes; `IsoTpChannel::set_timeout(Duration)` and `timeout() -> Duration`.

- [ ] **Step 1: Write the test double**

Create `crates/sterngate-protocol/src/test_support.rs`:

```rust
//! Test double for transport and flasher tests: answers UDS requests from a
//! script, records every frame the tester sent, and can delay replies so
//! paused-time tests can exercise timeouts and keep-alives.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use sterngate_core::{CanFrame, Result, SterngateError};
use sterngate_hal::VehicleInterface;

pub(crate) const TESTER_TX_ID: u16 = 0x7E0;
pub(crate) const ECU_RX_ID: u16 = 0x7E8;

struct Rule {
    sid: u8,
    replies: Vec<Vec<u8>>,
    delay: Duration,
    once: bool,
}

/// Service byte of a single-frame or first-frame request; `None` for CF/FC.
pub(crate) fn request_sid(data: &[u8]) -> Option<u8> {
    match data.first()? >> 4 {
        0x0 => data.get(1).copied(),
        0x1 => data.get(2).copied(),
        _ => None,
    }
}

pub(crate) struct ScriptedInterface {
    rules: Vec<Rule>,
    pending: VecDeque<(Duration, Vec<u8>)>,
    sent: Arc<Mutex<Vec<CanFrame>>>,
    connected: bool,
}

impl ScriptedInterface {
    pub(crate) fn new() -> Self {
        Self {
            rules: Vec::new(),
            pending: VecDeque::new(),
            sent: Arc::default(),
            connected: true,
        }
    }

    /// `is_connected()` reports false and every `send` fails.
    pub(crate) fn disconnected(mut self) -> Self {
        self.connected = false;
        self
    }

    /// Answer every request with service `sid` with these raw frames, in order.
    pub(crate) fn rule(mut self, sid: u8, replies: &[&[u8]]) -> Self {
        self.push_rule(sid, replies, Duration::ZERO, false);
        self
    }

    /// Like `rule`, but consumed after its first use (script sequence-dependent replies).
    pub(crate) fn rule_once(mut self, sid: u8, replies: &[&[u8]]) -> Self {
        self.push_rule(sid, replies, Duration::ZERO, true);
        self
    }

    /// A slow ECU: answers `7F <sid> 78` (ResponsePending) immediately, then
    /// delivers the first reply frame only after `delay`. Under the 1500 ms
    /// ISO-TP timeout a delayed reply is only reachable through P2*.
    pub(crate) fn rule_delayed(mut self, sid: u8, delay: Duration, replies: &[&[u8]]) -> Self {
        self.push_rule(sid, replies, delay, false);
        self
    }

    /// Frames delivered by `recv` before any request is sent (raw ECU traffic).
    pub(crate) fn raw_frames(mut self, frames: &[&[u8]]) -> Self {
        for f in frames {
            self.pending.push_back((Duration::ZERO, f.to_vec()));
        }
        self
    }

    fn push_rule(&mut self, sid: u8, replies: &[&[u8]], delay: Duration, once: bool) {
        self.rules.push(Rule {
            sid,
            replies: replies.iter().map(|r| r.to_vec()).collect(),
            delay,
            once,
        });
    }

    pub(crate) fn sent_frames(&self) -> Vec<CanFrame> {
        self.sent.lock().map(|v| v.clone()).unwrap_or_default()
    }

    /// Service byte of every SF/FF request sent, in order (CF and FC frames skipped).
    pub(crate) fn sent_services(&self) -> Vec<u8> {
        self.sent_frames()
            .iter()
            .filter_map(|f| request_sid(&f.data))
            .collect()
    }

    /// Shared handle so a test can inspect frames while the interface is borrowed.
    pub(crate) fn sent_handle(&self) -> Arc<Mutex<Vec<CanFrame>>> {
        Arc::clone(&self.sent)
    }
}

#[async_trait]
impl VehicleInterface for ScriptedInterface {
    async fn open(&mut self) -> Result<()> {
        Ok(())
    }

    async fn send(&mut self, frame: CanFrame) -> Result<()> {
        if !self.connected {
            return Err(SterngateError::DeviceNotFound("scripted interface is disconnected".into()));
        }
        if let Ok(mut v) = self.sent.lock() {
            v.push(frame.clone());
        }
        let Some(sid) = request_sid(&frame.data) else {
            return Ok(()); // CF or FC from the tester: never answered directly
        };
        let Some(idx) = self.rules.iter().position(|r| r.sid == sid) else {
            return Ok(()); // no script: the tester will time out
        };
        let (replies, delay) = {
            let r = &self.rules[idx];
            (r.replies.clone(), r.delay)
        };
        if self.rules[idx].once {
            self.rules.remove(idx);
        }
        if !delay.is_zero() {
            // A compliant ECU that needs longer than P2 answers ResponsePending first.
            self.pending.push_back((Duration::ZERO, vec![0x03, 0x7F, sid, 0x78]));
        }
        for (i, reply) in replies.into_iter().enumerate() {
            let d = if i == 0 { delay } else { Duration::ZERO };
            self.pending.push_back((d, reply));
        }
        Ok(())
    }

    async fn recv(&mut self) -> Result<CanFrame> {
        loop {
            if let Some((delay, data)) = self.pending.pop_front() {
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
                return Ok(CanFrame::new_standard(ECU_RX_ID, &data));
            }
            // Nothing scripted: park so the caller's timeout fires (paused-time aware).
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    async fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn name(&self) -> &str {
        "ScriptedInterface"
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}
```

Add to `crates/sterngate-protocol/src/lib.rs` after `pub mod uds;`:

```rust
#[cfg(test)]
pub(crate) mod test_support;
```

- [ ] **Step 2: Write the failing ISO-TP tests**

Append to `crates/sterngate-protocol/src/isotp.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ScriptedInterface;
    use std::time::Duration;

    #[tokio::test(start_paused = true)]
    async fn recv_payload_empty_frame_is_error_not_panic() {
        let mut iface = ScriptedInterface::new().raw_frames(&[&[]]);
        let mut ch = IsoTpChannel::new(&mut iface, 0x7E0, 0x7E8);
        assert!(matches!(ch.recv_payload().await, Err(SterngateError::IsoTpError(_))));
    }

    #[tokio::test(start_paused = true)]
    async fn first_frame_shorter_than_8_is_error() {
        let mut iface = ScriptedInterface::new().raw_frames(&[&[0x10, 0x0A, 0x62]]);
        let mut ch = IsoTpChannel::new(&mut iface, 0x7E0, 0x7E8);
        assert!(matches!(ch.recv_payload().await, Err(SterngateError::IsoTpError(_))));
    }

    #[tokio::test(start_paused = true)]
    async fn first_frame_announcing_less_than_8_is_error() {
        let mut iface =
            ScriptedInterface::new().raw_frames(&[&[0x10, 0x05, 0x62, 0xF1, 0x92, 0x30, 0x31, 0x32]]);
        let mut ch = IsoTpChannel::new(&mut iface, 0x7E0, 0x7E8);
        assert!(matches!(ch.recv_payload().await, Err(SterngateError::IsoTpError(_))));
    }

    #[tokio::test(start_paused = true)]
    async fn consecutive_frame_short_is_error() {
        let mut iface = ScriptedInterface::new().raw_frames(&[
            &[0x10, 0x0D, 0x62, 0xF1, 0x92, 0x30, 0x32, 0x38],
            &[0x21],
        ]);
        let mut ch = IsoTpChannel::new(&mut iface, 0x7E0, 0x7E8);
        assert!(matches!(ch.recv_payload().await, Err(SterngateError::IsoTpError(_))));
    }

    #[tokio::test(start_paused = true)]
    async fn flow_control_short_is_error() {
        // 20-byte payload -> FF; the ECU answers with a 1-byte FC.
        let mut iface = ScriptedInterface::new().rule(0x2E, &[&[0x30]]);
        let mut ch = IsoTpChannel::new(&mut iface, 0x7E0, 0x7E8);
        let payload = vec![0x2E; 20];
        assert!(matches!(ch.send_payload(&payload).await, Err(SterngateError::IsoTpError(_))));
    }

    #[tokio::test(start_paused = true)]
    async fn multi_frame_reply_reassembles() {
        // 62 F1 92 + "0281012224" = 13 bytes
        let mut iface = ScriptedInterface::new().rule(
            0x22,
            &[
                &[0x10, 0x0D, 0x62, 0xF1, 0x92, b'0', b'2', b'8'],
                &[0x21, b'1', b'0', b'1', b'2', b'2', b'2', b'4'],
            ],
        );
        let mut ch = IsoTpChannel::new(&mut iface, 0x7E0, 0x7E8);
        ch.send_payload(&[0x22, 0xF1, 0x92]).await.unwrap();
        let resp = ch.recv_payload().await.unwrap();
        assert_eq!(&resp[3..], b"0281012224");
        // The receiver answered the FF with a Flow Control frame.
        assert!(iface.sent_frames().iter().any(|f| f.data.first() == Some(&0x30)));
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_is_settable() {
        let mut iface = ScriptedInterface::new();
        let mut ch = IsoTpChannel::new(&mut iface, 0x7E0, 0x7E8);
        assert_eq!(ch.timeout(), Duration::from_millis(1500));
        ch.set_timeout(Duration::from_millis(50));
        assert_eq!(ch.timeout(), Duration::from_millis(50));
        assert!(matches!(ch.recv_payload().await, Err(SterngateError::IsoTpTimeout)));
    }
}
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p sterngate-protocol isotp:: 2>&1 | tail -25`
Expected: `recv_payload_empty_frame_is_error_not_panic` panics (`index out of bounds`), `first_frame_shorter_than_8_is_error` panics, `consecutive_frame_short_is_error` panics, `flow_control_short_is_error` panics, `timeout_is_settable` fails to compile (`no method named set_timeout`).

- [ ] **Step 4: Implement**

In `isotp.rs`:

1. Add after `with_timeout`:

```rust
    /// Change the receive timeout (used while the ECU reports NRC 0x78 ResponsePending).
    pub fn set_timeout(&mut self, duration: Duration) {
        self.timeout_duration = duration;
    }

    pub fn timeout(&self) -> Duration {
        self.timeout_duration
    }
```

2. Add a private helper near the top of the file:

```rust
fn pci_byte(frame: &CanFrame) -> Result<u8> {
    frame
        .data
        .first()
        .copied()
        .ok_or_else(|| SterngateError::IsoTpError("Empty CAN frame on ISO-TP channel".into()))
}
```

3. In `recv_payload`, replace `let pci_type = (frame.data[0] >> 4) & 0x0F;` with `let pci = pci_byte(&frame)?; let pci_type = (pci >> 4) & 0x0F;` and use `pci` instead of `frame.data[0]` in the SF arm (`let sf_len = usize::from(pci & 0x0F);`). Replace the FF arm's head with:

```rust
            0x01 => {
                if frame.data.len() < 8 {
                    return Err(SterngateError::IsoTpError(format!(
                        "First Frame too short: {} bytes",
                        frame.data.len()
                    )));
                }
                let total_len = (usize::from(pci & 0x0F) << 8) | usize::from(frame.data[1]);
                if total_len < 8 {
                    return Err(SterngateError::IsoTpError(format!(
                        "First Frame announces {total_len} bytes; a multi-frame message carries at least 8"
                    )));
                }
                let mut buffer = Vec::with_capacity(total_len);
                buffer.extend_from_slice(&frame.data[2..8]);
```

(The `frame.data[1]` and `[2..8]` indexes are now guarded by the length check.) In the CF loop replace the PCI/SN extraction and the slice with:

```rust
                    let cf_pci_byte = pci_byte(&cf_frame)?;
                    let cf_pci = (cf_pci_byte >> 4) & 0x0F;
                    let sn = cf_pci_byte & 0x0F;
                    if cf_pci != 0x02 || sn != expected_sn {
                        return Err(SterngateError::IsoTpError(format!(
                            "Out-of-order CF: expected {}, got {}",
                            expected_sn, sn
                        )));
                    }
                    let remaining = total_len - buffer.len();
                    let available = cf_frame.data.len().saturating_sub(1);
                    let take_len = remaining.min(7).min(available);
                    if take_len == 0 {
                        return Err(SterngateError::IsoTpError("Truncated Consecutive Frame".into()));
                    }
                    buffer.extend_from_slice(&cf_frame.data[1..1 + take_len]);
```

4. In `wait_for_flow_control`, replace `f.data[0] >> 4 == 0x03` with `f.data.first().is_some_and(|b| (b >> 4) == 0x03)`, and after the frame is received add:

```rust
        if frame.data.len() < 3 {
            return Err(SterngateError::IsoTpError(format!(
                "Flow Control frame too short: {} bytes",
                frame.data.len()
            )));
        }
```

before reading `frame.data[0] & 0x0F`. (Task 4 extends this function further.)

- [ ] **Step 5: Run the tests**

Run: `cargo test -p sterngate-protocol 2>&1 | tail -6` → all pass, including the seven new ones.

- [ ] **Step 6: Gate and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result" | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
git add crates/sterngate-protocol/src/test_support.rs crates/sterngate-protocol/src/lib.rs crates/sterngate-protocol/src/isotp.rs
git commit -m "fix(protocol): make the ISO-TP receive path length-checked and add a scripted test interface

A short or empty frame on 0x7E8 indexed past the end of the CAN data and,
with panic=abort, killed the process mid-session.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: P2\* ResponsePending handling

**Files:**
- Modify: `crates/sterngate-protocol/src/uds.rs` (`send_request`, `diagnostic_session_control`, new field)
- Test: `crates/sterngate-protocol/src/uds.rs` (new `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `UdsClient` remembers `p2_star: Duration` (default 5000 ms) parsed from a `0x50` reply (`resp[2..4]` = P2 ms, `resp[4..6]` = P2\* in 10 ms units, widened to `u64` before multiplying); on the first NRC 0x78 of a request the channel timeout becomes `p2_star + P2_STAR_MARGIN` and is restored on every exit path; the 100 ms sleep is removed.

- [ ] **Step 1: Write the failing tests**

Append to `uds.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ScriptedInterface;
    use std::time::Duration;

    const SESSION_OK: &[u8] = &[0x06, 0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]; // P2 50 ms, P2* 5000 ms

    #[tokio::test(start_paused = true)]
    async fn pending_response_waits_p2_star() {
        let mut iface = ScriptedInterface::new()
            .rule(0x10, &[SESSION_OK])
            .rule_delayed(0x31, Duration::from_millis(4000), &[&[0x05, 0x71, 0x01, 0xFF, 0x00, 0x00]]);
        let mut uds = UdsClient::new(&mut iface, 0x7E0, 0x7E8);
        uds.diagnostic_session_control(0x03).await.unwrap();
        // The double answers 0x78 at once; the real reply arrives 4 s later, inside P2* (5 s).
        let resp = uds.routine_control(0x01, 0xFF00, &[]).await.unwrap();
        assert_eq!(resp, vec![0x71, 0x01, 0xFF, 0x00, 0x00]);
    }

    #[tokio::test(start_paused = true)]
    async fn pending_response_times_out_after_p2_star() {
        let mut iface = ScriptedInterface::new()
            .rule(0x10, &[SESSION_OK])
            .rule(0x31, &[&[0x03, 0x7F, 0x31, 0x78]]);
        let mut uds = UdsClient::new(&mut iface, 0x7E0, 0x7E8);
        uds.diagnostic_session_control(0x03).await.unwrap();
        let started = tokio::time::Instant::now();
        let err = uds.routine_control(0x01, 0xFF00, &[]).await.unwrap_err();
        assert!(matches!(err, SterngateError::IsoTpTimeout));
        let waited = started.elapsed();
        assert!(waited >= Duration::from_millis(5400), "waited only {waited:?}");
        assert!(waited < Duration::from_millis(7000), "waited {waited:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_restored_after_pending() {
        let mut iface = ScriptedInterface::new()
            .rule(0x10, &[SESSION_OK])
            .rule_once(0x31, &[&[0x03, 0x7F, 0x31, 0x78], &[0x05, 0x71, 0x01, 0xFF, 0x00, 0x00]]);
        let mut uds = UdsClient::new(&mut iface, 0x7E0, 0x7E8);
        uds.diagnostic_session_control(0x03).await.unwrap();
        uds.routine_control(0x01, 0xFF00, &[]).await.unwrap();
        // A later request that never gets an answer times out at the normal 1500 ms.
        let started = tokio::time::Instant::now();
        assert!(matches!(uds.read_data_by_identifier(0x0100).await, Err(SterngateError::IsoTpTimeout)));
        assert!(started.elapsed() < Duration::from_millis(2000));
    }

    #[test]
    fn p2_star_widening_does_not_overflow() {
        assert_eq!(p2_star_from(&[0x50, 0x03, 0x00, 0x32, 0xFF, 0xFF]), Some(Duration::from_millis(655_350)));
        assert_eq!(p2_star_from(&[0x50, 0x03]), None);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sterngate-protocol uds:: 2>&1 | tail -20`
Expected: compile error `cannot find function p2_star_from`; after stubbing, `pending_response_waits_p2_star` fails with `IsoTpTimeout` (the 1500 ms channel timeout expires before the 4 s reply).

- [ ] **Step 3: Implement**

In `uds.rs`:

```rust
use std::time::Duration;

/// Extra wait beyond the ECU-announced P2* before giving up on a pending reply.
const P2_STAR_MARGIN: Duration = Duration::from_millis(500);
/// P2* used until a DiagnosticSessionControl reply announces the real value.
const DEFAULT_P2_STAR: Duration = Duration::from_millis(5000);

/// P2* (enhanced response timing) from a positive DiagnosticSessionControl
/// reply: bytes 4..6 hold the value in 10 ms units. Widened before multiplying
/// so 0xFFFF cannot overflow a u16.
pub(crate) fn p2_star_from(resp: &[u8]) -> Option<Duration> {
    let raw = u16::from_be_bytes([*resp.get(4)?, *resp.get(5)?]);
    Some(Duration::from_millis(u64::from(raw) * 10))
}

pub struct UdsClient<'a> {
    channel: IsoTpChannel<'a>,
    p2_star: Duration,
}
```

`new` sets `p2_star: DEFAULT_P2_STAR`. `diagnostic_session_control`:

```rust
    pub async fn diagnostic_session_control(&mut self, session_type: u8) -> Result<Vec<u8>> {
        let resp = self.send_request(0x10, &[session_type]).await?;
        if let Some(p2_star) = p2_star_from(&resp) {
            self.p2_star = p2_star;
        }
        Ok(resp)
    }
```

`send_request` becomes a thin wrapper that always restores the timeout:

```rust
    pub async fn send_request(&mut self, service: u8, payload: &[u8]) -> Result<Vec<u8>> {
        let saved = self.channel.timeout();
        let result = self.send_request_inner(service, payload).await;
        self.channel.set_timeout(saved);
        result
    }

    async fn send_request_inner(&mut self, service: u8, payload: &[u8]) -> Result<Vec<u8>> {
        let mut req = vec![service];
        req.extend_from_slice(payload);
        self.channel.send_payload(&req).await?;

        loop {
            let resp = self.channel.recv_payload().await?;
            let Some(&sid) = resp.first() else {
                return Err(SterngateError::IsoTpError("Empty UDS response received".into()));
            };

            if sid == 0x7F {
                let (Some(&rejected_service), Some(&nrc)) = (resp.get(1), resp.get(2)) else {
                    return Err(SterngateError::IsoTpError("Malformed Negative Response".into()));
                };
                if nrc == 0x78 {
                    // ResponsePending: the ECU asked for P2* before the real reply.
                    self.channel.set_timeout(self.p2_star + P2_STAR_MARGIN);
                    continue;
                }
                let desc = Self::lookup_nrc_description(nrc);
                return Err(SterngateError::UdsNegativeResponse { service: rejected_service, nrc, description: desc });
            }

            if sid != service.wrapping_add(0x40) {
                return Err(SterngateError::IsoTpError(format!(
                    "Unexpected response SID: expected 0x{:02X}, got 0x{:02X}",
                    service.wrapping_add(0x40),
                    sid
                )));
            }
            return Ok(resp);
        }
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p sterngate-protocol 2>&1 | tail -6` → all pass.

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result" | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
git add crates/sterngate-protocol/src/uds.rs
git commit -m "fix(protocol): honour P2* while an ECU reports ResponsePending

Erase and checksum routines pend for seconds; the fixed 1.5 s receive
timeout turned them into false aborts.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: Virtual ECU completes a flash and identifies itself in ASCII

**Files:**
- Modify: `crates/sterngate-hal/src/mock.rs`
- Test: `crates/sterngate-hal/src/lib.rs` (existing `mod tests`)

**Interfaces:**
- Produces: `VirtualCanInterface` answers `0x34` with `74 20 0F FF` (max block 4095), `0x36` with `76 <bsc>` (counter taken from the request), `0x37` with `77`, `0x28 xx` with `68 xx`, `0x85 xx` with `C5 xx`; `0x2E` echoes the DID from the First Frame; `3E 80` gets no reply and `3E 00` gets `7E 00`; a tester Flow Control (`0x3x`) gets no reply and instead releases queued Consecutive Frames; F192 answers the 10-char ASCII `0281012224` and F194 `1037372332` as multi-frame replies (FF, then CFs after the tester's FC).

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `crates/sterngate-hal/src/lib.rs` (the `recv_diag` helper already exists there):

```rust
    /// Send a multi-frame request: FF, then read the FC, then the CFs.
    async fn send_multi(sim: &mut VirtualCanInterface, frames: &[&[u8]]) {
        sim.send(CanFrame::new_standard(0x7E0, frames[0])).await.unwrap();
        let fc = recv_diag(sim).await;
        assert_eq!(fc.data[0] >> 4, 0x3, "expected Flow Control");
        for cf in &frames[1..] {
            sim.send(CanFrame::new_standard(0x7E0, cf)).await.unwrap();
        }
    }

    #[tokio::test]
    async fn test_virtual_can_answers_request_download() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();
        // 34 00 44 00 04 00 00 00 08 00 00 = 11 bytes -> FF + 1 CF
        send_multi(
            &mut sim,
            &[&[0x10, 0x0B, 0x34, 0x00, 0x44, 0x00, 0x04, 0x00], &[0x21, 0x00, 0x00, 0x08, 0x00, 0x00]],
        )
        .await;
        let resp = recv_diag(&mut sim).await;
        assert_eq!(&resp.data[..5], &[0x04, 0x74, 0x20, 0x0F, 0xFF]);
    }

    #[tokio::test]
    async fn test_virtual_can_echoes_transfer_data_counter() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();
        // 36 07 + 8 data bytes = 10 bytes -> FF + 1 CF
        send_multi(
            &mut sim,
            &[&[0x10, 0x0A, 0x36, 0x07, 0xAA, 0xBB, 0xCC, 0xDD], &[0x21, 0xEE, 0xFF, 0x11, 0x22]],
        )
        .await;
        let resp = recv_diag(&mut sim).await;
        assert_eq!(&resp.data[..3], &[0x02, 0x76, 0x07]);
    }

    #[tokio::test]
    async fn test_virtual_can_answers_transfer_exit_and_bus_control() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();
        sim.send(CanFrame::new_standard(0x7E0, &[0x01, 0x37])).await.unwrap();
        assert_eq!(&recv_diag(&mut sim).await.data[..2], &[0x01, 0x77]);
        sim.send(CanFrame::new_standard(0x7E0, &[0x03, 0x28, 0x01, 0x01])).await.unwrap();
        assert_eq!(&recv_diag(&mut sim).await.data[..3], &[0x02, 0x68, 0x01]);
        sim.send(CanFrame::new_standard(0x7E0, &[0x02, 0x85, 0x02])).await.unwrap();
        assert_eq!(&recv_diag(&mut sim).await.data[..3], &[0x02, 0xC5, 0x02]);
    }

    #[tokio::test]
    async fn test_virtual_can_suppresses_tester_present_response() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();
        sim.send(CanFrame::new_standard(0x7E0, &[0x02, 0x3E, 0x80])).await.unwrap();
        // Only background broadcast frames may arrive; no 0x7E8 diagnostic reply.
        let got = tokio::time::timeout(std::time::Duration::from_millis(150), recv_diag(&mut sim)).await;
        assert!(got.is_err(), "suppressed TesterPresent must not be answered");
        sim.send(CanFrame::new_standard(0x7E0, &[0x02, 0x3E, 0x00])).await.unwrap();
        assert_eq!(&recv_diag(&mut sim).await.data[..3], &[0x02, 0x7E, 0x00]);
    }

    #[tokio::test]
    async fn test_virtual_can_identification_is_multi_frame_ascii() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();
        sim.send(CanFrame::new_standard(0x7E0, &[0x03, 0x22, 0xF1, 0x92])).await.unwrap();
        let ff = recv_diag(&mut sim).await;
        assert_eq!(&ff.data[..5], &[0x10, 0x0D, 0x62, 0xF1, 0x92]);
        // Nothing more until the tester sends Flow Control.
        let early = tokio::time::timeout(std::time::Duration::from_millis(100), recv_diag(&mut sim)).await;
        assert!(early.is_err());
        sim.send(CanFrame::new_standard(0x7E0, &[0x30, 0x00, 0x05])).await.unwrap();
        let cf = recv_diag(&mut sim).await;
        assert_eq!(cf.data[0], 0x21);
        let mut text = ff.data[5..8].to_vec();
        text.extend_from_slice(&cf.data[1..8]);
        assert_eq!(text, b"0281012224");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sterngate-hal test_virtual_can_ 2>&1 | tail -20`
Expected: `answers_request_download` gets `6E 20 31`; `echoes_transfer_data_counter` gets `6E 20 31`; `transfer_exit_and_bus_control` gets `7F 37 11`; `suppresses_tester_present_response` gets `7E 80`; `identification_is_multi_frame_ascii` gets a single-frame BCD reply.

- [ ] **Step 3: Implement**

In `mock.rs`:

1. Add fields `pending_cfs: Arc<Mutex<VecDeque<CanFrame>>>` (import `std::collections::VecDeque`; `Mutex` is the tokio one already imported) and `last_multi_frame_bsc: Arc<AtomicU8>` (initialised to 0), plus `last_multi_frame_did: Arc<AtomicU16>` (`AtomicU16` from `std::sync::atomic`, initialised 0). Initialise all in `new()`.

2. Add a helper that builds a multi-frame reply and parks its CFs:

```rust
    /// Queue a multi-frame ISO-TP reply: returns the First Frame now and parks
    /// the Consecutive Frames until the tester's Flow Control arrives.
    fn multi_frame_reply(&self, resp_id: u16, payload: &[u8]) -> CanFrame {
        let len = payload.len().min(0x0FFF);
        let mut ff = vec![0x10 | u8::try_from(len >> 8).unwrap_or(0x0F), u8::try_from(len & 0xFF).unwrap_or(0xFF)];
        ff.extend_from_slice(&payload[..len.min(6)]);
        let mut sn = 1u8;
        let mut cfs = VecDeque::new();
        for chunk in payload[len.min(6)..len].chunks(7) {
            let mut cf = vec![0x20 | (sn & 0x0F)];
            cf.extend_from_slice(chunk);
            while cf.len() < 8 {
                cf.push(0xAA);
            }
            cfs.push_back(CanFrame::new_standard(resp_id, &cf));
            sn = sn.wrapping_add(1) & 0x0F;
        }
        if let Ok(mut q) = self.pending_cfs.try_lock() {
            *q = cfs;
        }
        CanFrame::new_standard(resp_id, &ff)
    }
```

3. In `send()`, before `generate_telemetry_frame`: if `frame.data.first().is_some_and(|b| b >> 4 == 0x3)` (tester Flow Control), drain `pending_cfs` into `tx_queue` in order and return `Ok(())` without generating a reply.

4. In the First-Frame branch of `generate_telemetry_frame`, after storing `last_multi_frame_sid`, also store: `if payload.len() >= 5 && payload[2] == 0x2E { last_multi_frame_did = u16::from_be_bytes([payload[3], payload[4]]) }` and `if payload.len() >= 4 && payload[2] == 0x36 { last_multi_frame_bsc = payload[3] }`.

5. In the CF-completion `match sid` (Task 1 of Phase 0a added it), add arms before `_`:

```rust
                0x34 => vec![0x04, 0x74, 0x20, 0x0F, 0xFF],
                0x36 => vec![0x02, 0x76, self.last_multi_frame_bsc.load(Ordering::Relaxed)],
                0x2E => {
                    let did = self.last_multi_frame_did.load(Ordering::Relaxed).to_be_bytes();
                    vec![0x03, 0x6E, did[0], did[1]]
                }
```

and change the `_` arm to `vec![0x03, 0x7F, sid, 0x11]` (ServiceNotSupported for anything else multi-frame).

6. Single-frame arms in the `match service`:

```rust
            0x37 => Some(CanFrame::new_standard(resp_id as u16, &[0x01, 0x77])),
            0x28 => {
                let sub = payload.get(2).copied().unwrap_or(0x01);
                Some(CanFrame::new_standard(resp_id as u16, &[0x02, 0x68, sub]))
            }
            0x85 => {
                let sub = payload.get(2).copied().unwrap_or(0x02);
                Some(CanFrame::new_standard(resp_id as u16, &[0x02, 0xC5, sub]))
            }
            0x34 => Some(CanFrame::new_standard(resp_id as u16, &[0x04, 0x74, 0x20, 0x0F, 0xFF])),
```

and replace the TesterPresent arm with:

```rust
            0x3E => {
                if payload.get(2).is_some_and(|sub| sub & 0x80 != 0) {
                    None
                } else {
                    Some(CanFrame::new_standard(resp_id as u16, &[0x02, 0x7E, 0x00]))
                }
            }
```

7. Identification DIDs: replace the `0xF192` and `0xF194` arms with multi-frame ASCII replies:

```rust
                    0xF192 => Some(self.multi_frame_reply(resp_id as u16, b"\x62\xF1\x920281012224")),
                    0xF194 => Some(self.multi_frame_reply(resp_id as u16, b"\x62\xF1\x941037372332")),
```

Run `grep -rn "0x02, 0x81, 0x01, 0x22\|0x10, 0x37, 0x37, 0x23\|F192\|F194" crates/ --include=*.rs` and fix any test that asserted the old 4-byte BCD replies (expected: none outside mock.rs; `BusDiscoverer` reads F187/F190 and tolerates both).

- [ ] **Step 4: Run the workspace tests**

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | head` → all pass (protocol `test_uds_client_read_did`, discovery and scanner tests included).

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result" | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
git add crates/sterngate-hal/src/mock.rs crates/sterngate-hal/src/lib.rs
git commit -m "test(hal): let the virtual ECU complete a programming sequence and answer identification in ASCII

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: UDS helpers — suppressed TesterPresent, `S3KeepAlive`, reply parsers

**Files:**
- Modify: `crates/sterngate-protocol/src/uds.rs`
- Test: `crates/sterngate-protocol/src/uds.rs` `mod tests`

**Interfaces:**
- Produces: `UdsClient::tester_present_suppressed(&mut self) -> Result<()>` (sends `3E 80`, waits for nothing); `tester_present(true)` delegates to it; `send_request` discards a stray `7E xx` positive reply when the service is not `0x3E`.
- Produces: `pub struct S3KeepAlive` with `new()`, `touch()`, `async fn tick(&mut self, uds: &mut UdsClient<'_>) -> Result<()>` (sends the suppressed TesterPresent when ≥ `S3_KEEPALIVE_INTERVAL` (1500 ms, `tokio::time::Instant`) elapsed since the last touch, then touches); `pub const S3_KEEPALIVE_INTERVAL: Duration`.
- Produces: `pub fn parse_routine_status(resp: &[u8], sub: u8, routine_id: u16) -> Result<u8>` (requires `71 <sub> <id_hi> <id_lo> <status>`; a missing status byte is an error); `pub fn parse_request_download(resp: &[u8]) -> Result<usize>` (`74 <n<<4> <n bytes BE>` with `1 ≤ n ≤ 4` and result ≥ 3).

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `uds.rs`:

```rust
    #[tokio::test(start_paused = true)]
    async fn tester_present_suppressed_does_not_wait() {
        let mut iface = ScriptedInterface::new(); // never answers
        let mut uds = UdsClient::new(&mut iface, 0x7E0, 0x7E8);
        let started = tokio::time::Instant::now();
        uds.tester_present_suppressed().await.unwrap();
        assert!(started.elapsed() < Duration::from_millis(10));
        assert_eq!(iface.sent_frames()[0].data[..3], [0x02, 0x3E, 0x80]);
    }

    #[tokio::test(start_paused = true)]
    async fn stray_7e_response_is_ignored() {
        let mut iface = ScriptedInterface::new()
            .rule(0x36, &[&[0x02, 0x7E, 0x80], &[0x02, 0x76, 0x01]]);
        let mut uds = UdsClient::new(&mut iface, 0x7E0, 0x7E8);
        let resp = uds.send_request(0x36, &[0x01, 0xAA]).await.unwrap();
        assert_eq!(resp, vec![0x76, 0x01]);
    }

    #[tokio::test(start_paused = true)]
    async fn keepalive_sends_only_when_idle() {
        let mut iface = ScriptedInterface::new();
        let mut uds = UdsClient::new(&mut iface, 0x7E0, 0x7E8);
        let mut ka = S3KeepAlive::new();
        ka.tick(&mut uds).await.unwrap(); // fresh: nothing sent
        tokio::time::sleep(Duration::from_millis(1600)).await;
        ka.tick(&mut uds).await.unwrap(); // idle > 1500 ms: one 3E 80
        ka.tick(&mut uds).await.unwrap(); // just touched: nothing
        let sent = iface.sent_frames();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].data[..3], [0x02, 0x3E, 0x80]);
    }

    #[test]
    fn routine_status_parser_is_strict() {
        assert_eq!(parse_routine_status(&[0x71, 0x01, 0xFF, 0x00, 0x00], 0x01, 0xFF00).unwrap(), 0x00);
        assert_eq!(parse_routine_status(&[0x71, 0x01, 0x02, 0x02, 0x07], 0x01, 0x0202).unwrap(), 0x07);
        assert!(parse_routine_status(&[0x71, 0x01, 0xFF, 0x00], 0x01, 0xFF00).is_err()); // no status byte
        assert!(parse_routine_status(&[0x71, 0x01, 0xFF, 0x01, 0x00], 0x01, 0xFF00).is_err()); // wrong id
        assert!(parse_routine_status(&[0x71, 0x03, 0xFF, 0x00, 0x00], 0x01, 0xFF00).is_err()); // wrong sub
        assert!(parse_routine_status(&[], 0x01, 0xFF00).is_err());
    }

    #[test]
    fn request_download_parser_is_strict() {
        assert_eq!(parse_request_download(&[0x74, 0x20, 0x0F, 0xFF]).unwrap(), 4095);
        assert_eq!(parse_request_download(&[0x74, 0x10, 0xFF]).unwrap(), 255);
        assert!(parse_request_download(&[0x74, 0x10, 0x02]).is_err()); // < 3
        assert!(parse_request_download(&[0x74, 0x50, 0, 0, 0, 0, 0]).is_err()); // n > 4
        assert!(parse_request_download(&[0x74, 0x20, 0x0F]).is_err()); // short
        assert!(parse_request_download(&[0x74]).is_err());
        assert!(parse_request_download(&[]).is_err());
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sterngate-protocol uds:: 2>&1 | tail -15` → compile errors for `tester_present_suppressed`, `S3KeepAlive`, `parse_routine_status`, `parse_request_download`.

- [ ] **Step 3: Implement**

In `uds.rs`:

```rust
/// TesterPresent must go out at least every 2 s in an extended or programming
/// session; sending at 1.5 s leaves margin for one slow exchange.
pub const S3_KEEPALIVE_INTERVAL: Duration = Duration::from_millis(1500);

/// Tracks the last exchange with the ECU and sends a suppressed TesterPresent
/// when the S3 timer is about to expire. Only ever called between complete
/// request/response exchanges, so it cannot interleave with an ISO-TP transfer.
pub struct S3KeepAlive {
    last_activity: tokio::time::Instant,
}

impl Default for S3KeepAlive {
    fn default() -> Self {
        Self::new()
    }
}

impl S3KeepAlive {
    pub fn new() -> Self {
        Self { last_activity: tokio::time::Instant::now() }
    }

    /// Record that a request/response exchange just completed.
    pub fn touch(&mut self) {
        self.last_activity = tokio::time::Instant::now();
    }

    /// Send `3E 80` if the session has been idle for the keep-alive interval.
    pub async fn tick(&mut self, uds: &mut UdsClient<'_>) -> Result<()> {
        if self.last_activity.elapsed() >= S3_KEEPALIVE_INTERVAL {
            uds.tester_present_suppressed().await?;
            self.touch();
        }
        Ok(())
    }
}

/// Positive RoutineControl reply `71 <sub> <id> <status>`: returns the first
/// routineStatusRecord byte. A reply without a status byte is an error, never
/// an implicit success.
pub fn parse_routine_status(resp: &[u8], sub: u8, routine_id: u16) -> Result<u8> {
    let id = routine_id.to_be_bytes();
    match resp {
        [0x71, s, hi, lo, status, ..] if *s == sub && *hi == id[0] && *lo == id[1] => Ok(*status),
        _ => Err(SterngateError::ProtocolError(format!(
            "RoutineControl 0x{routine_id:04X}: unexpected reply {resp:02X?} (need 71 {sub:02X} {:02X} {:02X} <status>)",
            id[0], id[1]
        ))),
    }
}

/// Positive RequestDownload reply: `74 <lengthFormat> <maxNumberOfBlockLength>`.
/// Returns the ECU's maximum block length (including the SID and counter bytes).
pub fn parse_request_download(resp: &[u8]) -> Result<usize> {
    let err = |what: &str| SterngateError::ProtocolError(format!("RequestDownload: {what} in reply {resp:02X?}"));
    if resp.first() != Some(&0x74) {
        return Err(err("missing positive SID"));
    }
    let n = usize::from(resp.get(1).ok_or_else(|| err("missing length format"))? >> 4);
    if !(1..=4).contains(&n) {
        return Err(err("lengthFormatIdentifier out of range"));
    }
    let bytes = resp.get(2..2 + n).ok_or_else(|| err("truncated maxNumberOfBlockLength"))?;
    let max = bytes.iter().fold(0usize, |acc, b| (acc << 8) | usize::from(*b));
    if max < 3 {
        return Err(err("maxNumberOfBlockLength leaves no room for data"));
    }
    Ok(max)
}
```

In `impl UdsClient`: 

```rust
    /// TesterPresent with positive-response suppression: fire and forget. A
    /// conformant ECU sends nothing back, so waiting would only time out.
    pub async fn tester_present_suppressed(&mut self) -> Result<()> {
        self.channel.send_payload(&[0x3E, 0x80]).await
    }

    /// TesterPresent (0x3E). With `suppress_pos_rsp` the call returns as soon as
    /// the frame is sent; otherwise it waits for `7E 00`.
    pub async fn tester_present(&mut self, suppress_pos_rsp: bool) -> Result<()> {
        if suppress_pos_rsp {
            return self.tester_present_suppressed().await;
        }
        self.send_request(0x3E, &[0x00]).await?;
        Ok(())
    }
```

In `send_request_inner`, before the `if sid != service.wrapping_add(0x40)` check:

```rust
            if sid == 0x7E && service != 0x3E {
                // A non-conformant ECU answered a suppressed TesterPresent; it is
                // not the reply we are waiting for.
                continue;
            }
```

Export from `crates/sterngate-protocol/src/lib.rs`: `pub use uds::{parse_request_download, parse_routine_status, S3KeepAlive, UdsClient, S3_KEEPALIVE_INTERVAL};`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p sterngate-protocol 2>&1 | tail -6` → all pass.

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result" | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
git add crates/sterngate-protocol/src/uds.rs crates/sterngate-protocol/src/lib.rs
git commit -m "feat(protocol): suppressed TesterPresent keep-alive and strict RoutineControl/RequestDownload parsers

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: The programming sequence fails closed

**Files:**
- Modify: `crates/sterngate-protocol/src/flasher.rs` (`execute_flash`, new `run_programming_sequence`, constants, preflight length check)
- Test: `crates/sterngate-protocol/src/flasher.rs` (new `#[cfg(test)] mod tests`), `crates/sterngate-mcp/src/lib.rs` (existing full-flash test stays green)

**Interfaces:**
- Consumes: `S3KeepAlive`, `parse_routine_status`, `parse_request_download`, `UdsClient::tester_present_suppressed` (Task 4); the mock answers from Task 3; `IsoTpChannel` from Tasks 1–2.
- Produces: `pub const FLASH_TX_ID: u32 = 0x7E0; pub const FLASH_RX_ID: u32 = 0x7E8; pub const ERASE_ROUTINE_ID: u16 = 0xFF00; pub const CHECKSUM_ROUTINE_ID: u16 = 0x0202; pub const CHECKSUM_STATUS_OK: u8 = 0x00; pub const ISOTP_MAX_PAYLOAD: usize = 4095;`
- Produces: `execute_flash` holds the interface lock for preflight and the whole sequence; every step propagates errors; on error the state is `Failed` (not `Locked`) with `error_message` starting `"Flash aborted before erase; ECU untouched."` or `"FLASH FAILED AFTER ERASE - ECU is in bootloader with incomplete application. Keep ignition ON, do not disconnect."`; a failed ECUReset after a verified checksum yields `Completed` with `error_message: Some("ECU did not acknowledge reset; cycle ignition manually")`.
- Produces: preflight check 5: `usize::try_from(manifest.flash_length) == Ok(rom_data.len())`, otherwise `passed = false`.

- [ ] **Step 1: Write the failing tests**

Append to `flasher.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ScriptedInterface;
    use sterngate_hal::VirtualCanInterface;
    use std::time::Duration;

    const SESSION_EXT: &[u8] = &[0x06, 0x50, 0x03, 0x00, 0x32, 0x01, 0xF4];
    const SESSION_PROG: &[u8] = &[0x06, 0x50, 0x02, 0x00, 0x32, 0x01, 0xF4];
    const SEED: &[u8] = &[0x06, 0x67, 0x0B, 0x12, 0x34, 0x56, 0x78];
    const KEY_OK: &[u8] = &[0x02, 0x67, 0x0C];
    const COMM_OFF: &[u8] = &[0x02, 0x68, 0x01];
    const DTC_OFF: &[u8] = &[0x02, 0xC5, 0x02];
    const ERASE_OK: &[u8] = &[0x05, 0x71, 0x01, 0xFF, 0x00, 0x00];
    const DOWNLOAD_OK: &[u8] = &[0x04, 0x74, 0x20, 0x0F, 0xFF];
    const EXIT_OK: &[u8] = &[0x01, 0x77];
    const CHECKSUM_OK: &[u8] = &[0x05, 0x71, 0x01, 0x02, 0x02, 0x00];
    const RESET_OK: &[u8] = &[0x02, 0x51, 0x01];
    // F192 -> "0281012224" as FF + CF (the test double delivers CFs after the tester's FC)
    const F192_FF: &[u8] = &[0x10, 0x0D, 0x62, 0xF1, 0x92, b'0', b'2', b'8'];
    const F192_CF: &[u8] = &[0x21, b'1', b'0', b'1', b'2', b'2', b'2', b'4'];

    fn manifest(rom: &[u8]) -> FlashPackageManifest {
        let mut hasher = Sha256::new();
        hasher.update(rom);
        FlashPackageManifest {
            target_module: "EDC16".into(),
            expected_hw_id: "0281012224".into(),
            expected_sw_id: "1037372332".into(),
            sha256_checksum: format!("{:x}", hasher.finalize()),
            crc32_checksum: crc32fast::hash(rom),
            flash_start_address: 0x0004_0000,
            flash_length: u32::try_from(rom.len()).unwrap(),
            block_size: 256,
        }
    }

    /// A scripted ECU that answers every step positively; tests override single steps.
    fn happy_ecu() -> ScriptedInterface {
        ScriptedInterface::new()
            .rule(0x22, &[F192_FF, F192_CF])
            .rule_once(0x10, &[SESSION_EXT])
            .rule(0x10, &[SESSION_PROG])
            .rule_once(0x27, &[SEED])
            .rule(0x27, &[KEY_OK])
            .rule(0x28, &[COMM_OFF])
            .rule(0x85, &[DTC_OFF])
            .rule_once(0x31, &[ERASE_OK])
            .rule(0x31, &[CHECKSUM_OK])
            .rule(0x34, &[DOWNLOAD_OK])
            .rule(0x36, &[&[0x02, 0x76, 0x01]])
            .rule(0x37, &[EXIT_OK])
            .rule(0x11, &[RESET_OK])
    }

    async fn run(iface: ScriptedInterface, rom: Vec<u8>) -> (FlashingWorker, Result<()>, Arc<Mutex<Box<dyn VehicleInterface>>>) {
        let flasher = FlashingWorker::new();
        let m = manifest(&rom);
        let shared: Arc<Mutex<Box<dyn VehicleInterface>>> = Arc::new(Mutex::new(Box::new(iface)));
        let res = flasher.execute_flash(m, rom, 13.5, shared.clone()).await;
        (flasher, res, shared)
    }

    #[tokio::test(start_paused = true)]
    async fn full_success_path_completes_in_order() {
        let iface = happy_ecu();
        let log = iface.sent_handle();
        let rom = vec![0x5A; 300]; // 2 blocks at 256
        let (flasher, res, _) = run(iface, rom).await;
        res.unwrap();
        assert_eq!(flasher.current_state().await, FlashState::Completed);
        let sids: Vec<u8> = log.lock().unwrap().iter().filter_map(|f| crate::test_support::request_sid(&f.data)).collect();
        // 22 (preflight F192), 10 03, 27 0B, 27 0C, 28, 85, 10 02, 31 FF00, 34, 36, 36, 37, 31 0202, 11
        assert_eq!(sids, vec![0x22, 0x10, 0x27, 0x27, 0x28, 0x85, 0x10, 0x31, 0x34, 0x36, 0x36, 0x37, 0x31, 0x11]);
        let prog = flasher.subscribe().borrow().clone();
        assert_eq!(prog.bytes_written, 300);
        assert!(prog.error_message.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn security_access_nrc_aborts_before_erase() {
        let iface = happy_ecu().rule_once(0x27, &[&[0x03, 0x7F, 0x27, 0x35]]);
        // rule_once is appended after the happy seed rule; make the NRC win by
        // building a fresh script instead:
        let iface = ScriptedInterface::new()
            .rule(0x22, &[F192_FF, F192_CF])
            .rule(0x10, &[SESSION_EXT])
            .rule(0x27, &[&[0x03, 0x7F, 0x27, 0x35]]);
        let log = iface.sent_handle();
        let (flasher, res, _) = run(iface, vec![0x5A; 300]).await;
        assert!(matches!(res, Err(SterngateError::UdsNegativeResponse { service: 0x27, nrc: 0x35, .. })));
        assert_eq!(flasher.current_state().await, FlashState::Failed);
        assert!(!flasher.is_locked().await);
        let prog = flasher.subscribe().borrow().clone();
        assert!(prog.error_message.as_deref().unwrap().starts_with("Flash aborted before erase; ECU untouched."));
        let sids: Vec<u8> = log.lock().unwrap().iter().filter_map(|f| crate::test_support::request_sid(&f.data)).collect();
        assert!(!sids.contains(&0x31), "no erase after a security-access NRC");
    }

    #[tokio::test(start_paused = true)]
    async fn request_download_nrc_aborts_after_erase() {
        let iface = ScriptedInterface::new()
            .rule(0x22, &[F192_FF, F192_CF])
            .rule_once(0x10, &[SESSION_EXT])
            .rule(0x10, &[SESSION_PROG])
            .rule_once(0x27, &[SEED])
            .rule(0x27, &[KEY_OK])
            .rule(0x28, &[COMM_OFF])
            .rule(0x85, &[DTC_OFF])
            .rule(0x31, &[ERASE_OK])
            .rule(0x34, &[&[0x03, 0x7F, 0x34, 0x70]]);
        let log = iface.sent_handle();
        let (flasher, res, _) = run(iface, vec![0x5A; 300]).await;
        assert!(res.is_err());
        assert_eq!(flasher.current_state().await, FlashState::Failed);
        let prog = flasher.subscribe().borrow().clone();
        assert!(prog.error_message.as_deref().unwrap().starts_with("FLASH FAILED AFTER ERASE"));
        let sids: Vec<u8> = log.lock().unwrap().iter().filter_map(|f| crate::test_support::request_sid(&f.data)).collect();
        assert!(!sids.contains(&0x36));
    }

    #[tokio::test(start_paused = true)]
    async fn block_counter_echo_mismatch_aborts() {
        let iface = happy_ecu().rule_once(0x36, &[&[0x02, 0x76, 0x05]]);
        // rule_once must precede the repeating rule to win: rebuild explicitly.
        let iface = ScriptedInterface::new()
            .rule(0x22, &[F192_FF, F192_CF])
            .rule_once(0x10, &[SESSION_EXT])
            .rule(0x10, &[SESSION_PROG])
            .rule_once(0x27, &[SEED])
            .rule(0x27, &[KEY_OK])
            .rule(0x28, &[COMM_OFF])
            .rule(0x85, &[DTC_OFF])
            .rule_once(0x31, &[ERASE_OK])
            .rule(0x34, &[DOWNLOAD_OK])
            .rule(0x36, &[&[0x02, 0x76, 0x05]]);
        let log = iface.sent_handle();
        let (_, res, _) = run(iface, vec![0x5A; 300]).await;
        assert!(matches!(res, Err(SterngateError::ProtocolError(_))));
        let sids: Vec<u8> = log.lock().unwrap().iter().filter_map(|f| crate::test_support::request_sid(&f.data)).collect();
        assert_eq!(sids.iter().filter(|s| **s == 0x36).count(), 1);
        assert!(!sids.contains(&0x37));
    }

    #[tokio::test(start_paused = true)]
    async fn checksum_status_nonzero_fails_before_reset() {
        let iface = ScriptedInterface::new()
            .rule(0x22, &[F192_FF, F192_CF])
            .rule_once(0x10, &[SESSION_EXT])
            .rule(0x10, &[SESSION_PROG])
            .rule_once(0x27, &[SEED])
            .rule(0x27, &[KEY_OK])
            .rule(0x28, &[COMM_OFF])
            .rule(0x85, &[DTC_OFF])
            .rule_once(0x31, &[ERASE_OK])
            .rule(0x31, &[&[0x05, 0x71, 0x01, 0x02, 0x02, 0x01]])
            .rule(0x34, &[DOWNLOAD_OK])
            .rule(0x36, &[&[0x02, 0x76, 0x01]])
            .rule(0x37, &[EXIT_OK])
            .rule(0x11, &[RESET_OK]);
        let log = iface.sent_handle();
        let (flasher, res, _) = run(iface, vec![0x5A; 100]).await;
        assert!(matches!(res, Err(SterngateError::ChecksumMismatch { .. })));
        assert_eq!(flasher.current_state().await, FlashState::Failed);
        let sids: Vec<u8> = log.lock().unwrap().iter().filter_map(|f| crate::test_support::request_sid(&f.data)).collect();
        assert!(!sids.contains(&0x11), "no reset after a failed checksum");
    }

    #[tokio::test(start_paused = true)]
    async fn checksum_missing_status_byte_fails_closed() {
        let iface = ScriptedInterface::new()
            .rule(0x22, &[F192_FF, F192_CF])
            .rule_once(0x10, &[SESSION_EXT])
            .rule(0x10, &[SESSION_PROG])
            .rule_once(0x27, &[SEED])
            .rule(0x27, &[KEY_OK])
            .rule(0x28, &[COMM_OFF])
            .rule(0x85, &[DTC_OFF])
            .rule_once(0x31, &[ERASE_OK])
            .rule(0x31, &[&[0x04, 0x71, 0x01, 0x02, 0x02]])
            .rule(0x34, &[DOWNLOAD_OK])
            .rule(0x36, &[&[0x02, 0x76, 0x01]])
            .rule(0x37, &[EXIT_OK]);
        let (_, res, _) = run(iface, vec![0x5A; 100]).await;
        assert!(res.is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn reset_failure_after_verified_checksum_completes_with_warning() {
        let iface = ScriptedInterface::new()
            .rule(0x22, &[F192_FF, F192_CF])
            .rule_once(0x10, &[SESSION_EXT])
            .rule(0x10, &[SESSION_PROG])
            .rule_once(0x27, &[SEED])
            .rule(0x27, &[KEY_OK])
            .rule(0x28, &[COMM_OFF])
            .rule(0x85, &[DTC_OFF])
            .rule_once(0x31, &[ERASE_OK])
            .rule(0x31, &[CHECKSUM_OK])
            .rule(0x34, &[DOWNLOAD_OK])
            .rule(0x36, &[&[0x02, 0x76, 0x01]])
            .rule(0x37, &[EXIT_OK])
            .rule(0x11, &[&[0x03, 0x7F, 0x11, 0x22]]);
        let (flasher, res, _) = run(iface, vec![0x5A; 100]).await;
        res.unwrap();
        assert_eq!(flasher.current_state().await, FlashState::Completed);
        let prog = flasher.subscribe().borrow().clone();
        assert_eq!(prog.error_message.as_deref(), Some("ECU did not acknowledge reset; cycle ignition manually"));
    }

    #[tokio::test(start_paused = true)]
    async fn request_download_uses_manifest_and_clamps_block_size() {
        // ECU max block 258 -> 256 data bytes per 0x36 even though manifest says 4096
        let iface = ScriptedInterface::new()
            .rule(0x22, &[F192_FF, F192_CF])
            .rule_once(0x10, &[SESSION_EXT])
            .rule(0x10, &[SESSION_PROG])
            .rule_once(0x27, &[SEED])
            .rule(0x27, &[KEY_OK])
            .rule(0x28, &[COMM_OFF])
            .rule(0x85, &[DTC_OFF])
            .rule_once(0x31, &[ERASE_OK])
            .rule(0x31, &[CHECKSUM_OK])
            .rule(0x34, &[&[0x04, 0x74, 0x20, 0x01, 0x02]])
            .rule(0x36, &[&[0x02, 0x76, 0x01]])
            .rule(0x37, &[EXIT_OK])
            .rule(0x11, &[RESET_OK]);
        let log = iface.sent_handle();
        let rom = vec![0x5A; 600];
        let flasher = FlashingWorker::new();
        let mut m = manifest(&rom);
        m.flash_start_address = 0x0008_0000;
        m.block_size = 4096;
        let shared: Arc<Mutex<Box<dyn VehicleInterface>>> = Arc::new(Mutex::new(Box::new(iface)));
        flasher.execute_flash(m, rom, 13.5, shared).await.unwrap();
        let frames = log.lock().unwrap().clone();
        // 0x34 First Frame carries: 34 00 44 00 08 00 00 (address) then CF: 00 00 02 58 (length 600)
        let ff = frames.iter().find(|f| f.data.first().map(|b| b >> 4) == Some(1) && f.data.get(2) == Some(&0x34)).unwrap();
        assert_eq!(&ff.data[2..8], &[0x34, 0x00, 0x44, 0x00, 0x08, 0x00]);
        let prog = flasher.subscribe().borrow().clone();
        assert_eq!(prog.total_blocks, 3, "600 bytes at 256 per block");
    }

    #[tokio::test(start_paused = true)]
    async fn keepalive_sent_when_ecu_is_slow() {
        let iface = ScriptedInterface::new()
            .rule(0x22, &[F192_FF, F192_CF])
            .rule_once(0x10, &[SESSION_EXT])
            .rule(0x10, &[SESSION_PROG])
            .rule_once(0x27, &[SEED])
            .rule(0x27, &[KEY_OK])
            .rule(0x28, &[COMM_OFF])
            .rule(0x85, &[DTC_OFF])
            .rule_once(0x31, &[ERASE_OK])
            .rule(0x31, &[CHECKSUM_OK])
            .rule(0x34, &[DOWNLOAD_OK])
            .rule_delayed(0x36, Duration::from_millis(1600), &[&[0x02, 0x76, 0x01]])
            .rule(0x37, &[EXIT_OK])
            .rule(0x11, &[RESET_OK]);
        let log = iface.sent_handle();
        let (_, res, _) = run(iface, vec![0x5A; 600]).await;
        res.unwrap();
        let frames = log.lock().unwrap().clone();
        let tp: Vec<usize> = frames.iter().enumerate().filter(|(_, f)| f.data.get(1) == Some(&0x3E)).map(|(i, _)| i).collect();
        assert!(!tp.is_empty(), "a 3E 80 keep-alive must be sent between slow blocks");
        // Never between a First Frame and the last Consecutive Frame of one request.
        for i in tp {
            let prev = &frames[i - 1].data;
            assert_ne!(prev.first().map(|b| b >> 4), Some(1), "keep-alive after a First Frame");
        }
    }

    #[tokio::test]
    async fn preflight_rejects_flash_length_mismatch() {
        let mut iface = happy_ecu();
        let rom = vec![0x5A; 100];
        let mut m = manifest(&rom);
        m.flash_length = 101;
        let report = FlashingWorker::new().run_preflight_checks(&m, &rom, 13.5, &mut iface).await.unwrap();
        assert!(!report.passed);
        assert!(report.details.iter().any(|d| d.contains("flash_length")));
    }

    #[tokio::test]
    async fn execute_flash_on_virtual_can_completes() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();
        let rom = vec![0x5A; 1000];
        let m = manifest(&rom);
        let shared: Arc<Mutex<Box<dyn VehicleInterface>>> = Arc::new(Mutex::new(Box::new(sim)));
        let flasher = FlashingWorker::new();
        flasher.execute_flash(m, rom, 13.5, shared).await.unwrap();
        assert_eq!(flasher.current_state().await, FlashState::Completed);
        assert_eq!(flasher.subscribe().borrow().bytes_written, 1000);
    }
}
```

Remove the two shadowed `happy_ecu()` lines in `security_access_nrc_aborts_before_erase` / `block_counter_echo_mismatch_aborts` before committing (they are there only to show why a fresh script is built: `ScriptedInterface` rules match first-registered-first).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sterngate-protocol flasher:: 2>&1 | tail -30`
Expected: the sequence tests fail (today every step is ignored, the state ends `Completed`, and `FLASH_TX_ID`/constants are missing → compile errors first). `execute_flash_on_virtual_can_completes` passes only because failures are swallowed; it stays green after the change because Task 3's mock now answers every step.

- [ ] **Step 3: Implement**

In `flasher.rs` add the constants and a progress tracker:

```rust
use crate::uds::{parse_request_download, parse_routine_status, S3KeepAlive};
use std::time::Duration;

pub const FLASH_TX_ID: u32 = 0x7E0;
pub const FLASH_RX_ID: u32 = 0x7E8;
pub const ERASE_ROUTINE_ID: u16 = 0xFF00;
/// Bosch EDC16 memory-check routine as used by this project's virtual ECU;
/// not yet verified against real firmware (spec D8).
pub const CHECKSUM_ROUTINE_ID: u16 = 0x0202;
pub const CHECKSUM_STATUS_OK: u8 = 0x00;
pub const ISOTP_MAX_PAYLOAD: usize = 4095;

/// Where the sequence is, for the failure message and the progress feed.
#[derive(Default)]
struct SequenceProgress {
    /// Set immediately before the erase request goes out: from here on a
    /// failure leaves the ECU with an incomplete application.
    post_erase: bool,
    current_block: usize,
    total_blocks: usize,
    bytes_written: usize,
    /// Set when the ECU verified the checksum but did not acknowledge the reset.
    reset_warning: Option<String>,
}
```

Preflight (`run_preflight_checks`), after the CRC32 check and before the hardware check, add check 5:

```rust
        // 5. The download request is built from the manifest, so its length must
        // describe the bytes that will actually be streamed.
        let length_ok = usize::try_from(manifest.flash_length).is_ok_and(|l| l == rom_data.len());
        if length_ok {
            details.push(format!("flash_length matches ROM size ({} bytes)", rom_data.len()));
        } else {
            details.push(format!(
                "flash_length mismatch: manifest says {} bytes, ROM is {} bytes",
                manifest.flash_length,
                rom_data.len()
            ));
        }
```

and include `length_ok` in `passed` (`voltage_ok && sha256_ok && crc32_ok && hw_match && length_ok`). Task 6 rewrites the hardware check; in this task keep it as is but use `FLASH_TX_ID`/`FLASH_RX_ID` for the `UdsClient::new` call.

Replace `execute_flash` from the pre-flight block to the end of the function with:

```rust
        let total_bytes = rom_data.len();
        let mut iface_guard = interface.lock().await;

        // Pre-flight check
        let report = self
            .run_preflight_checks(&manifest, &rom_data, battery_voltage, iface_guard.as_mut())
            .await?;
        if !report.passed {
            let err_msg = format!("Pre-flight check failed: {:?}", report.details);
            self.update_progress(FlashState::Failed, 0, 0, 0, 0, total_bytes, &err_msg, Some(err_msg.clone()));
            *self.current_state.lock().await = FlashState::Failed;
            return Err(SterngateError::PreFlightCheckFailed(err_msg));
        }
        self.update_progress(FlashState::Locked, 5, 0, 0, 0, total_bytes, "Pre-flight checks passed. API Lockout engaged.", None);

        // The interface stays locked for the whole sequence: nothing else may
        // put a frame on the bus between the session request and the reset.
        let mut uds = UdsClient::new(iface_guard.as_mut(), FLASH_TX_ID, FLASH_RX_ID);
        let mut ka = S3KeepAlive::new();
        let mut progress = SequenceProgress::default();
        let result = self
            .run_programming_sequence(&manifest, &rom_data, &mut uds, &mut ka, &mut progress)
            .await;
        drop(uds);
        drop(iface_guard);

        match result {
            Ok(()) => {
                self.update_progress(
                    FlashState::Completed,
                    100,
                    progress.total_blocks,
                    progress.total_blocks,
                    progress.bytes_written,
                    total_bytes,
                    "Flash completed successfully! ECU checksum verified.",
                    progress.reset_warning.clone(),
                );
                *self.current_state.lock().await = FlashState::Completed;
                Ok(())
            }
            Err(e) => {
                let msg = if progress.post_erase {
                    format!("FLASH FAILED AFTER ERASE - ECU is in bootloader with incomplete application. Keep ignition ON, do not disconnect. {e}")
                } else {
                    format!("Flash aborted before erase; ECU untouched. {e}")
                };
                self.update_progress(
                    FlashState::Failed,
                    0,
                    progress.current_block,
                    progress.total_blocks,
                    progress.bytes_written,
                    total_bytes,
                    &msg,
                    Some(msg.clone()),
                );
                *self.current_state.lock().await = FlashState::Failed;
                Err(e)
            }
        }
    }

    async fn run_programming_sequence(
        &self,
        manifest: &FlashPackageManifest,
        rom_data: &[u8],
        uds: &mut UdsClient<'_>,
        ka: &mut S3KeepAlive,
        progress: &mut SequenceProgress,
    ) -> Result<()> {
        let total_bytes = rom_data.len();

        // Step 1: Extended Diagnostic Session (0x10 03)
        self.update_progress(FlashState::SessionExtended, 10, 0, 0, 0, total_bytes, "Requesting Extended Diagnostic Session (0x10 03)...", None);
        let resp = uds.diagnostic_session_control(0x03).await?;
        if resp.get(1) != Some(&0x03) {
            return Err(SterngateError::ProtocolError(format!("Extended session not confirmed: {resp:02X?}")));
        }
        ka.touch();

        // Step 2: Security Access (0x27 0B)
        self.update_progress(FlashState::SecurityUnlocked, 20, 0, 0, 0, total_bytes, "Performing Bootloader Security Access (Level 0x0B)...", None);
        ka.tick(uds).await?;
        uds.security_access(0x0B, &DaimlerSolver).await?;
        ka.touch();

        // Step 3: Silence bus (0x28 01 01) and DTC recording (0x85 02)
        self.update_progress(FlashState::BusSilenced, 25, 0, 0, 0, total_bytes, "Silencing vehicle CAN traffic (0x28) and DTC recording (0x85)...", None);
        ka.tick(uds).await?;
        uds.send_request(0x28, &[0x01, 0x01]).await?;
        ka.touch();
        uds.send_request(0x85, &[0x02]).await?;
        ka.touch();

        // Step 4: Programming Session (0x10 02)
        self.update_progress(FlashState::SessionProgramming, 30, 0, 0, 0, total_bytes, "Entering Programming Session (0x10 02)...", None);
        ka.tick(uds).await?;
        let resp = uds.diagnostic_session_control(0x02).await?;
        if resp.get(1) != Some(&0x02) {
            return Err(SterngateError::ProtocolError(format!("Programming session not confirmed: {resp:02X?}")));
        }
        ka.touch();

        // Step 5: Erase (0x31 01 FF 00) - point of no return
        self.update_progress(FlashState::Erasing, 40, 0, 0, 0, total_bytes, "Erasing ECU Flash Memory sectors (0x31 01 FF 00)...", None);
        ka.tick(uds).await?;
        progress.post_erase = true;
        let resp = uds.routine_control(0x01, ERASE_ROUTINE_ID, &[]).await?;
        let status = parse_routine_status(&resp, 0x01, ERASE_ROUTINE_ID)?;
        if status != CHECKSUM_STATUS_OK {
            return Err(SterngateError::FlashAborted(format!("erase routine reported status 0x{status:02X}")));
        }
        ka.touch();

        // Step 6: Request Download (0x34) from the manifest, then Transfer Data (0x36)
        let a = manifest.flash_start_address.to_be_bytes();
        let l = manifest.flash_length.to_be_bytes();
        ka.tick(uds).await?;
        let resp = uds
            .send_request(0x34, &[0x00, 0x44, a[0], a[1], a[2], a[3], l[0], l[1], l[2], l[3]])
            .await?;
        ka.touch();
        let max_block_len = parse_request_download(&resp)?;
        // maxNumberOfBlockLength counts SID + block counter; ISO-TP caps the whole payload.
        let chunk_len = manifest.block_size.min(max_block_len - 2).min(ISOTP_MAX_PAYLOAD - 2);
        if chunk_len == 0 {
            return Err(SterngateError::FlashAborted("negotiated block size leaves no room for data".into()));
        }
        let chunks: Vec<&[u8]> = rom_data.chunks(chunk_len).collect();
        progress.total_blocks = chunks.len();
        self.update_progress(FlashState::Transferring, 45, 0, chunks.len(), 0, total_bytes, "Download accepted; transferring blocks (0x36)...", None);

        for (i, chunk) in chunks.iter().enumerate() {
            let block_num = u8::try_from((i + 1) % 256).map_err(|_| SterngateError::Internal("block counter".into()))?;
            let mut payload = vec![block_num];
            payload.extend_from_slice(chunk);
            ka.tick(uds).await?;
            let resp = uds.send_request(0x36, &payload).await?;
            ka.touch();
            if resp.get(1) != Some(&block_num) {
                return Err(SterngateError::ProtocolError(format!(
                    "TransferData block {} not acknowledged (echo {:02X?})",
                    i + 1,
                    resp.get(1)
                )));
            }
            progress.current_block = i + 1;
            progress.bytes_written += chunk.len();
            let pct = 45 + u8::try_from(progress.bytes_written * 45 / total_bytes.max(1)).unwrap_or(45);
            let log_msg = format!("Writing Block {}/{} ({} bytes written)", i + 1, chunks.len(), progress.bytes_written);
            self.update_progress(FlashState::Transferring, pct, i + 1, chunks.len(), progress.bytes_written, total_bytes, &log_msg, None);
        }

        // Step 7: Request Transfer Exit (0x37)
        self.update_progress(FlashState::TransferExited, 92, progress.current_block, progress.total_blocks, progress.bytes_written, total_bytes, "Exiting Transfer (0x37)...", None);
        ka.tick(uds).await?;
        uds.send_request(0x37, &[]).await?;
        ka.touch();

        // Step 8: ECU checksum routine, before anything leaves the bootloader
        self.update_progress(FlashState::VerifyingChecksum, 95, progress.current_block, progress.total_blocks, progress.bytes_written, total_bytes, "Verifying checksum on written sectors...", None);
        ka.tick(uds).await?;
        let resp = uds.routine_control(0x01, CHECKSUM_ROUTINE_ID, &[]).await?;
        ka.touch();
        let status = parse_routine_status(&resp, 0x01, CHECKSUM_ROUTINE_ID)?;
        if status != CHECKSUM_STATUS_OK {
            return Err(SterngateError::ChecksumMismatch {
                expected: format!("0x{CHECKSUM_STATUS_OK:02X}"),
                calculated: format!("0x{status:02X}"),
            });
        }

        // Step 9: ECU reset. A verified image that fails to reset is not a failed flash.
        self.update_progress(FlashState::ResettingEcu, 98, progress.current_block, progress.total_blocks, progress.bytes_written, total_bytes, "Issuing ECU Hard Reset (0x11 01)...", None);
        ka.tick(uds).await?;
        if uds.ecu_reset(0x01).await.is_err() {
            progress.reset_warning = Some("ECU did not acknowledge reset; cycle ignition manually".into());
        }
        Ok(())
    }
```

Delete the old per-step blocks and the `tokio::time::sleep(500)` / `sleep(20)` calls (the mock answers synchronously; on hardware the ECU's own P2\* pacing applies). Keep `update_progress` as is.

- [ ] **Step 4: Run the tests**

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | head` → all pass, including `test_mcp_discovery_service_flash_and_report_tools` (full flash on the mock) and the server vault test (background flash).

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result" | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
git add crates/sterngate-protocol/src/flasher.rs
git commit -m "fix(protocol): run the programming sequence under one lock with keep-alive and no ignored step

Security access, erase, download, every transfer block, exit and the ECU
checksum now abort the flash on failure; the state ends Failed with a
message that says whether the ECU was erased. RequestDownload is built
from the manifest and the block size is clamped to the ECU's maximum.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: 423 guards on every interface-touching route

**Files:**
- Modify: `crates/sterngate-server/src/routes/community_mods.rs` (`mods_inspect`), `crates/sterngate-server/src/routes/diagnostics.rs` (`get_dtcs`, `clear_dtcs`, `scan_vehicle_quick_test`)
- Test: `crates/sterngate-server/src/lib.rs`

**Interfaces:**
- Produces: the four routes return `423 Locked` with `{"success": false, "error": "System is locked in a flashing routine"}` (the DTC list route returns 423 with an empty JSON array body is NOT acceptable; return the error object) while `state.flasher.is_locked().await` is true.

- [ ] **Step 1: Write the failing test**

In `crates/sterngate-server/src/lib.rs` tests:

```rust
    #[tokio::test]
    async fn test_interface_routes_return_423_while_flashing() {
        let mut iface = Box::new(VirtualCanInterface::new());
        let _ = iface.open().await;
        let profile =
            VehicleProfile::load_from_file("../../profiles/mercedes/w211_om646_edc16.json").unwrap();
        let flasher = Arc::new(FlashingWorker::new());
        let state = Arc::new(AppState::new(iface, profile, flasher.clone()));

        // Start a flash that will hold the lock: the request is spawned and we
        // observe the lock through the same worker.
        let rom = vec![0x5A; 100_000];
        let mut hasher = sha2::Sha256::new();
        sha2::Digest::update(&mut hasher, &rom);
        let manifest = sterngate_core::FlashPackageManifest {
            target_module: "EDC16".into(),
            expected_hw_id: "0281012224".into(),
            expected_sw_id: "1037372332".into(),
            sha256_checksum: format!("{:x}", sha2::Digest::finalize(hasher)),
            crc32_checksum: crc32fast::hash(&rom),
            flash_start_address: 0x0004_0000,
            flash_length: 100_000,
            block_size: 256,
        };
        let f = flasher.clone();
        let iface_arc = state.interface.clone();
        let handle = tokio::spawn(async move { f.execute_flash(manifest, rom, 13.5, iface_arc).await });
        // Wait until the worker reports a locked state.
        let mut rx = flasher.subscribe();
        while !flasher.is_locked().await {
            rx.changed().await.unwrap();
        }

        for (method, uri, body) in [
            ("GET", "/api/v1/dtc", None),
            ("POST", "/api/v1/dtc/clear", None),
            ("POST", "/api/v1/vehicle/scan", Some(json!({}))),
            ("POST", "/api/v1/mods/inspect", Some(json!({"content": "{}"}))),
        ] {
            let mut req = Request::builder().method(method).uri(uri);
            let req = match body {
                Some(b) => req.header("Content-Type", "application/json").body(Body::from(serde_json::to_vec(&b).unwrap())).unwrap(),
                None => req.body(Body::empty()).unwrap(),
            };
            let resp = create_router(state.clone()).oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::LOCKED, "{method} {uri}");
        }
        let _ = handle.await;
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sterngate-server test_interface_routes_return_423 2>&1 | tail -8` → fails on `/api/v1/dtc` (200; the route waits on the interface lock instead).

- [ ] **Step 3: Implement**

Add to each of the four handlers, as the first statement, the existing pattern from `diag_discover`:

```rust
    if state.flasher.is_locked().await {
        return (
            StatusCode::LOCKED,
            Json(serde_json::json!({
                "success": false,
                "error": "System is locked in a flashing routine",
            })),
        )
            .into_response();
    }
```

`get_dtcs` currently returns `Json<Vec<Dtc>>`; change its return type to `impl IntoResponse` and wrap the success path as `(StatusCode::OK, Json(dtcs)).into_response()`. `clear_dtcs` returns `StatusCode`; change to `impl IntoResponse` returning `StatusCode::OK.into_response()` on success.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p sterngate-server 2>&1 | tail -6` → all pass (the existing DTC tests read the JSON body unchanged).

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result" | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
git add crates/sterngate-server/src/routes/community_mods.rs crates/sterngate-server/src/routes/diagnostics.rs crates/sterngate-server/src/lib.rs
git commit -m "fix(server): return 423 from every interface-touching route while a flash holds the bus

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: Flow Control BlockSize, WAIT and OVFLW

**Files:**
- Modify: `crates/sterngate-protocol/src/isotp.rs` (`send_payload`, `wait_for_flow_control`)
- Test: `crates/sterngate-protocol/src/isotp.rs` `mod tests`

**Interfaces:**
- Produces: after sending `BS` Consecutive Frames (when `BS != 0`) the sender waits for another Flow Control and re-reads BS and STmin; `FS = 1` (WAIT) re-awaits up to `N_WFT_MAX = 8` times; `FS = 2` (OVFLW) → `IsoTpError("receiver overflow")`; any other FS → error.

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `isotp.rs`:

```rust
    #[tokio::test(start_paused = true)]
    async fn sender_waits_for_fc_after_block_size() {
        // 20-byte payload = FF + 2 CFs. FC says BS = 1: after one CF the sender must wait for a second FC.
        let mut iface = ScriptedInterface::new().rule(0x2E, &[&[0x30, 0x01, 0x00]]);
        let mut ch = IsoTpChannel::new(&mut iface, 0x7E0, 0x7E8);
        ch.set_timeout(Duration::from_millis(200));
        let payload = vec![0x2E; 20];
        // No second FC is scripted, so the sender must time out after exactly one CF.
        assert!(matches!(ch.send_payload(&payload).await, Err(SterngateError::IsoTpTimeout)));
        let cfs = iface.sent_frames().iter().filter(|f| f.data.first().map(|b| b >> 4) == Some(2)).count();
        assert_eq!(cfs, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn flow_status_wait_is_honoured() {
        let mut iface = ScriptedInterface::new().rule(0x2E, &[&[0x31, 0x00, 0x00], &[0x30, 0x00, 0x00]]);
        let mut ch = IsoTpChannel::new(&mut iface, 0x7E0, 0x7E8);
        ch.send_payload(&vec![0x2E; 20]).await.unwrap();
        let cfs = iface.sent_frames().iter().filter(|f| f.data.first().map(|b| b >> 4) == Some(2)).count();
        assert_eq!(cfs, 2);
    }

    #[tokio::test(start_paused = true)]
    async fn flow_status_overflow_is_error() {
        let mut iface = ScriptedInterface::new().rule(0x2E, &[&[0x32, 0x00, 0x00]]);
        let mut ch = IsoTpChannel::new(&mut iface, 0x7E0, 0x7E8);
        let err = ch.send_payload(&vec![0x2E; 20]).await.unwrap_err();
        assert!(err.to_string().contains("overflow"), "{err}");
    }

    #[tokio::test(start_paused = true)]
    async fn flow_status_wait_gives_up_after_n_wft_max() {
        let waits: Vec<&[u8]> = vec![&[0x31, 0, 0]; 9];
        let mut iface = ScriptedInterface::new().rule(0x2E, &waits);
        let mut ch = IsoTpChannel::new(&mut iface, 0x7E0, 0x7E8);
        assert!(matches!(ch.send_payload(&vec![0x2E; 20]).await, Err(SterngateError::IsoTpError(_))));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sterngate-protocol isotp:: 2>&1 | tail -15` → `sender_waits_for_fc_after_block_size` fails (2 CFs sent), `flow_status_wait_is_honoured` fails (error on FS=1), `overflow` message differs.

- [ ] **Step 3: Implement**

In `isotp.rs`:

```rust
/// Maximum number of consecutive Flow Control WAIT frames tolerated (N_WFTmax).
const N_WFT_MAX: usize = 8;

fn st_min_to_ms(st: u8) -> u64 {
    match st {
        st if st <= 127 => u64::from(st),
        st if (0xF1..=0xF9).contains(&st) => 1,
        _ => 10,
    }
}
```

`wait_for_flow_control` returns `Result<(u8, u8)>` = `(block_size, st_min)`:

```rust
    async fn wait_for_flow_control(&mut self) -> Result<(u8, u8)> {
        for _ in 0..=N_WFT_MAX {
            let frame = timeout(self.timeout_duration, async {
                loop {
                    let f = self.interface.recv().await?;
                    if f.id == self.rx_id && f.data.first().is_some_and(|b| (b >> 4) == 0x03) {
                        return Ok::<CanFrame, SterngateError>(f);
                    }
                }
            })
            .await
            .map_err(|_| SterngateError::IsoTpTimeout)??;

            if frame.data.len() < 3 {
                return Err(SterngateError::IsoTpError(format!("Flow Control frame too short: {} bytes", frame.data.len())));
            }
            match frame.data[0] & 0x0F {
                0 => return Ok((frame.data[1], frame.data[2])),
                1 => continue, // WAIT: the receiver asks for another Flow Control
                2 => return Err(SterngateError::IsoTpError("Flow Control: receiver overflow".into())),
                fs => return Err(SterngateError::IsoTpError(format!("Flow Control status not CTS: {fs}"))),
            }
        }
        Err(SterngateError::IsoTpError(format!("Flow Control WAIT repeated more than {N_WFT_MAX} times")))
    }
```

In `send_payload`, replace the FC handling and the CF loop with:

```rust
        let (mut block_size, mut st_min) = self.wait_for_flow_control().await?;
        let mut st_min_ms = st_min_to_ms(st_min);

        let mut offset = 6;
        let mut seq_num = 1u8;
        let mut sent_in_block = 0u8;

        while offset < len {
            let chunk_size = (len - offset).min(7);
            let pci = 0x20 | (seq_num & 0x0F);
            let mut cf_data = vec![pci];
            cf_data.extend_from_slice(&payload[offset..offset + chunk_size]);
            while cf_data.len() < 8 {
                cf_data.push(0xAA);
            }
            self.interface.send(CanFrame::new_standard(self.tx_id as u16, &cf_data)).await?;

            offset += chunk_size;
            seq_num = (seq_num + 1) % 16;

            if block_size != 0 && offset < len {
                sent_in_block += 1;
                if sent_in_block == block_size {
                    let (bs, st) = self.wait_for_flow_control().await?;
                    block_size = bs;
                    st_min = st;
                    st_min_ms = st_min_to_ms(st_min);
                    sent_in_block = 0;
                }
            }

            if st_min_ms > 0 {
                tokio::time::sleep(Duration::from_millis(st_min_ms)).await;
            }
        }
```

(`st_min` is only read through `st_min_ms`; drop the separate variable if clippy reports it unused.)

- [ ] **Step 4: Run the tests**

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED" | head` → all pass (the virtual ECU answers BS = 0, so nothing else changes).

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result" | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
git add crates/sterngate-protocol/src/isotp.rs
git commit -m "fix(protocol): honour ISO-TP Flow Control BlockSize, WAIT and OVFLW when sending

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 8: Real hardware-identity gate (F192, exact) and honest ROM inspection

**Files:**
- Modify: `crates/sterngate-protocol/src/flasher.rs` (`run_preflight_checks` step 4, `inspect_rom`), `crates/sterngate-protocol/src/lib.rs` (re-exports), `crates/sterngate-mcp/src/tools/flashing.rs` (`sterngate_verify_flash_staging`, demo manifest ids)
- Test: `crates/sterngate-protocol/src/flasher.rs` `mod tests`, `crates/sterngate-protocol/src/lib.rs` (`test_flasher_preflight_battery_interlock`, `test_rom_signature_inspection_and_hardware_matching` stay green on the ASCII mock), `crates/sterngate-mcp/src/lib.rs`

**Interfaces:**
- Produces: `pub async fn read_supplier_hw_id(interface: &mut dyn VehicleInterface, tx_id: u32, rx_id: u32) -> Result<String>` — `DeviceNotFound` when not connected; reads DID `0xF192`; requires `resp.len() > 3` and `resp[1..3] == [0xF1, 0x92]`; ASCII payload returned as text, otherwise each byte rendered as two hex digits (`02 81 01 22` → `"02810122"`).
- Produces: `pub fn hw_id_matches(expected: &str, live: &str) -> bool` — trim + ASCII-uppercase both; `expected.len() >= 8 && live.len() >= 8 && expected == live` (exact; no prefix rule).
- Produces: preflight `hw_id_match` is `false` when the interface is disconnected, the read fails, or the ids differ; the manifest's `expected_hw_id` must be non-empty.
- Produces: `inspect_rom` starts with `can_flash = false` and sets it `true` only in the `Match`/`CalibrationUpdate` arms after a live F192 matched; a connected ECU that does not answer F192 yields `Unknown` with `can_flash = false`; the F191 hex fallback is dropped (F191 is the OEM number, a different namespace).
- Produces: MCP `sterngate_verify_flash_staging` runs `run_preflight_checks` against the virtual ECU with the demo manifest and returns its real `passed`, `details`, `hw_id_match`; the demo manifests use `expected_hw_id: "0281012224"` and `expected_sw_id: "1037372332"` (what the virtual ECU reports).

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `flasher.rs`:

```rust
    #[tokio::test]
    async fn preflight_hw_mismatch_fails_closed() {
        let mut iface = ScriptedInterface::new().rule(
            0x22,
            &[&[0x10, 0x0D, 0x62, 0xF1, 0x92, b'0', b'2', b'8'], &[0x21, b'1', b'0', b'1', b'3', b'3', b'4', b'5']],
        );
        let rom = vec![0x5A; 64];
        let m = manifest(&rom); // expects 0281012224
        let report = FlashingWorker::new().run_preflight_checks(&m, &rom, 13.5, &mut iface).await.unwrap();
        assert!(!report.hw_id_match);
        assert!(!report.passed);
        assert!(report.details.iter().any(|d| d.contains("MISMATCH")));
    }

    #[tokio::test]
    async fn preflight_hw_read_error_fails_closed() {
        let mut iface = ScriptedInterface::new().rule(0x22, &[&[0x03, 0x7F, 0x22, 0x31]]);
        let rom = vec![0x5A; 64];
        let report = FlashingWorker::new().run_preflight_checks(&manifest(&rom), &rom, 13.5, &mut iface).await.unwrap();
        assert!(!report.hw_id_match);
    }

    #[tokio::test]
    async fn preflight_not_connected_fails_closed() {
        let mut iface = ScriptedInterface::new().disconnected();
        let rom = vec![0x5A; 64];
        let report = FlashingWorker::new().run_preflight_checks(&manifest(&rom), &rom, 13.5, &mut iface).await.unwrap();
        assert!(!report.passed);
        assert!(iface.sent_frames().is_empty());
    }

    #[tokio::test]
    async fn preflight_sibling_variant_is_not_a_match() {
        // 0281012224 vs 0281012238: only the last digits differ; a prefix rule would accept it.
        let mut iface = ScriptedInterface::new().rule(
            0x22,
            &[&[0x10, 0x0D, 0x62, 0xF1, 0x92, b'0', b'2', b'8'], &[0x21, b'1', b'0', b'1', b'2', b'2', b'3', b'8']],
        );
        let rom = vec![0x5A; 64];
        let report = FlashingWorker::new().run_preflight_checks(&manifest(&rom), &rom, 13.5, &mut iface).await.unwrap();
        assert!(!report.hw_id_match);
    }

    #[test]
    fn hw_id_matches_is_exact() {
        assert!(hw_id_matches("0281012224", " 0281012224 "));
        assert!(!hw_id_matches("0281012224", "02810122"));
        assert!(!hw_id_matches("0281012224", "0281012238"));
        assert!(!hw_id_matches("", ""));
        assert!(!hw_id_matches("0281012224", ""));
    }

    #[tokio::test]
    async fn inspect_rom_connected_but_silent_is_not_flashable() {
        let mut iface = ScriptedInterface::new().rule(0x22, &[&[0x03, 0x7F, 0x22, 0x31]]);
        let mut rom = vec![0xEA; 4096];
        rom[64..74].copy_from_slice(b"0281012224");
        let report = FlashingWorker::new().inspect_rom(&mut iface, 0x7E0, 0x7E8, &rom).await.unwrap();
        assert!(!report.can_flash);
        assert_eq!(report.verdict, RomCompatibilityVerdict::Unknown);
    }

    #[tokio::test]
    async fn inspect_rom_without_markers_is_not_flashable() {
        let mut iface = ScriptedInterface::new();
        let report = FlashingWorker::new().inspect_rom(&mut iface, 0x7E0, 0x7E8, &vec![0xEA; 4096]).await.unwrap();
        assert!(!report.can_flash);
    }
```

In `crates/sterngate-mcp/src/lib.rs` `mod tests`, add:

```rust
    #[tokio::test]
    async fn test_mcp_verify_flash_staging_runs_real_preflight() {
        let verify = tools::handle_tool_call("sterngate_verify_flash_staging", &json!({}))
            .await
            .unwrap();
        assert!(verify["passed"].as_bool().unwrap());
        assert!(verify["hw_id_match"].as_bool().unwrap());
        assert!(verify["checksum_match"].as_bool().unwrap());
        assert!(verify["simulated"].as_bool().unwrap());
        assert!(verify["details"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d.as_str().unwrap().contains("0281012224")));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sterngate-protocol flasher:: 2>&1 | tail -20` → `hw_id_matches` not found; `preflight_hw_mismatch_fails_closed` fails (`hw_id_match` is `true` today); `inspect_rom_*` fail (`can_flash` true).

- [ ] **Step 3: Implement**

In `flasher.rs`:

```rust
/// Read the system-supplier ECU hardware number (DID 0xF192, the Bosch
/// `0281…` number). ASCII payloads are returned as text; binary-coded ones are
/// rendered as hex so they can never accidentally equal an ASCII manifest id.
pub async fn read_supplier_hw_id(interface: &mut dyn VehicleInterface, tx_id: u32, rx_id: u32) -> Result<String> {
    if !interface.is_connected() {
        return Err(SterngateError::DeviceNotFound("interface not connected".into()));
    }
    let mut uds = UdsClient::new(interface, tx_id, rx_id);
    let resp = uds.read_data_by_identifier(0xF192).await?;
    if resp.len() <= 3 || resp.get(1..3) != Some(&[0xF1, 0x92]) {
        return Err(SterngateError::IsoTpError(format!("F192 reply malformed: {resp:02X?}")));
    }
    let payload = &resp[3..];
    if payload.iter().all(u8::is_ascii_graphic) {
        Ok(String::from_utf8_lossy(payload).to_string())
    } else {
        Ok(payload.iter().map(|b| format!("{b:02X}")).collect())
    }
}

/// Exact hardware-number comparison. Bosch numbers differ in their last digits
/// between hardware variants, so a prefix rule would accept a sibling ECU.
pub fn hw_id_matches(expected: &str, live: &str) -> bool {
    let e = expected.trim().to_ascii_uppercase();
    let l = live.trim().to_ascii_uppercase();
    e.len() >= 8 && l.len() >= 8 && e == l
}
```

Preflight step 4 becomes:

```rust
        // 4. Hardware identity: the ECU's supplier number must equal the manifest's.
        let hw_match = if manifest.expected_hw_id.trim().is_empty() {
            details.push("Hardware ID check FAILED (fail-closed): manifest declares no expected_hw_id".into());
            false
        } else {
            match read_supplier_hw_id(interface, FLASH_TX_ID, FLASH_RX_ID).await {
                Ok(live) => {
                    let ok = hw_id_matches(&manifest.expected_hw_id, &live);
                    details.push(format!(
                        "ECU F192 supplier HW '{live}' vs manifest '{}' -> {}",
                        manifest.expected_hw_id,
                        if ok { "MATCH" } else { "MISMATCH" }
                    ));
                    ok
                }
                Err(e) => {
                    details.push(format!("Hardware ID check FAILED (fail-closed): {e}"));
                    false
                }
            }
        };
```

`inspect_rom`: replace the F192/F191 block with `let hw = read_supplier_hw_id(interface, tx_id, rx_id).await.ok();` (keep the F194/F187 reads through a `UdsClient` created after it; they are informational). Initialise `let mut can_flash = false;`. In the verdict logic: the `HardwareMismatch` arm stays (`can_flash = false`); the `Match`/`CalibrationUpdate` arms set `can_flash = true`, using `hw_id_matches(sig_hw, live_hw)` instead of the prefix compare (the SW compare may stay a prefix compare); the arm for "signature present, no live id" becomes:

```rust
        } else if let Some(sig_hw) = &signatures.bosch_hw_id {
            if interface.is_connected() {
                verdict = RomCompatibilityVerdict::Unknown;
                explanation = format!("ECU connected but did not return a supplier hardware number (F192); cannot confirm firmware '{sig_hw}' matches the installed hardware.");
            } else {
                verdict = RomCompatibilityVerdict::Unknown;
                explanation = format!("Firmware signature detected: Bosch HW {sig_hw}, SW {}. ECU offline: identity not verified, not flashable.", signatures.bosch_sw_id.as_deref().unwrap_or("Unknown"));
            }
        } else {
```

(the raw-binary arm keeps `Unknown`, `can_flash` stays false). Re-export from `lib.rs`: `pub use flasher::{hw_id_matches, read_supplier_hw_id, FlashingWorker, CHECKSUM_ROUTINE_ID, ERASE_ROUTINE_ID, FLASH_RX_ID, FLASH_TX_ID};`.

MCP `tools/flashing.rs`: `sterngate_verify_flash_staging` builds the same demo manifest as `sterngate_flash_ecu` (extract a `fn demo_manifest(target_module: &str, rom: &[u8]) -> FlashPackageManifest` with `expected_hw_id: "0281012224"`, `expected_sw_id: "1037372332"`, `block_size: 512`), runs `FlashingWorker::new().run_preflight_checks(&manifest, &rom, 13.8, &mut mock_iface).await` and returns `{"passed": report.passed, "battery_voltage": 13.8, "min_voltage_required": report.min_voltage_required, "hw_id_match": report.hw_id_match, "checksum_match": report.checksum_match, "details": report.details, "simulated": true, "advice": ...}`. Use `demo_manifest` in `sterngate_flash_ecu` too (its `expected_hw_id` was `0281012234`, which no longer matches).

- [ ] **Step 4: Run the tests**

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | head` → all pass. `test_flasher_preflight_battery_interlock` (13.5 V → passed) stays green because the virtual ECU now answers F192 with `0281012224`; `test_rom_signature_inspection_and_hardware_matching` keeps its match/mismatch expectations.

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result" | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
git add crates/sterngate-protocol crates/sterngate-mcp
git commit -m "fix(protocol): compare the ECU's F192 supplier number exactly in preflight and inspection

The hardware gate was a constant true; a ROM for a sibling variant passed
preflight and proceeded to erase.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 9: Containers are never staged

**Files:**
- Create: `crates/sterngate-core/src/cff/mod.rs`
- Modify: `crates/sterngate-core/src/lib.rs` (`pub mod cff;`, re-export `cff::sniff`), `crates/sterngate-core/src/flash.rs` (`FirmwareVaultEntry.stageable`, `scan_recursive`, `find_upgrade_recommendation`), `crates/sterngate-server/src/routes/flashing.rs` (`vault_stage`), `crates/sterngate-cli/src/commands/flash.rs` (print `stageable`), `.gitignore`
- Test: `crates/sterngate-core/src/cff/mod.rs`, `crates/sterngate-core/src/lib.rs`, `crates/sterngate-server/src/lib.rs`

**Interfaces:**
- Produces: `sterngate_core::cff::sniff(bytes: &[u8]) -> bool` — true when `bytes` starts with `b"CFF-TRANSLATOR-VERSION"` and `bytes[0x400..0x402] == [0xED, 0x05]`.
- Produces: `FirmwareVaultEntry.stageable: bool` (`#[serde(default)]`, false for old JSON); `.cff`/`.smr-f` entries and any file that sniffs as CFF get `stageable: false` and signatures with `bosch_hw_id`, `bosch_sw_id`, `oem_part_number`, `project_name` all `None`; `find_upgrade_recommendation` ignores non-stageable entries; `POST /api/v1/vault/stage` returns 400 `"Refusing to stage: file is a Caesar flash container (.cff); extract a verified segment with `sterngate corpus extract` first (Phase 1)"` for a `.cff`/`.smr-f` extension or a sniffed container.

- [ ] **Step 1: Write the failing tests**

`crates/sterngate-core/src/cff/mod.rs` (new):

```rust
//! Mercedes SDflash `.CFF` flash containers. Phase 0 only recognises them so
//! the vault never treats a container as a raw image; Phase 1 adds the parser.

/// Byte-cheap detection of a CFF container: the ASCII prologue at offset 0
/// and the `0x05ED` stub magic at 0x400 (little-endian).
pub fn sniff(bytes: &[u8]) -> bool {
    bytes.starts_with(b"CFF-TRANSLATOR-VERSION") && bytes.get(0x400..0x402) == Some(&[0xED, 0x05])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal synthetic container: prologue, NUL padding, stub magic. No firmware bytes.
    pub(crate) fn synthetic_cff_header() -> Vec<u8> {
        let mut v = b"CFF-TRANSLATOR-VERSION:02.01.03\nDATE:15.9.2026\nFINGERPRINT:1.2.3.4\nCFF:TEST\nLANGUAGE:ORIGINAL\n".to_vec();
        v.resize(0x400, 0);
        v.extend_from_slice(&[0xED, 0x05, 0xEA, 0x07, 0x09, 0x0F]);
        v.resize(0x1000, 0xFF);
        v
    }

    #[test]
    fn sniff_recognises_prologue_and_stub_magic() {
        assert!(sniff(&synthetic_cff_header()));
        assert!(!sniff(b"CFF-TRANSLATOR-VERSION"));
        let mut wrong_magic = synthetic_cff_header();
        wrong_magic[0x400] = 0x00;
        assert!(!sniff(&wrong_magic));
        assert!(!sniff(&[0xEA; 4096]));
        assert!(!sniff(&[]));
    }
}
```

Core `lib.rs` tests, add:

```rust
    #[test]
    fn test_firmware_vault_marks_containers_not_stageable() {
        let temp_dir = std::env::temp_dir().join(format!("sterngate_vault_cff_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        // A container carrying the same ids as a raw image inside its payload.
        let mut cff = crate::cff::tests::synthetic_cff_header();
        cff[0x800..0x80A].copy_from_slice(b"0281012224");
        cff[0x900..0x90A].copy_from_slice(b"1037389123");
        std::fs::write(temp_dir.join("update.CFF"), &cff).unwrap();
        // A raw image with the same ids, but named .bin so it sniffs as a container too.
        std::fs::write(temp_dir.join("disguised.bin"), &cff).unwrap();
        // A real raw image.
        let mut rom = vec![0xFF; 2048];
        rom[100..110].copy_from_slice(b"0281012224");
        rom[200..210].copy_from_slice(b"1037389123");
        std::fs::write(temp_dir.join("real.bin"), &rom).unwrap();

        let mut entries = FirmwareVault::scan_directory(&temp_dir);
        entries.sort_by(|a, b| a.filename.cmp(&b.filename));
        assert_eq!(entries.len(), 3);
        let by_name = |n: &str| entries.iter().find(|e| e.filename == n).unwrap();
        assert!(!by_name("update.CFF").stageable);
        assert!(by_name("update.CFF").signatures.bosch_hw_id.is_none());
        assert!(!by_name("disguised.bin").stageable);
        assert!(by_name("real.bin").stageable);

        let rec = FirmwareVault::find_upgrade_recommendation(&entries, "0281012224", "1037372332").unwrap();
        assert_eq!(rec.recommended_file.filename, "real.bin");
        let only_containers: Vec<_> = entries.iter().filter(|e| !e.stageable).cloned().collect();
        assert!(FirmwareVault::find_upgrade_recommendation(&only_containers, "0281012224", "1037372332").is_none());
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
```

(make the cff test helper reachable: mark the `tests` module `pub(crate)` and the helper `pub(crate)`.) Server `lib.rs`, add:

```rust
    #[tokio::test]
    async fn test_vault_stage_refuses_containers() {
        let mut sim = Box::new(VirtualCanInterface::new());
        let _ = sim.open().await;
        let profile = VehicleProfile::load_from_file(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../profiles/mercedes/w211_om646_edc16.json"),
        )
        .unwrap();
        let flasher = Arc::new(FlashingWorker::new());
        let temp_dir = std::env::temp_dir().join(format!("sterngate_vault_cff_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let mut cff = b"CFF-TRANSLATOR-VERSION:02.01.03\nCFF:TEST\n".to_vec();
        cff.resize(0x400, 0);
        cff.extend_from_slice(&[0xED, 0x05, 0, 0, 0, 0]);
        cff.resize(0x1000, 0xFF);
        std::fs::write(temp_dir.join("payload.CFF"), &cff).unwrap();
        std::fs::write(temp_dir.join("disguised.bin"), &cff).unwrap();
        let state = Arc::new(AppState::new(sim, profile, flasher).with_vault_root(Some(temp_dir.clone())));

        for file in ["payload.CFF", "disguised.bin"] {
            let payload = json!({ "file_path": file, "measured_voltage": 13.4 });
            let req = Request::builder().method("POST").uri("/api/v1/vault/stage")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap())).unwrap();
            let resp = create_router(state.clone()).oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "{file}");
            let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
            let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert!(v["error"].as_str().unwrap().contains("container"));
        }
        assert!(!state.flasher.is_locked().await, "no flash may have started");
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sterngate-core cff 2>&1 | tail -5; cargo test -p sterngate-server test_vault_stage_refuses_containers 2>&1 | tail -5` → module missing / `stageable` missing; the server test gets 200 (the container is staged and a flash starts).

- [ ] **Step 3: Implement**

`crates/sterngate-core/src/lib.rs`: add `pub mod cff;` after `pub mod catalog;`.

`flash.rs`: add to `FirmwareVaultEntry` after `format`: `/// False for flash containers (.cff/.smr-f, or anything that sniffs as one); only raw images may be staged.\n#[serde(default)]\npub stageable: bool,`. In `scan_recursive`, after reading `data`:

```rust
                            let is_container = matches!(ext.as_str(), "cff" | "smr-f") || crate::cff::sniff(&data);
                            let sigs = if is_container {
                                // A container's identity lives in its own header, not in
                                // scattered ASCII; and its bytes must never be flashed raw.
                                let raw = FirmwareSignatures::extract(&data);
                                FirmwareSignatures { bosch_hw_id: None, bosch_sw_id: None, oem_part_number: None, project_name: None, ..raw }
                            } else {
                                FirmwareSignatures::extract(&data)
                            };
```

and set `stageable: !is_container` in the literal. In `find_upgrade_recommendation`, add `if !entry.stageable { continue; }` at the top of the loop.

Server `vault_stage`, after `std::fs::read` succeeds:

```rust
    let ext = rom_file.extension().and_then(|s| s.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    if matches!(ext.as_str(), "cff" | "smr-f") || sterngate_core::cff::sniff(&rom_data) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "error": "Refusing to stage: file is a Caesar flash container (.cff); extract a verified segment with `sterngate corpus extract` first (Phase 1).",
            })),
        )
            .into_response();
    }
```

CLI `flash vault-scan`: print `  Stageable:       {}` after `Format`. `.gitignore`: add `*.CFF`, `*.smr-f`, `*.SMR-F`, `/firmware_corpus/` after the `*.cff` line.

- [ ] **Step 4: Run the tests**

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED" | head` → all pass (`test_firmware_vault_scanning_and_recommendation` and `test_firmware_vault_endpoints` use raw `.bin` files and stay green).

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result" | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
git add crates/sterngate-core crates/sterngate-server crates/sterngate-cli .gitignore
git commit -m "fix(vault): never stage a flash container as a raw image

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 10: The server and CLI measure the voltage they gate on

**Files:**
- Modify: `crates/sterngate-server/src/routes/common.rs` (new `resolve_flash_voltage`), `crates/sterngate-server/src/routes/flashing.rs` (`stage_flash`, `vault_stage`), `crates/sterngate-server/src/routes/community_mods.rs` (`mods_apply`), `crates/sterngate-cli/src/commands/flash.rs` (`Preflight`, `Start`), `crates/sterngate-cli/src/args.rs` (`FlashCommands::Preflight { voltage }` doc)
- Test: `crates/sterngate-server/src/routes/common.rs` (unit), `crates/sterngate-server/src/lib.rs`, `crates/sterngate-cli/src/commands/flash.rs`

**Interfaces:**
- Produces: `pub async fn resolve_flash_voltage(iface: &mut dyn VehicleInterface, client_value: Option<f64>) -> std::result::Result<f64, String>` with the rule: adapter measures `v` → use `v`, and if the client also sent a value that differs by more than `VOLTAGE_CROSS_CHECK_TOLERANCE = 1.0` V refuse with a message containing "disagrees"; adapter cannot measure (`Ok(None)`) → client value required, else refuse "no measured battery voltage"; adapter read error → refuse with the error text. The three routes call it after locking the interface and before spawning/applying; the pure decision is `pub fn choose_voltage(measured: Result<Option<f32>>, client: Option<f64>) -> Result<f64, String>` for unit tests.
- Produces: CLI `flash start` takes the voltage from `iface.measure_battery_voltage()` after opening the interface and refuses with "cannot measure battery voltage" on `None`; `flash preflight` uses `--voltage` if given (documented as a multimeter reading for a dry run), otherwise the measurement, otherwise refuses. The 12.6 fallbacks and the second `OpenPortInterface` are deleted.

- [ ] **Step 1: Write the failing tests**

`routes/common.rs`, add a test module (create one if absent):

```rust
#[cfg(test)]
mod tests {
    use super::choose_voltage;
    use sterngate_core::SterngateError;

    #[test]
    fn measured_value_wins_and_cross_checks_client() {
        assert_eq!(choose_voltage(Ok(Some(12.7)), None).unwrap(), f64::from(12.7f32));
        assert_eq!(choose_voltage(Ok(Some(12.7)), Some(12.9)).unwrap(), f64::from(12.7f32));
        let err = choose_voltage(Ok(Some(12.7)), Some(14.5)).unwrap_err();
        assert!(err.contains("disagrees"), "{err}");
    }

    #[test]
    fn unmeasurable_adapter_requires_client_value() {
        assert_eq!(choose_voltage(Ok(None), Some(12.8)).unwrap(), 12.8);
        let err = choose_voltage(Ok(None), None).unwrap_err();
        assert!(err.contains("no measured battery voltage"), "{err}");
    }

    #[test]
    fn adapter_error_refuses() {
        let err = choose_voltage(Err(SterngateError::HalError("usb".into())), Some(12.8)).unwrap_err();
        assert!(err.contains("usb"), "{err}");
    }
}
```

Server `lib.rs`: extend `test_flash_stage_refuses_without_measured_voltage` with a second request carrying `"measured_voltage": 13.0` and assert it returns 200 (the virtual ECU cannot measure, so the client value is accepted as before). CLI `commands/flash.rs`, add:

```rust
#[cfg(test)]
mod tests {
    use super::voltage_for_flash;

    #[test]
    fn flash_voltage_refuses_when_unmeasurable() {
        assert!(voltage_for_flash(Ok(None), None, "can0").unwrap_err().to_string().contains("cannot measure"));
        assert!((voltage_for_flash(Ok(None), Some(12.9), "can0").unwrap() - 12.9).abs() < 1e-9);
        assert!((voltage_for_flash(Ok(Some(12.7)), None, "openport").unwrap() - f64::from(12.7f32)).abs() < 1e-9);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sterngate-server common:: 2>&1 | tail -5; cargo test -p sterngate-cli 2>&1 | tail -5` → functions not found.

- [ ] **Step 3: Implement**

`routes/common.rs`:

```rust
use sterngate_core::Result;
use sterngate_hal::VehicleInterface;

/// A client-supplied voltage may not disagree with the adapter's own reading by more than this.
pub const VOLTAGE_CROSS_CHECK_TOLERANCE: f64 = 1.0;

/// Decide which voltage gates a write. The adapter's measurement wins; a client
/// value is accepted only when the adapter cannot measure, and refused when it
/// contradicts a measurement.
pub fn choose_voltage(measured: Result<Option<f32>>, client: Option<f64>) -> std::result::Result<f64, String> {
    match measured {
        Ok(Some(v)) => {
            let v = f64::from(v);
            if let Some(c) = client {
                if (c - v).abs() > VOLTAGE_CROSS_CHECK_TOLERANCE {
                    return Err(format!(
                        "Refusing: client-supplied battery voltage {c:.2} V disagrees with the adapter measurement {v:.2} V"
                    ));
                }
            }
            Ok(v)
        }
        Ok(None) => client.ok_or_else(|| {
            "Refusing: no measured battery voltage. This adapter cannot measure; send 'measured_voltage' from a real hardware reading.".to_string()
        }),
        Err(e) => Err(format!("Refusing: battery voltage read failed: {e}")),
    }
}

pub async fn resolve_flash_voltage(iface: &mut dyn VehicleInterface, client_value: Option<f64>) -> std::result::Result<f64, String> {
    choose_voltage(iface.measure_battery_voltage().await, client_value)
}
```

`stage_flash` and `vault_stage`: replace the `let Some(measured_voltage) = payload.measured_voltage else { 400 }` block with

```rust
    let measured_voltage = {
        let mut iface = state.interface.lock().await;
        match super::common::resolve_flash_voltage(iface.as_mut(), payload.measured_voltage).await {
            Ok(v) => v,
            Err(message) => {
                return (StatusCode::BAD_REQUEST, Json(GenericResponse { success: false, message })).into_response();
            }
        }
    };
```

(`vault_stage` uses its `serde_json::json!({"success": false, "error": message})` shape.) `mods_apply`: replace the `battery_voltage` `else { 400 }` block the same way, keeping the lock scoped so `apply_mod` can take it afterwards. Rename the payload field docs to say the value is a cross-check when the adapter measures.

CLI `flash.rs`: add at module level

```rust
/// Voltage that gates a flash: the opened interface's own measurement, or an
/// explicit reading only when the adapter cannot measure.
pub(crate) fn voltage_for_flash(
    measured: sterngate_core::Result<Option<f32>>,
    explicit: Option<f64>,
    iface_name: &str,
) -> Result<f64> {
    match (measured, explicit) {
        (Ok(Some(v)), _) => Ok(f64::from(v)),
        (Ok(None), Some(v)) => Ok(v),
        (Ok(None), None) => anyhow::bail!("Refusing: interface `{iface_name}` cannot measure battery voltage (Tactrix OpenPort Pin 16 ADC required, or --voltage for a dry-run preflight)"),
        (Err(e), _) => anyhow::bail!("Refusing: battery voltage read failed: {e}"),
    }
}
```

`Preflight`: open the interface first, then `let batt_voltage = voltage_for_flash(iface.measure_battery_voltage().await, voltage, &cli.can_interface)?;`. `Start`: open the interface, `let batt_voltage = voltage_for_flash(iface.measure_battery_voltage().await, None, &cli.can_interface)?;` (no override), print the measured value in the banner, then wrap the interface in the `Arc<Mutex<..>>`. Delete both `OpenPortInterface::new()` blocks and the `12.6` constants; remove the now-unused `OpenPortInterface` import. `args.rs` `Preflight.voltage` doc: `/// Override the battery voltage for a dry-run preflight only (a multimeter reading); ignored when the adapter can measure`.

- [ ] **Step 4: Run the tests**

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED" | head` → all pass (on the virtual ECU the adapter cannot measure, so client values keep working).

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result" | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
git add crates/sterngate-server crates/sterngate-cli
git commit -m "fix(voltage): measure the flash and apply voltage through the interface, cross-checking client values

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 11: One source of truth for flash-package refusals

**Files:**
- Modify: `crates/sterngate-core/src/modpack/mod.rs` (`ERASE_MEMORY_ROUTINE`, `SterngateMod::writes_flash`, `flash_write_refusals`, `check_compatibility`), `crates/sterngate-core/src/lib.rs` (re-export), `crates/sterngate-protocol/src/modrunner.rs` (use the core helpers; drop private duplicates and the redundant inspect mirrors)
- Test: `crates/sterngate-core/src/modpack/mod.rs` `mod tests`; existing protocol tests must stay green

**Interfaces:**
- Produces: `pub const ERASE_MEMORY_ROUTINE: u16 = 0xFF00` in `sterngate_core::modpack` (re-exported); `SterngateMod::writes_flash(&self) -> bool`; `SterngateMod::flash_write_refusals(&self) -> Vec<String>` (one message per non-`Scanned` `PatchFlashMap`/`DtcMask` in actions or rollback — text contains "provenance" — and per `Routine` with id `0xFF00` — text contains "EraseMemory"); `check_compatibility` pushes every refusal to `warning_messages`, sets `matched_vehicle = false`, and applies `max(min_battery_voltage, FLASH_WRITE_MIN_VOLTAGE)` when the package writes flash (message contains "12.5").
- Produces: `ModRunner` calls `modpack.flash_write_refusals()` (first message → `PreFlightCheckFailed`) instead of its own `check_provenance`/`check_no_erase_routine`; `ModRunner::inspect_compatibility` no longer duplicates the provenance/erase/voltage mirrors (the whitelist-read mirror stays). All CLI/MCP inspect paths therefore report the same refusals.

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `modpack/mod.rs`:

```rust
    #[test]
    fn check_compatibility_reports_flash_refusals() {
        let m = SterngateMod::create(sample_metadata(), sample_target(12.5), vec![flash_patch(MapProvenance::Synthetic)], vec![]).unwrap();
        let report = m.check_compatibility(Some("WDB2112061A000001"), None, Some(12.8));
        assert!(!report.matched_vehicle);
        assert!(report.warning_messages.iter().any(|w| w.contains("provenance")));

        let erase = ModAction::Routine { routine_id: ERASE_MEMORY_ROUTINE, subfunction: 0x01, data: vec![], description: "erase".into() };
        let m = SterngateMod::create(sample_metadata(), sample_target(12.0), vec![erase], vec![]).unwrap();
        let report = m.check_compatibility(None, None, None);
        assert!(!report.matched_vehicle);
        assert!(report.warning_messages.iter().any(|w| w.contains("EraseMemory")));
    }

    #[test]
    fn check_compatibility_applies_flash_floor() {
        let m = SterngateMod::create(sample_metadata(), sample_target(12.5), vec![flash_patch(MapProvenance::Scanned)], vec![]).unwrap();
        let low = m.check_compatibility(None, None, Some(12.2));
        assert!(!low.matched_vehicle);
        assert!(low.warning_messages.iter().any(|w| w.contains("12.5")));
        assert!(!low.compatibility_notes.iter().any(|n| n.contains("sufficient")));
        let ok = m.check_compatibility(None, None, Some(12.8));
        assert!(ok.matched_vehicle);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sterngate-core check_compatibility 2>&1 | tail -8` → `ERASE_MEMORY_ROUTINE` not found; after stubbing, the first test fails (`matched_vehicle` true).

- [ ] **Step 3: Implement**

`modpack/mod.rs`:

```rust
/// UDS RoutineControl id for EraseMemory. A community package may never
/// trigger it: erase runs only through the flashing worker's interlocks.
pub const ERASE_MEMORY_ROUTINE: u16 = 0xFF00;

impl SterngateMod {
    /// True when any action or rollback writes flash memory.
    pub fn writes_flash(&self) -> bool {
        self.actions.iter().chain(self.rollback_actions.iter()).any(|a| matches!(a, ModAction::PatchFlashMap { .. } | ModAction::DtcMask { .. }))
    }

    /// Every reason this package may not be executed regardless of the vehicle:
    /// flash actions without `Scanned` provenance and EraseMemory routines.
    pub fn flash_write_refusals(&self) -> Vec<String> {
        let mut out = Vec::new();
        for action in self.actions.iter().chain(self.rollback_actions.iter()) {
            match action {
                ModAction::PatchFlashMap { map_name, provenance, .. } if !provenance.is_scanned() => out.push(format!(
                    "action '{map_name}' has map provenance {provenance:?}; only maps scanned from the target ECU's own ROM may be flashed"
                )),
                ModAction::DtcMask { p_code, provenance, .. } if !provenance.is_scanned() => out.push(format!(
                    "action '{p_code}' has map provenance {provenance:?}; only maps scanned from the target ECU's own ROM may be flashed"
                )),
                ModAction::Routine { routine_id, description, .. } if *routine_id == ERASE_MEMORY_ROUTINE => out.push(format!(
                    "routine '{description}' is UDS EraseMemory (0x{routine_id:04X}); flash erase is only permitted through the flashing worker"
                )),
                _ => {}
            }
        }
        out
    }
}
```

In `check_compatibility`, after the integrity gate and before the VIN check:

```rust
        for refusal in self.flash_write_refusals() {
            matched = false;
            report.warning_messages.push(refusal);
        }
        let required_voltage = if self.writes_flash() { self.target.min_battery_voltage.max(FLASH_WRITE_MIN_VOLTAGE) } else { self.target.min_battery_voltage };
```

and use `required_voltage` (with `volts.is_nan()` counted as too low) in the voltage branch instead of `self.target.min_battery_voltage`. Re-export `ERASE_MEMORY_ROUTINE` from the crate root.

`modrunner.rs`: delete `ERASE_MEMORY_ROUTINE`, `writes_flash`, `check_provenance`, `check_no_erase_routine`; in `apply_mod` replace the provenance and erase gates with

```rust
        if let Some(refusal) = modpack.flash_write_refusals().into_iter().next() {
            return Err(SterngateError::PreFlightCheckFailed(refusal));
        }
        let writes_flash = modpack.writes_flash();
```

and in `inspect_compatibility` remove the provenance/erase/voltage mirror blocks added in Phase 0a (keep the whitelist-read mirror). Update the runner's gate-order doc comment.

- [ ] **Step 4: Run the tests**

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED" | head` → all pass, including `test_synthetic_provenance_refused_before_bus_traffic`, `test_erase_memory_routine_refused` and the three `test_inspect_mirrors_*` tests (now satisfied by core).

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result" | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
git add crates/sterngate-core crates/sterngate-protocol
git commit -m "refactor(modpack): report flash-package refusals from core so every inspect surface agrees with apply

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 12: `ModRunner` keeps the extended session alive

**Files:**
- Modify: `crates/sterngate-protocol/src/modrunner.rs` (`apply_mod`)
- Test: `crates/sterngate-protocol/src/lib.rs`

**Interfaces:**
- Consumes: `S3KeepAlive` (Task 4).
- Produces: after `diagnostic_session_control(0x03)` the runner owns an `S3KeepAlive`, calls `tick` before the F191 read (when the whitelist is non-empty), before every precondition read, and before every write, and `touch` after each successful exchange, so a slow ECU or a long garage commit between preconditions and writes never lets S3 expire.

- [ ] **Step 1: Write the failing test**

In `crates/sterngate-protocol/src/lib.rs` tests (uses `crate::test_support::ScriptedInterface`):

```rust
    #[tokio::test(start_paused = true)]
    async fn test_apply_mod_sends_keepalive_when_ecu_is_slow() {
        use std::time::Duration;
        // Empty hardware whitelist: no F191 read; no bitmask: no precondition read.
        // Each write answers 0x78 at once and completes after 1.6 s (P2* path).
        let iface = crate::test_support::ScriptedInterface::new()
            .rule(0x10, &[&[0x06, 0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]])
            .rule_delayed(0x2E, Duration::from_millis(1600), &[&[0x03, 0x6E, 0x02, 0x01]]);
        let log = iface.sent_handle();
        let mut iface = iface;
        let mut m = runner_mod(vec![write_did(None), write_did(None)], vec![], 12.0);
        ModRunner::apply_mod(&mut iface, &mut m, &vin(23), 12.8, TargetFingerprintPolicy::Enforce).await.unwrap();
        let frames = log.lock().unwrap().clone();
        assert!(frames.iter().any(|f| f.data.get(1) == Some(&0x3E) && f.data.get(2) == Some(&0x80)),
            "a suppressed TesterPresent must be sent between slow writes");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sterngate-protocol test_apply_mod_sends_keepalive 2>&1 | tail -5` → fails (no `3E 80` frame).

- [ ] **Step 3: Implement**

In `apply_mod`, after `let _ = uds.diagnostic_session_control(0x03).await;` add `let mut ka = S3KeepAlive::new();` (import `crate::uds::S3KeepAlive`). Insert `ka.tick(&mut uds).await?;` immediately before: the F191 read, each `read_data_by_identifier` / `read_memory_by_address` in step 5, the bitmask read in step 7, and each `write_data_by_identifier` / `routine_control` / `write_memory_by_address`; insert `ka.touch();` after each of those calls returns `Ok`. The garage snapshot between steps 5 and 7 needs no change: the tick before the first write covers it.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p sterngate-protocol 2>&1 | tail -6` → all pass.

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result" | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
git add crates/sterngate-protocol
git commit -m "fix(protocol): keep the extended session alive while ModRunner reads, commits and writes

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 13: OpenPort reopen after close

**Files:**
- Modify: `crates/sterngate-hal/src/openport.rs` (`open`, `close`)
- Test: `crates/sterngate-hal/src/openport.rs` `mod tests`

**Interfaces:**
- Produces: `open()` rebuilds the RX mpsc channel when `rx_sender` was consumed by an earlier `open()`, so a second open spawns a reader again; `close()` clears the voltage cache (`voltage_cache` → `None`) so a stale reading can never satisfy `measure_battery_voltage` after a reopen; `pub(crate) fn clear_voltage_cache(cache: &VoltageCache)`.

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `openport.rs`:

```rust
    #[test]
    fn close_clears_the_voltage_cache() {
        let cache = VoltageCache::default();
        store_voltage_cache(&cache, 12.7);
        assert!(read_voltage_cache(&cache).is_some());
        clear_voltage_cache(&cache);
        assert!(read_voltage_cache(&cache).is_none());
    }

    #[tokio::test]
    async fn simulated_interface_survives_reopen() {
        let (mut iface, _feed) = OpenPortInterface::new_simulated(12.65);
        iface.open().await.unwrap();
        iface.close().await.unwrap();
        iface.open().await.unwrap();
        assert!(iface.is_connected());
        iface.send(CanFrame::new_standard(0x7E0, &[0x02, 0x10, 0x03])).await.unwrap();
        assert_eq!(iface.recv().await.unwrap().data, vec![0x02, 0x10, 0x03]);
    }

    #[test]
    fn hardware_open_rebuilds_rx_channel_after_take() {
        let mut iface = OpenPortInterface::new();
        iface.rx_sender.take();
        iface.ensure_rx_channel();
        assert!(iface.rx_sender.is_some());
        assert!(iface.rx_channel.is_some());
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sterngate-hal openport:: 2>&1 | tail -8` → `clear_voltage_cache` / `ensure_rx_channel` not found.

- [ ] **Step 3: Implement**

```rust
fn clear_voltage_cache(cache: &VoltageCache) {
    *cache.lock().unwrap_or_else(PoisonError::into_inner) = None;
}
```

In `impl OpenPortInterface`:

```rust
    /// Recreate the RX channel if a previous `open()` handed its sender to a
    /// reader task, so reopening after `close()` gets a live reader again.
    fn ensure_rx_channel(&mut self) {
        if self.rx_sender.is_none() {
            let (tx, rx) = mpsc::channel(256);
            self.rx_sender = Some(tx);
            self.rx_channel = Some(rx);
        }
    }
```

In `open()`, on the hardware path just before `find_and_open_device` (after the simulated early return): `self.ensure_rx_channel();`. Note: `new_simulated` sets `rx_sender: None` on purpose (its feed task owns the sender), so `ensure_rx_channel` must only run on the hardware path. In `close()`, right after `self.is_open.store(false, ..)`: `clear_voltage_cache(&self.voltage_cache);` (both backends; the cache is only read by the hardware `measure_battery_voltage` path). Leave `last_voltage_v` alone: it is the informational `last_voltage()` accessor, not an interlock input.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p sterngate-hal 2>&1 | tail -6` → all pass.

- [ ] **Step 5: Gate and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result" | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
git add crates/sterngate-hal
git commit -m "fix(hal): let the OpenPort interface reopen after close and forget cached voltage on close

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 14: Documentation

**Files:**
- Modify: `.agents/skills/safe-flashing/SKILL.md`, `.agents/skills/sterngate-ops/SKILL.md`, `.agents/skills/sterngate-mcp/SKILL.md`, `.agents/skills/community-mods/SKILL.md`, `.agents/skills/bench-recovery/SKILL.md`, `AGENTS.md`, `README.md`

- [ ] **Step 1: safe-flashing skill**

Add a section `## Phase 0b contract` (after the pre-flight checklist) with, verbatim:

```markdown
## Phase 0b contract

- Pre-flight compares the ECU's system-supplier hardware number (DID F192) with the manifest's `expected_hw_id` **exactly** (trimmed, case-insensitive, both ≥ 8 characters). A read failure, a disconnected interface or an empty manifest id fails the check. `flash_length` must equal the ROM size.
- The programming sequence holds the interface for its whole duration; every step (extended session, security access, communication/DTC control, programming session, erase, RequestDownload, each TransferData block, TransferExit, the ECU checksum routine `0x0202`, reset) aborts the flash on a negative response, a malformed reply or a timeout. The worker sends a suppressed TesterPresent whenever 1.5 s pass without an exchange and waits P2\* (+500 ms) while the ECU reports ResponsePending.
- RequestDownload is built from the manifest's `flash_start_address`/`flash_length`; the block size is the minimum of the manifest's `block_size`, the ECU's `maxNumberOfBlockLength − 2` and 4093. Every TransferData reply must echo the block counter.
- On failure the state is `FAILED` (not locked) and `error_message` starts with either `Flash aborted before erase; ECU untouched.` or `FLASH FAILED AFTER ERASE - ECU is in bootloader with incomplete application. Keep ignition ON, do not disconnect.` A verified image whose ECU does not acknowledge the reset ends `COMPLETED` with the warning `ECU did not acknowledge reset; cycle ignition manually`.
- The ECU checksum routine id `0x0202` and its OK status `0x00` are project constants not yet verified against real EDC16 firmware; a real ECU that reports success differently will fail the flash after the write (fail-closed).
- Vault entries carry `stageable`; `.cff`/`.smr-f` files and anything that sniffs as a Caesar container are never stageable and `POST /api/v1/vault/stage` returns 400 for them. Obtain a raw image via `sterngate corpus extract` (Phase 1).
- Voltage: the server and CLI read `VehicleInterface::measure_battery_voltage` themselves. When the adapter measures, that value gates the flash and a client-supplied `measured_voltage` that differs by more than 1.0 V is refused; when the adapter cannot measure (SocketCAN, mock), the client value is required. `sterngate flash start` refuses on adapters that cannot measure; `flash preflight --voltage` is a dry-run override only.
```

Also update the existing `measured_voltage` bullet (~line 118) to point at this section, and the vault paragraph (~line 111) to say containers are listed but not stageable.

- [ ] **Step 2: ops, MCP, community-mods, bench-recovery skills**

`sterngate-ops`: in the flash section add `# voltage is measured through the adapter; --voltage only overrides a dry-run preflight` above the `flash preflight` example and note that every interface-touching route answers 423 during a flash. `sterngate-mcp`: `sterngate_verify_flash_staging` row → "Runs the real pre-flight (voltage, SHA-256/CRC32, F192 identity, flash_length) against the built-in virtual ECU and returns its report; `simulated: true`". `community-mods`: in the §7 safety contract add "Inspect (CLI, REST, MCP) reports the same refusals as apply: provenance, EraseMemory, the 12.5 V floor" and "`mod apply` keeps the extended session alive with TesterPresent". `bench-recovery`: in Scenario A note that recovery images from SDflash must come through `corpus extract` (Phase 1) and that a bench read of EDC16CP31 internal flash contains block D only (no calibration).

- [ ] **Step 3: AGENTS.md and README**

`AGENTS.md`: `FlashingWorker` bullet → append "Fail-closed since Phase 0b: exact F192 identity, one locked sequence with keep-alive and P2\*, every step aborts on error, checksum before reset, honest FAILED messages." `IsoTpChannel` bullet → append "length-checked receive path, Flow Control BlockSize/WAIT/OVFLW." `UdsClient` bullet → append "P2\* handling, suppressed TesterPresent, `S3KeepAlive`." `FlashPackage`/vault bullet → mention `stageable`. `README.md`: in the flashing section replace any claim that the vault stages `.cff` files and add one line that the voltage is measured by the adapter. Keep the symlinked `CLAUDE.md` untouched.

- [ ] **Step 4: Verify and commit**

```bash
grep -rn "12\.6\|F191.*flash\|prefix" .agents/skills/safe-flashing/SKILL.md | head
cargo test --workspace 2>&1 | grep -E "^test result" | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
git add .agents/skills AGENTS.md README.md
git commit -m "docs: record the fail-closed flashing, transport and vault contracts

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

## Self-review

- **Spec coverage.** 4.2 step 1 → Task 1; step 2 → Task 2; step 3 (lock, keep-alive, every step `?`, parsers, manifest download, block echo, Failed messages, 423 guards, mock answers) → Tasks 3, 4, 5, 6; step 4 → Task 7; step 5 (F192 exact, `inspect_rom`, MCP verify) → Task 8; 4.1 Vault row and D9's vault half → Task 9; §6 client-supplied voltage → Task 10; Phase 0a residuals (inspect surfaces, runner keep-alive, OpenPort reopen) → Tasks 11–13; docs → Task 14. D8 (routine constants) is honoured in Task 5 and documented in Task 14. Out of scope by spec: `FlashState::Failed` remaining locked after erase, F191→F192 in `ModRunner`, SecurityAccess in `ModRunner`; the fabricated VIN fallbacks in service/coding routes are tracked for Phase 1.
- **Placeholders.** None; every step carries code, a command and an expected result. The two shadowed `happy_ecu()` lines in Task 5's tests are explicitly marked for removal.
- **Type consistency.** `ScriptedInterface` API (`rule`, `rule_once`, `rule_delayed`, `raw_frames`, `disconnected`, `sent_frames`, `sent_services`, `sent_handle`) is defined in Task 1 and used identically in Tasks 2, 4, 5, 7, 8, 12. `S3KeepAlive::{new, touch, tick}` (Task 4) used in Tasks 5 and 12. `parse_routine_status(resp, sub, id)`, `parse_request_download(resp)` (Task 4) used in Task 5. `hw_id_matches`/`read_supplier_hw_id` (Task 8). `cff::sniff` (Task 9) used by `vault_stage`. `choose_voltage`/`resolve_flash_voltage` (Task 10). `flash_write_refusals`/`writes_flash`/`ERASE_MEMORY_ROUTINE` (Task 11) replace the runner's private helpers. `VoltageCache`, `store_voltage_cache`, `read_voltage_cache`, `clear_voltage_cache` (Task 13).
- **Slow-ECU scripting.** `rule_delayed` always emits `7F <sid> 78` first, so every delayed reply in Tasks 2, 5 and 12 arrives inside the P2\* window (5000 ms + 500 ms) rather than the 1500 ms ISO-TP timeout; a plain `rule` with a lone `0x78` reply is how a test makes P2\* itself expire.
- **Known intermediate states.** After Task 3 the mock answers F192 in ASCII while preflight still ignores it (fixed in Task 8). After Task 5 the flasher's `run_preflight_checks` still uses the constant `hw_match = true` (Task 8 replaces it); the sequence tests in Task 5 script an F192 reply so they stay valid once Task 8 lands.
