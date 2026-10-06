use crate::ble::{Chunked, write_type_for};
use crate::error::BandError;
use crate::protocol::chunk::{ChunkDecoder, ChunkEncoder, ZeppMessage};
use btleplug::api::{Peripheral as _, ValueNotification};
use btleplug::platform::Peripheral;
use futures::Stream;
use futures::StreamExt;
use std::pin::Pin;
use std::time::Duration;

/// The Band 7 always uses the extended 2021 header.
const EXTENDED_HEADER: bool = true;

pub struct Session {
    peripheral: Peripheral,
    chunked: Chunked,
    notifications: Pin<Box<dyn Stream<Item = ValueNotification> + Send>>,
    decoder: ChunkDecoder,
    encoder: ChunkEncoder,
    raw: bool,
}

impl Session {
    pub async fn open(
        peripheral: Peripheral,
        chunked: Chunked,
        raw: bool,
        mtu_override: Option<u16>,
        subscribe_all: bool,
    ) -> Result<Self, BandError> {
        // Take the stream before subscribing so no early notification is lost.
        let notifications = peripheral.notifications().await?;

        peripheral.subscribe(&chunked.read).await?;
        if subscribe_all {
            for c in peripheral.characteristics() {
                if c.uuid == chunked.read.uuid {
                    continue;
                }
                if c.properties.contains(btleplug::api::CharPropFlags::NOTIFY) {
                    // A failure here is not fatal. Keep going.
                    let _ = peripheral.subscribe(&c).await;
                }
            }
        }

        let mtu = mtu_override.unwrap_or_else(|| peripheral.mtu().max(23));
        if raw {
            eprintln!("MTU in use: {mtu}");
        }

        Ok(Self {
            peripheral,
            chunked,
            notifications,
            decoder: ChunkDecoder::new(EXTENDED_HEADER),
            encoder: ChunkEncoder::new(mtu),
            raw,
        })
    }

    /// Write a value to one of the band's characteristics.
    ///
    /// Some things do not go through the chunked transport at all. The clock is
    /// one: it is the standard current time characteristic.
    pub async fn write_characteristic(
        &self,
        uuid: uuid::Uuid,
        value: &[u8],
    ) -> Result<(), BandError> {
        let found = self
            .peripheral
            .characteristics()
            .into_iter()
            .find(|c| c.uuid == uuid)
            .ok_or(BandError::MissingCharacteristic("current time"))?;
        let write_type = write_type_for(&found);
        self.peripheral.write(&found, value, write_type).await?;
        Ok(())
    }

    /// Read a value from one of the band's characteristics.
    pub async fn read_characteristic(&self, uuid: uuid::Uuid) -> Result<Vec<u8>, BandError> {
        let found = self
            .peripheral
            .characteristics()
            .into_iter()
            .find(|c| c.uuid == uuid)
            .ok_or(BandError::MissingCharacteristic("current time"))?;
        Ok(self.peripheral.read(&found).await?)
    }

    /// The band this session talks to.
    ///
    /// An install drives the firmware service on the same peripheral, so it
    /// borrows this rather than opening a second link.
    pub fn peripheral(&self) -> &Peripheral {
        &self.peripheral
    }

    /// The chunked read characteristic, which an install must unsubscribe.
    pub fn chunked_read(&self) -> &btleplug::api::Characteristic {
        &self.chunked.read
    }

    pub fn set_session_params(&mut self, sequence: u32, key: [u8; 16]) {
        self.encoder.set_encryption(sequence, key);
        self.decoder.set_session_key(key);
    }

    pub async fn send(
        &mut self,
        endpoint: u16,
        payload: &[u8],
        encrypt: bool,
    ) -> Result<(), BandError> {
        let chunks = self.encoder.encode(endpoint, payload, EXTENDED_HEADER, encrypt)?;
        let write_type = write_type_for(&self.chunked.write);
        for chunk in chunks {
            if self.raw {
                eprintln!("TX gatt=0016 raw={}", hex::encode(&chunk));
            }
            self.peripheral
                .write(&self.chunked.write, &chunk, write_type)
                .await?;
        }
        Ok(())
    }

    /// Pump notifications until one full message is reassembled.
    /// Acknowledgements are written back before the message is returned.
    pub async fn next_message(&mut self, timeout: Duration) -> Result<ZeppMessage, BandError> {
        loop {
            let notification = tokio::time::timeout(timeout, self.notifications.next())
                .await
                .map_err(|_| BandError::Timeout)?
                .ok_or(BandError::StreamClosed)?;

            if notification.uuid != self.chunked.read.uuid {
                if self.raw {
                    eprintln!(
                        "RX gatt={} raw={} (ignored)",
                        notification.uuid,
                        hex::encode(&notification.value)
                    );
                }
                continue;
            }

            if self.raw {
                eprintln!("RX gatt=0017 raw={}", hex::encode(&notification.value));
            }

            let outcome = match self.decoder.push(&notification.value) {
                Ok(outcome) => outcome,
                Err(err) => {
                    tracing::warn!("dropping packet: {err}");
                    continue;
                }
            };

            if let Some(ack) = outcome.ack {
                // Gadgetbridge writes the ack to the read characteristic. Keep that.
                let bytes = ack.to_bytes();
                let write_type = write_type_for(&self.chunked.read);
                self.peripheral
                    .write(&self.chunked.read, &bytes, write_type)
                    .await?;
                if self.raw {
                    eprintln!("TX gatt=0017 ack={}", hex::encode(bytes));
                }
            }

            if let Some(message) = outcome.message {
                if self.raw {
                    eprintln!(
                        "CHUNK endpoint={:#06x} encrypted={} payload={}",
                        message.endpoint,
                        message.encrypted,
                        hex::encode(&message.payload)
                    );
                }
                return Ok(message);
            }
        }
    }

    pub async fn disconnect(&self) -> Result<(), BandError> {
        self.peripheral.disconnect().await?;
        Ok(())
    }
}
