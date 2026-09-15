pub mod doip;
pub mod interface;
pub mod j2534;
pub mod mock;
pub mod openport;
pub mod openport_codec;
pub mod socketcan;

pub use doip::DoIpInterface;
pub use interface::VehicleInterface;
pub use j2534::J2534Interface;
pub use mock::VirtualCanInterface;
pub use openport::OpenPortInterface;
pub use socketcan::SocketCanInterface;

#[cfg(test)]
mod tests {
    use super::*;
    use sterngate_core::CanFrame;

    #[tokio::test]
    async fn test_virtual_can_lifecycle() {
        let mut sim = VirtualCanInterface::new();
        assert!(!sim.is_connected());
        sim.open().await.unwrap();
        assert!(sim.is_connected());

        // Send DiagnosticSessionControl Extended (0x10 03)
        let req = CanFrame::new_standard(0x7E0, &[0x02, 0x10, 0x03]);
        sim.send(req).await.unwrap();

        // Recv response from EDC16 (0x7E8)
        let resp = sim.recv().await.unwrap();
        assert_eq!(resp.id, 0x7E8);
        assert_eq!(resp.data[1], 0x50); // Positive response to 0x10
        assert_eq!(resp.data[2], 0x03);

        sim.close().await.unwrap();
        assert!(!sim.is_connected());
    }

    #[tokio::test]
    async fn test_virtual_can_read_egs_fluid_temp() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();

        // Send ReadDataByIdentifier for Transmission Fluid Temp (0x2001) to EGS52 (0x7E1)
        let req = CanFrame::new_standard(0x7E1, &[0x03, 0x22, 0x20, 0x01]);
        sim.send(req).await.unwrap();

        let resp = sim.recv().await.unwrap();
        assert_eq!(resp.id, 0x7E9);
        assert_eq!(resp.data[1], 0x62); // Positive response to 0x22
        assert_eq!(resp.data[2], 0x20);
        assert_eq!(resp.data[3], 0x01);
        assert_eq!(resp.data[4], 120); // 120 - 40 = 80°C (Exact target!)
    }

    #[tokio::test]
    async fn test_virtual_can_routine_control() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();

        // Send RoutineControl (0x31) Start (0x01) Fuel Pump Prime (0xFF01) to EDC16 (0x7E0)
        let req = CanFrame::new_standard(0x7E0, &[0x04, 0x31, 0x01, 0xFF, 0x01]);
        sim.send(req).await.unwrap();

        let resp = sim.recv().await.unwrap();
        assert_eq!(resp.id, 0x7E8);
        assert_eq!(resp.data[1], 0x71); // Positive response to 0x31
        assert_eq!(resp.data[2], 0x01); // sub-function startRoutine
        assert_eq!(resp.data[3], 0xFF); // routine high byte
        assert_eq!(resp.data[4], 0x01); // routine low byte
        assert_eq!(resp.data[5], 0x00); // routine status ok
    }

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

    /// Send a multi-frame request: FF, then read the FC, then the CFs.
    async fn send_multi(sim: &mut VirtualCanInterface, frames: &[&[u8]]) {
        sim.send(CanFrame::new_standard(0x7E0, frames[0]))
            .await
            .unwrap();
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
            &[
                &[0x10, 0x0B, 0x34, 0x00, 0x44, 0x00, 0x04, 0x00],
                &[0x21, 0x00, 0x00, 0x08, 0x00, 0x00],
            ],
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
            &[
                &[0x10, 0x0A, 0x36, 0x07, 0xAA, 0xBB, 0xCC, 0xDD],
                &[0x21, 0xEE, 0xFF, 0x11, 0x22],
            ],
        )
        .await;
        let resp = recv_diag(&mut sim).await;
        assert_eq!(&resp.data[..3], &[0x02, 0x76, 0x07]);
    }

    #[tokio::test]
    async fn test_virtual_can_answers_transfer_exit_and_bus_control() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();
        sim.send(CanFrame::new_standard(0x7E0, &[0x01, 0x37]))
            .await
            .unwrap();
        assert_eq!(&recv_diag(&mut sim).await.data[..2], &[0x01, 0x77]);
        sim.send(CanFrame::new_standard(0x7E0, &[0x03, 0x28, 0x01, 0x01]))
            .await
            .unwrap();
        assert_eq!(&recv_diag(&mut sim).await.data[..3], &[0x02, 0x68, 0x01]);
        sim.send(CanFrame::new_standard(0x7E0, &[0x02, 0x85, 0x02]))
            .await
            .unwrap();
        assert_eq!(&recv_diag(&mut sim).await.data[..3], &[0x02, 0xC5, 0x02]);
    }

    #[tokio::test]
    async fn test_virtual_can_suppresses_tester_present_response() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();
        sim.send(CanFrame::new_standard(0x7E0, &[0x02, 0x3E, 0x80]))
            .await
            .unwrap();
        // Only background broadcast frames may arrive; no 0x7E8 diagnostic reply.
        let got =
            tokio::time::timeout(std::time::Duration::from_millis(150), recv_diag(&mut sim)).await;
        assert!(
            got.is_err(),
            "suppressed TesterPresent must not be answered"
        );
        sim.send(CanFrame::new_standard(0x7E0, &[0x02, 0x3E, 0x00]))
            .await
            .unwrap();
        assert_eq!(&recv_diag(&mut sim).await.data[..3], &[0x02, 0x7E, 0x00]);
    }

    #[tokio::test]
    async fn test_virtual_can_identification_is_multi_frame_ascii() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();
        sim.send(CanFrame::new_standard(0x7E0, &[0x03, 0x22, 0xF1, 0x92]))
            .await
            .unwrap();
        let ff = recv_diag(&mut sim).await;
        assert_eq!(&ff.data[..5], &[0x10, 0x0D, 0x62, 0xF1, 0x92]);
        // Nothing more until the tester sends Flow Control.
        let early =
            tokio::time::timeout(std::time::Duration::from_millis(100), recv_diag(&mut sim)).await;
        assert!(early.is_err());
        sim.send(CanFrame::new_standard(0x7E0, &[0x30, 0x00, 0x05]))
            .await
            .unwrap();
        let cf = recv_diag(&mut sim).await;
        assert_eq!(cf.data[0], 0x21);
        let mut text = ff.data[5..8].to_vec();
        text.extend_from_slice(&cf.data[1..8]);
        assert_eq!(text, b"0281012224");
    }

    /// The Pin-16 cache freshness rule, tested without USB hardware.
    #[test]
    fn test_fresh_reading_window() {
        use crate::openport::fresh_reading;
        use std::time::{Duration, Instant};

        let max_age = Duration::from_millis(500);
        let measured_at = Instant::now();

        // Young reading: reused as-is.
        let volts = fresh_reading(Some((12.6, measured_at)), measured_at, max_age)
            .expect("a reading taken now is fresh");
        assert!((volts - 12.6).abs() < f32::EPSILON);

        // Exactly at the age limit still counts as fresh.
        assert!(fresh_reading(Some((12.6, measured_at)), measured_at + max_age, max_age).is_some());

        // Stale reading: the caller must issue a new query.
        assert!(fresh_reading(
            Some((12.6, measured_at)),
            measured_at + Duration::from_millis(600),
            max_age
        )
        .is_none());

        // Empty cache: nothing has ever been measured.
        assert!(fresh_reading(None, measured_at, max_age).is_none());
    }
}
