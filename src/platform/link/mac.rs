//! The link on macOS: a Unix socket in BandBridge's folder.

use crate::error::BandError;
use std::path::PathBuf;
use tokio::net::{UnixListener, UnixStream};

/// The word a message uses for the link.
pub const CALLED: &str = "socket";

/// Where the holder listens.
///
/// In BandBridge's folder, beside the config, so it is easy to find and easy
/// to delete by hand when a process dies without cleaning up. A socket's path
/// must fit in 104 bytes on macOS, with the byte that ends it. The folder is
/// deeper than it was, so a test checks that the path still fits.
pub fn socket_path() -> Option<PathBuf> {
    Some(crate::config::folder()?.join("bandbridge.sock"))
}

/// The socket the holder listens on.
pub struct Listener(UnixListener);

/// Listen on the socket, for the process that holds the band.
///
/// A stale socket file is removed first: a process that dies without cleaning up
/// would otherwise lock the band out of a socket for good.
pub async fn listen() -> Result<Listener, BandError> {
    let Some(path) = socket_path() else {
        return Err(BandError::Config("no home directory".to_string()));
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if path.exists() && UnixStream::connect(&path).await.is_err() {
        let _ = std::fs::remove_file(&path);
    }

    let listener = UnixListener::bind(&path)
        .map_err(|err| BandError::Config(format!("cannot listen on {}: {err}", path.display())))?;
    Ok(Listener(listener))
}

impl Listener {
    /// The next caller.
    pub async fn accept(&mut self) -> std::io::Result<UnixStream> {
        self.0.accept().await.map(|(stream, _)| stream)
    }
}

/// The holder's socket, or None when nothing listens on it.
pub async fn connect() -> Option<UnixStream> {
    UnixStream::connect(socket_path()?).await.ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A socket's path must fit in the 104 bytes macOS gives it, with the byte
    /// that ends it, or the holder cannot listen. The folder is deeper than
    /// the old one, so check the path, and the room it leaves for a home
    /// folder longer than this one.
    #[test]
    fn the_socket_path_stays_under_104_bytes() {
        let path = socket_path().expect("a home folder");
        assert!(path.ends_with("BandBridge/bandbridge.sock"), "{}", path.display());
        let bytes = path.as_os_str().len();
        assert!(bytes < 104, "{} is {bytes} bytes", path.display());

        // The same path under `/Users/` and a name of 32 letters.
        let home = dirs::home_dir().expect("a home folder");
        if let Ok(rest) = path.strip_prefix(&home) {
            let longer = "/Users/".len() + 32 + "/".len() + rest.as_os_str().len();
            assert!(longer < 104, "a longer home folder makes it {longer} bytes");
        }
    }
}
