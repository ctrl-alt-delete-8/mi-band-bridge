//! Installing a Zepp OS app onto the band.
//!
//! This does NOT use the Zepp 2021 chunked transport. It uses the older Huami
//! firmware service: control commands and replies on characteristic 0x1531,
//! raw payload bytes on 0x1532.

use crate::ble::{Firmware, write_type_for};
use crate::error::BandError;
use btleplug::api::Peripheral as _;
use btleplug::platform::Peripheral;
use futures::StreamExt;
use std::time::Duration;

/// Ask the band for its transfer parameters, including the chunk size.
pub const CMD_REQUEST_PARAMETERS: u8 = 0xd0;
/// Declare what we are about to send.
pub const CMD_SEND_FIRMWARE_INFO: u8 = 0xd2;
/// Begin the payload stream.
pub const CMD_START_TRANSFER: u8 = 0xd3;
/// Sent BY THE BAND when it has received the whole payload.
pub const CMD_COMPLETE_TRANSFER: u8 = 0xd5;
/// Ask the band to install what it received.
pub const CMD_FINALIZE_UPDATE: u8 = 0xd6;
/// Sent BY THE BAND during the transfer. Not an error, and judged by length.
pub const CMD_UPDATE_PROGRESS: u8 = 0xd4;
/// Firmware type for a Zepp OS app.
///
/// This is the ONLY type this module may ever declare. Type 0x00 is firmware:
/// the same protocol flashes it and reboots the band.
pub const TYPE_ZEPPOS_APP: u8 = 0xa0;

/// Control replies start with this byte.
pub const RESPONSE: u8 = 0x10;
/// Third byte of a successful reply.
pub const SUCCESS: u8 = 0x01;
/// The band has no room for the file.
pub const ERR_FREE_SPACE: u8 = 0x47;
/// The band's battery is too low to install.
pub const ERR_LOW_BATTERY: u8 = 0x22;

/// Refuse anything larger than this. A Zepp OS app is kilobytes; firmware is
/// hundreds of megabytes. This is a guard against pointing `install` at a
/// firmware image by mistake.
pub const MAX_APP_BYTES: u64 = 5 * 1024 * 1024;



/// Build the 19-byte command that declares the file to the band.
///
/// The type is hard-coded to an app. There is deliberately no parameter for it.
pub fn firmware_info_command(size: u32, crc32: u32, chunk_size: u16) -> [u8; 19] {
    let size_bytes = size.to_le_bytes();
    let crc_bytes = crc32.to_le_bytes();
    let chunk_bytes = chunk_size.to_le_bytes();

    let mut cmd = [0u8; 19];
    cmd[0] = CMD_SEND_FIRMWARE_INFO;
    cmd[1] = TYPE_ZEPPOS_APP;
    cmd[2..6].copy_from_slice(&size_bytes);
    cmd[6..10].copy_from_slice(&crc_bytes);
    cmd[10..12].copy_from_slice(&chunk_bytes);
    cmd[12] = 0x00; // 0 installs in the foreground, 1 in the background
    cmd[13] = 0x00; // index within the batch
    cmd[14] = 0x01; // count: a batch of one
    cmd[15..19].copy_from_slice(&size_bytes); // total across the batch
    cmd
}

/// Split the payload into transfer-sized pieces, in order.
pub fn chunks(data: &[u8], chunk_size: usize) -> Vec<&[u8]> {
    if chunk_size == 0 {
        return Vec::new();
    }
    data.chunks(chunk_size).collect()
}

/// How much of an upload has gone, in whole percent, rounded down, so it
/// says 100 only once the last byte has gone.
pub fn percent(sent: usize, total: usize) -> usize {
    sent.min(total) * 100 / total.max(1)
}

/// What a caller who asked for an install hears of the upload: a line each
/// time its percentage moves, such as `62%`, as the command line prints it.
/// The band app editor reads these lines as its progress.
#[derive(Debug, Default)]
pub struct ProgressLines {
    said: Option<usize>,
}

impl ProgressLines {
    /// The line for this much sent, or None while the percentage is the one
    /// said last.
    pub fn line(&mut self, sent: usize, total: usize) -> Option<String> {
        let now = percent(sent, total);
        if self.said == Some(now) {
            return None;
        }
        self.said = Some(now);
        Some(format!("{now}%"))
    }
}

#[cfg(test)]
mod command_tests {
    use super::*;

    #[test]
    fn builds_the_nineteen_byte_firmware_info_command() {
        // size 0x00000900 = 2304, crc 0xdeadbeef, chunk size 0x00f4 = 244
        let cmd = firmware_info_command(2304, 0xdead_beef, 244);
        assert_eq!(cmd.len(), 19);
        assert_eq!(cmd[0], CMD_SEND_FIRMWARE_INFO);
        assert_eq!(cmd[1], TYPE_ZEPPOS_APP, "install must only ever declare an app");
        assert_eq!(&cmd[2..6], &2304u32.to_le_bytes());
        assert_eq!(&cmd[6..10], &0xdead_beefu32.to_le_bytes());
        assert_eq!(&cmd[10..12], &244u16.to_le_bytes());
        assert_eq!(cmd[12], 0x00, "0 installs in the foreground");
        assert_eq!(cmd[13], 0x00, "index");
        assert_eq!(cmd[14], 0x01, "count: a batch of one");
        assert_eq!(&cmd[15..19], &2304u32.to_le_bytes(), "total size repeats the size");
    }

    #[test]
    fn the_app_type_is_never_firmware() {
        // Guard against a future edit widening this. 0x00 flashes firmware.
        assert_eq!(TYPE_ZEPPOS_APP, 0xa0);
        assert_ne!(TYPE_ZEPPOS_APP, 0x00);
    }

    #[test]
    fn chunks_an_exact_multiple() {
        let data = [0u8; 12];
        let out = chunks(&data, 4);
        assert_eq!(out.len(), 3);
        assert!(out.iter().all(|c| c.len() == 4));
    }

    #[test]
    fn chunks_with_a_trailing_partial() {
        let data = [0u8; 10];
        let out = chunks(&data, 4);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].len(), 4);
        assert_eq!(out[1].len(), 4);
        assert_eq!(out[2].len(), 2);
    }

    #[test]
    fn a_file_smaller_than_one_chunk_is_a_single_chunk() {
        let data = [1u8, 2, 3];
        let out = chunks(&data, 244);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], &[1, 2, 3]);
    }

    #[test]
    fn chunking_preserves_every_byte_in_order() {
        let data: Vec<u8> = (0..=255u8).collect();
        let joined: Vec<u8> = chunks(&data, 7).concat();
        assert_eq!(joined, data);
    }
}

#[cfg(test)]
mod progress_tests {
    use super::*;

    #[test]
    fn the_percentage_is_whole_and_reaches_100_only_at_the_last_byte() {
        assert_eq!(percent(0, 48_213), 0);
        assert_eq!(percent(30_100, 48_213), 62);
        assert_eq!(percent(48_212, 48_213), 99);
        assert_eq!(percent(48_213, 48_213), 100);
        // Nothing to send is not a division by zero.
        assert_eq!(percent(0, 0), 0);
    }

    /// A caller hears the upload as the command line prints it, one line
    /// each time the percentage moves, and never the same one twice in a row.
    #[test]
    fn a_line_goes_out_each_time_the_percentage_moves() {
        let mut lines = ProgressLines::default();
        let heard: Vec<String> = [244, 488, 30_100, 30_344, 48_000, 48_213]
            .into_iter()
            .filter_map(|sent| lines.line(sent, 48_213))
            .collect();
        assert_eq!(heard, ["0%", "1%", "62%", "99%", "100%"]);
    }
}

/// A control reply from the band.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// The band stated its transfer parameters.
    Parameters { chunk_size: u16 },
    /// The band acknowledged the named command.
    Ack(u8),
    /// A progress report sent during the transfer. Not an error.
    Progress { offset: u32 },
    /// The band refused, with a reason fit to show the user.
    Failed(String),
    /// Not a control reply we care about.
    Ignored,
}

/// Parse one notification from the control characteristic.
pub fn parse_reply(value: &[u8]) -> Reply {
    tracing::debug!("control notification: {}", hex::encode(value));
    if value.len() < 3 || value[0] != RESPONSE {
        return Reply::Ignored;
    }
    let command = value[1];
    let status = value[2];

    // 0xd4 breaks the status rule: it is valid when long enough. Match it
    // before the status check, or a working transfer looks like a failure.
    if command == CMD_UPDATE_PROGRESS {
        return if value.len() >= 6 {
            // Bytes 2..6 are how much the band has taken. It is the offset the
            // next chunk must start at, and the band drives the transfer with
            // it. Do not send the next chunk before this arrives.
            let offset = u32::from_le_bytes([value[2], value[3], value[4], value[5]]);
            Reply::Progress { offset }
        } else {
            Reply::Failed("truncated progress report".to_string())
        };
    }

    if status != SUCCESS {
        return Reply::Failed(describe_error(command, status));
    }

    if command == CMD_REQUEST_PARAMETERS {
        if value.len() < 6 {
            return Reply::Failed("truncated parameters reply".to_string());
        }
        let chunk_size = u16::from_le_bytes([value[4], value[5]]);
        return Reply::Parameters { chunk_size };
    }

    Reply::Ack(command)
}

fn describe_error(command: u8, status: u8) -> String {
    match status {
        ERR_FREE_SPACE => "the band does not have enough free space".to_string(),
        ERR_LOW_BATTERY => "the band's battery is too low to install".to_string(),
        other => format!("the band refused command {command:#04x} with status {other:#04x}"),
    }
}

/// Where the transfer has got to.
///
/// `Declared` and `Sending` carry the chunk size the band reported, so the
/// driver never has to remember it separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Declared { chunk_size: u16 },
    Sending { chunk_size: u16 },
    Completing,
    Done,
}

/// What the driver should do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Write these bytes to the control characteristic.
    Send(Vec<u8>),
    /// Write one chunk of the payload, starting at this offset.
    SendChunk { offset: u32, chunk_size: u16 },
    /// The app is installed.
    Finish,
    /// Give up, with a reason fit to show the user.
    Fail(String),
    /// Nothing to do. Wait for the next reply.
    Wait,
}

/// Decide the next step from the current phase and the band's reply.
///
/// Pure, so the whole sequence is testable without Bluetooth. The band drives:
/// every command except the first is a response to a reply.
///
/// `size` and `crc32` describe the file; they are needed to build the info
/// command when the band states its parameters.
pub fn next_step(phase: Phase, reply: &Reply, size: u32, crc32: u32) -> (Phase, Action) {
    if let Reply::Failed(message) = reply {
        return (Phase::Done, Action::Fail(message.clone()));
    }

    match (phase, reply) {
        (Phase::Idle, Reply::Parameters { chunk_size }) => (
            Phase::Declared { chunk_size: *chunk_size },
            Action::Send(firmware_info_command(size, crc32, *chunk_size).to_vec()),
        ),
        (Phase::Declared { chunk_size }, Reply::Ack(CMD_SEND_FIRMWARE_INFO)) => (
            Phase::Sending { chunk_size },
            Action::Send(vec![CMD_START_TRANSFER, 0x01]),
        ),
        (Phase::Sending { chunk_size }, Reply::Ack(CMD_START_TRANSFER)) => (
            Phase::Sending { chunk_size },
            Action::SendChunk { offset: 0, chunk_size },
        ),
        // The band reports how much it has taken and thereby asks for the rest.
        // Announcing completion before it has everything is refused at 0xd5.
        (Phase::Sending { chunk_size }, Reply::Progress { offset }) => {
            if *offset >= size {
                (Phase::Sending { chunk_size }, Action::Send(vec![CMD_COMPLETE_TRANSFER]))
            } else {
                (
                    Phase::Sending { chunk_size },
                    Action::SendChunk { offset: *offset, chunk_size },
                )
            }
        }
        (Phase::Sending { .. }, Reply::Ack(CMD_COMPLETE_TRANSFER)) => {
            (Phase::Completing, Action::Send(vec![CMD_FINALIZE_UPDATE]))
        }
        (Phase::Completing, Reply::Ack(CMD_FINALIZE_UPDATE)) => (Phase::Done, Action::Finish),
        _ => (phase, Action::Wait),
    }
}

#[cfg(test)]
mod reply_tests {
    use super::*;

    #[test]
    fn reads_the_chunk_size_from_the_parameters_reply() {
        // 0x10 d0 01 <pad> f4 00  -> chunk size 244, little-endian at [4..6]
        let reply = parse_reply(&[RESPONSE, CMD_REQUEST_PARAMETERS, SUCCESS, 0x00, 0xf4, 0x00]);
        assert_eq!(reply, Reply::Parameters { chunk_size: 244 });
    }

    #[test]
    fn a_short_parameters_reply_is_a_failure_not_a_panic() {
        let reply = parse_reply(&[RESPONSE, CMD_REQUEST_PARAMETERS, SUCCESS]);
        assert!(matches!(reply, Reply::Failed(_)));
    }

    #[test]
    fn acknowledges_each_command_by_echo() {
        assert_eq!(
            parse_reply(&[RESPONSE, CMD_SEND_FIRMWARE_INFO, SUCCESS]),
            Reply::Ack(CMD_SEND_FIRMWARE_INFO)
        );
        assert_eq!(
            parse_reply(&[RESPONSE, CMD_START_TRANSFER, SUCCESS]),
            Reply::Ack(CMD_START_TRANSFER)
        );
        assert_eq!(
            parse_reply(&[RESPONSE, CMD_COMPLETE_TRANSFER, SUCCESS]),
            Reply::Ack(CMD_COMPLETE_TRANSFER)
        );
    }

    /// The band sends 0xd4 DURING the transfer. It is judged by length, not by
    /// the status byte. Treating it as a failure would abort a working install.
    #[test]
    fn progress_updates_are_not_failures() {
        assert_eq!(
            parse_reply(&[RESPONSE, CMD_UPDATE_PROGRESS, 0x00, 0x00, 0x00, 0x00]),
            Reply::Progress { offset: 0 }
        );
    }

    #[test]
    fn a_short_progress_reply_is_a_failure() {
        assert!(matches!(
            parse_reply(&[RESPONSE, CMD_UPDATE_PROGRESS, 0x00]),
            Reply::Failed(_)
        ));
    }

    #[test]
    fn reports_the_two_known_errors_in_words() {
        let space = parse_reply(&[RESPONSE, CMD_SEND_FIRMWARE_INFO, ERR_FREE_SPACE]);
        match space {
            Reply::Failed(message) => assert!(
                message.contains("space"),
                "expected a message about free space, got {message}"
            ),
            other => panic!("expected Failed, got {other:?}"),
        }

        let battery = parse_reply(&[RESPONSE, CMD_SEND_FIRMWARE_INFO, ERR_LOW_BATTERY]);
        match battery {
            Reply::Failed(message) => assert!(
                message.contains("battery"),
                "expected a message about the battery, got {message}"
            ),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn ignores_a_notification_that_is_not_a_control_reply() {
        assert_eq!(parse_reply(&[0x00, 0x01]), Reply::Ignored);
        assert_eq!(parse_reply(&[]), Reply::Ignored);
    }
}

#[cfg(test)]
mod state_tests {
    use super::*;

    const SIZE: u32 = 2304;
    const CRC: u32 = 0xdead_beef;

    #[test]
    fn walks_the_whole_successful_sequence() {
        // Parameters -> declare the file, carrying the band's chunk size forward
        let (phase, action) = next_step(Phase::Idle, &Reply::Parameters { chunk_size: 244 }, SIZE, CRC);
        assert_eq!(phase, Phase::Declared { chunk_size: 244 });
        match action {
            Action::Send(bytes) => {
                assert_eq!(bytes.len(), 19, "must be the full info command");
                assert_eq!(bytes[0], CMD_SEND_FIRMWARE_INFO);
                assert_eq!(bytes[1], TYPE_ZEPPOS_APP);
                assert_eq!(&bytes[2..6], &SIZE.to_le_bytes());
                assert_eq!(&bytes[6..10], &CRC.to_le_bytes());
                assert_eq!(&bytes[10..12], &244u16.to_le_bytes());
            }
            other => panic!("expected the info command, got {other:?}"),
        }

        // Info acknowledged -> start the transfer
        let (phase, action) = next_step(
            Phase::Declared { chunk_size: 244 },
            &Reply::Ack(CMD_SEND_FIRMWARE_INFO),
            SIZE,
            CRC,
        );
        assert_eq!(phase, Phase::Sending { chunk_size: 244 });
        assert_eq!(action, Action::Send(vec![CMD_START_TRANSFER, 0x01]));

        // Start acknowledged -> stream the payload at the band's chunk size
        let (phase, action) = next_step(
            Phase::Sending { chunk_size: 244 },
            &Reply::Ack(CMD_START_TRANSFER),
            SIZE,
            CRC,
        );
        assert_eq!(phase, Phase::Sending { chunk_size: 244 });
        assert_eq!(
            action,
            Action::SendChunk {
                offset: 0,
                chunk_size: 244
            }
        );

        // Band says it has everything -> finalize
        let (phase, action) = next_step(
            Phase::Sending { chunk_size: 244 },
            &Reply::Ack(CMD_COMPLETE_TRANSFER),
            SIZE,
            CRC,
        );
        assert_eq!(phase, Phase::Completing);
        assert_eq!(action, Action::Send(vec![CMD_FINALIZE_UPDATE]));

        // Finalize acknowledged -> done
        let (phase, action) = next_step(Phase::Completing, &Reply::Ack(CMD_FINALIZE_UPDATE), SIZE, CRC);
        assert_eq!(phase, Phase::Done);
        assert_eq!(action, Action::Finish);
    }

    #[test]
    fn a_failure_reply_ends_the_transfer_from_any_phase() {
        let phases = [
            Phase::Idle,
            Phase::Declared { chunk_size: 244 },
            Phase::Sending { chunk_size: 244 },
            Phase::Completing,
        ];
        for phase in phases {
            let (next, action) = next_step(phase, &Reply::Failed("no space".to_string()), SIZE, CRC);
            assert_eq!(next, Phase::Done, "failed from {phase:?}");
            assert!(matches!(action, Action::Fail(_)));
        }
    }

    /// The band drives the transfer: each progress report asks for the chunk at
    /// that offset. Announcing completion before the band has everything is
    /// refused at 0xd5, which is what a whole-payload blast produced.
    #[test]
    fn progress_asks_for_the_chunk_at_that_offset() {
        let sending = Phase::Sending { chunk_size: 244 };

        let (phase, action) = next_step(sending, &Reply::Progress { offset: 488 }, SIZE, CRC);
        assert_eq!(phase, sending);
        assert_eq!(
            action,
            Action::SendChunk {
                offset: 488,
                chunk_size: 244
            }
        );
    }

    /// Only once the band reports it holds the whole file does completion go out.
    #[test]
    fn completion_waits_until_the_band_has_everything() {
        let sending = Phase::Sending { chunk_size: 244 };
        let (phase, action) = next_step(sending, &Reply::Progress { offset: SIZE }, SIZE, CRC);
        assert_eq!(phase, sending);
        assert_eq!(action, Action::Send(vec![CMD_COMPLETE_TRANSFER]));
    }

    #[test]
    fn ignored_replies_do_not_move_the_state_machine() {
        let sending = Phase::Sending { chunk_size: 244 };

        let (phase, action) = next_step(sending, &Reply::Ignored, SIZE, CRC);
        assert_eq!(phase, sending);
        assert_eq!(action, Action::Wait);
    }

    #[test]
    fn an_unexpected_acknowledgement_is_waited_on_not_acted_on() {
        // A stray CMD_FINALIZE_UPDATE ack while still sending must not finish.
        let sending = Phase::Sending { chunk_size: 244 };
        let (phase, action) = next_step(sending, &Reply::Ack(CMD_FINALIZE_UPDATE), SIZE, CRC);
        assert_eq!(phase, sending);
        assert_eq!(action, Action::Wait);
    }
}

/// Fail rather than hang if the band goes quiet mid-transfer.
const REPLY_TIMEOUT: Duration = Duration::from_secs(30);

/// Upload an app and ask the band to install it.
///
/// The band drives the sequence: each reply triggers the next command.
/// `progress` hears how many bytes have gone, of how many, after each chunk.
pub async fn run(
    peripheral: &Peripheral,
    fw: &Firmware,
    chunked_read: &btleplug::api::Characteristic,
    bytes: &[u8],
    mut progress: impl FnMut(usize, usize),
) -> Result<(), BandError> {
    if bytes.len() as u64 > MAX_APP_BYTES {
        return Err(BandError::FileTooLarge(bytes.len() as u64));
    }

    let size = bytes.len() as u32;
    let crc32 = crc32fast::hash(bytes);

    let mut notifications = peripheral.notifications().await?;

    // The 2021 chunked transport MUST be quiet during a firmware transfer.
    // The reference implementation disables these notifications first, with the
    // comment "otherwise firmware upgrades and activity sync get interrupted".
    // Leaving them on makes the band refuse the very first command with 0x02.
    if let Err(err) = peripheral.unsubscribe(chunked_read).await {
        tracing::warn!("could not quiet the chunked transport: {err}");
    }

    peripheral.subscribe(&fw.control).await?;

    let control_write = write_type_for(&fw.control);
    let data_write = write_type_for(&fw.data);

    println!("Requesting transfer parameters...");
    peripheral
        .write(&fw.control, &[CMD_REQUEST_PARAMETERS], control_write)
        .await?;

    let mut phase = Phase::Idle;

    loop {
        let notification = tokio::time::timeout(REPLY_TIMEOUT, notifications.next())
            .await
            .map_err(|_| BandError::Install("the band stopped replying".to_string()))?
            .ok_or(BandError::StreamClosed)?;

        if notification.uuid != fw.control.uuid {
            continue;
        }

        let reply = parse_reply(&notification.value);
        if let Reply::Parameters { chunk_size } = &reply {
            println!("Band chunk size: {chunk_size}");
        }
        let (next, action) = next_step(phase, &reply, size, crc32);
        phase = next;

        match action {
            Action::Send(command) => {
                peripheral.write(&fw.control, &command, control_write).await?;
            }
            Action::SendChunk { offset, chunk_size } => {
                let sent =
                    send_chunk(peripheral, fw, bytes, offset, chunk_size, data_write).await?;
                progress(sent, bytes.len());
            }
            Action::Finish => {
                println!("Installed.");
                return Ok(());
            }
            Action::Fail(message) => return Err(BandError::Install(message)),
            Action::Wait => {}
        }
    }
}

/// Send the chunk the band asked for, and answer how many bytes have gone:
/// all of them up to its end.
async fn send_chunk(
    peripheral: &Peripheral,
    fw: &Firmware,
    bytes: &[u8],
    offset: u32,
    chunk_size: u16,
    data_write: btleplug::api::WriteType,
) -> Result<usize, BandError> {
    // Two different sizes matter. The band's chunk size is the logical unit it
    // asks for; the actual BLE write must fit an ATT packet, which is MTU minus
    // the 3-byte ATT header. Writing a whole logical chunk in one call would be
    // truncated.
    let mtu = peripheral.mtu().max(23) as usize;
    let packet_len = (mtu - 3).max(1);

    let start = offset as usize;
    let end = (start + chunk_size as usize).min(bytes.len());
    let chunk = &bytes[start..end];

    for piece in chunks(chunk, packet_len) {
        peripheral.write(&fw.data, piece, data_write).await?;
    }

    println!("  {}%", percent(end, bytes.len()));
    Ok(end)
}
