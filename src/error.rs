use thiserror::Error;

#[derive(Debug, Error)]
pub enum BandError {
    #[error("invalid auth key: {0}")]
    AuthKey(String),

    /// No key anywhere BandBridge looks. The reason says where it looked, and
    /// names the band when one is paired.
    #[error("no auth key: {0}")]
    MissingAuthKey(String),

    /// The system's secret store could not keep, read or delete a key. The
    /// message names the band, and never holds the key.
    #[error("{0}")]
    KeyStore(String),

    #[error("bluetooth error: {0}")]
    Bluetooth(#[from] btleplug::Error),

    #[error("no bluetooth adapter found")]
    NoAdapter,

    #[error("band not found")]
    BandNotFound,

    /// Pairing looked for bands for the whole of its search, and found none.
    #[error(
        "no band is nearby. Wake the band and keep it close to {}. If your phone is connected to \
         it, turn off the phone's Bluetooth, then try again.",
        crate::platform::this_computer!()
    )]
    NoBandNearby,

    /// Pairing found bands, and every one it could ask refused the key. The
    /// reason counts them. It never holds the key.
    #[error("no band nearby accepts this key: {0}")]
    KeyRefused(String),

    /// Pairing found bands, and could ask none of them for the key. The
    /// reason counts them and says why the last could not be asked.
    #[error("{0}")]
    BandUnreachable(String),

    /// The band that answered is not the paired band. Nothing was sent to it.
    #[error(
        "the band that answered is {found}, not the paired band, {paired}. Mi Band Bridge let \
         it go, and stopped trying. Connect to try again."
    )]
    WrongBand { paired: String, found: String },

    /// Pairing or forgetting cannot go ahead now. The reason says why.
    #[error("{0}")]
    Pairing(String),

    #[error("characteristic {0} not found on the band")]
    MissingCharacteristic(&'static str),

    #[error("timed out waiting for the band")]
    Timeout,

    #[error("display list: {0}")]
    DisplayList(String),

    #[error("notification stream closed")]
    StreamClosed,

    #[error("config error: {0}")]
    Config(String),

    /// The system gave no adapter within 20 seconds. On macOS that is a
    /// Bluetooth permission not given yet. Windows asks none.
    #[cfg_attr(
        target_os = "macos",
        error(
            "bluetooth adapter did not become available within 20s -- on macOS, grant Bluetooth permission to your terminal in System Settings > Privacy & Security > Bluetooth"
        )
    )]
    #[cfg_attr(
        windows,
        error("Bluetooth did not start within 20 seconds. Turn Bluetooth on, then try again.")
    )]
    BluetoothUnavailable,

    #[error("protocol error: {0}")]
    Protocol(#[from] crate::protocol::ProtocolError),

    #[error("install failed: {0}")]
    Install(String),

    #[error("apps: {0}")]
    Apps(String),

    #[error("device: {0}")]
    Device(String),

    #[error("refusing to send {0} bytes: a Zepp OS app is kilobytes, and this module must never send firmware")]
    FileTooLarge(u64),
}
