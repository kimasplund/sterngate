//! Test double for transport and flasher tests: answers UDS requests from a
//! script, records every frame the tester sent, and can delay replies so
//! paused-time tests can exercise timeouts and keep-alives.
//!
//! This is a shared fixture: later Phase 0b tasks exercise `disconnected`,
//! `rule_once`, `rule_delayed`, `sent_services`, `sent_handle`, and
//! `TESTER_TX_ID`, so this task's tests alone don't call every item yet.
#![allow(dead_code)]

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

    /// Like `rule_delayed`, but consumed after its first use, so consecutive
    /// requests for one service can be answered differently (a TransferData
    /// acknowledgement echoes the block counter it was sent).
    pub(crate) fn rule_delayed_once(mut self, sid: u8, delay: Duration, replies: &[&[u8]]) -> Self {
        self.push_rule(sid, replies, delay, true);
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
            return Err(SterngateError::DeviceNotFound(
                "scripted interface is disconnected".into(),
            ));
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
        // A tester First Frame gets Flow Control before the reply, as a real ECU
        // sends it -- otherwise the tester never emits its Consecutive Frames and
        // every multi-frame request (0x34, a 0x36 block) times out. A script whose
        // first reply is itself a flow-control frame drives flow control by hand.
        let is_first_frame = frame.data.first().is_some_and(|b| b >> 4 == 0x1);
        let script_drives_flow_control = replies
            .first()
            .and_then(|r| r.first())
            .is_some_and(|b| b >> 4 == 0x3);
        if is_first_frame && !script_drives_flow_control {
            self.pending.push_back((
                Duration::ZERO,
                vec![0x30, 0x00, 0x00, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA],
            ));
        }
        if !delay.is_zero() {
            // A compliant ECU that needs longer than P2 answers ResponsePending first.
            self.pending
                .push_back((Duration::ZERO, vec![0x03, 0x7F, sid, 0x78]));
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
