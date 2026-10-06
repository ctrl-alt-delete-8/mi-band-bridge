use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "bandbridge", about = "Xiaomi Smart Band 7 remote for macOS")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// List nearby peripherals and their advertised names.
    Scan {
        /// How long to scan, in seconds.
        #[arg(long, default_value_t = 6)]
        seconds: u64,
    },
    /// Connect to one peripheral and print its services and characteristics.
    Inspect {
        /// The btleplug peripheral id shown by `scan`.
        peripheral_id: String,

        /// How long to scan before giving up, in seconds.
        #[arg(long, default_value_t = 20)]
        seconds: u64,
    },
    /// Authenticate and print band events.
    Listen {
        /// The btleplug peripheral id. Omit to pick the first band by name.
        #[arg(long)]
        peripheral_id: Option<String>,

        /// Print every GATT notification and decoded chunk.
        #[arg(long, default_value_t = false)]
        raw: bool,

        /// Override the negotiated MTU. Use only when the negotiated value is wrong.
        #[arg(long)]
        mtu: Option<u16>,

        /// Subscribe to every notifying characteristic, not only 0x17.
        #[arg(long, default_value_t = false)]
        subscribe_all: bool,

        /// Do not reconnect automatically after an unexpected drop.
        /// Overrides auto_reconnect in the config file.
        #[arg(long, default_value_t = false)]
        no_auto_reconnect: bool,

        /// How long to scan before giving up, in seconds.
        #[arg(long, default_value_t = 20)]
        seconds: u64,
    },
    /// List the apps installed on the band, or remove one.
    Apps {
        /// The id of an app to remove, as the list shows it, such as 58E538D5.
        /// Without this, the list is printed and nothing changes.
        #[arg(long)]
        remove: Option<String>,

        /// Show every package the band holds, including the watch face and
        /// whatever came with the band. Those cannot be named or safely
        /// removed, so they are left out by default.
        #[arg(long, default_value_t = false)]
        all: bool,

        /// The btleplug peripheral id. Omit to pick the first band by name.
        #[arg(long)]
        peripheral_id: Option<String>,

        /// How long to scan before giving up, in seconds.
        #[arg(long, default_value_t = 20)]
        seconds: u64,
    },

    /// Install a Zepp OS app onto the band.
    ///
    /// Only apps. This never sends firmware.
    Install {
        /// Path to the app package built by zmake, for example
        /// band-app/dist/band-app.bin
        file: String,

        /// The btleplug peripheral id. Omit to use the configured one.
        #[arg(long)]
        peripheral_id: Option<String>,

        /// How long to scan before giving up, in seconds.
        #[arg(long, default_value_t = 20)]
        seconds: u64,
    },

    /// Print what this host knows about the band, from whichever process
    /// holds it: the address, the firmware, and the battery.
    ///
    /// The holder reads the address once per link and asks for the battery on
    /// connect, so this prints what it last heard.
    Device,

    /// Print band events from whichever process holds the band.
    ///
    /// Use this when a window or another session already has the band, so a
    /// second link is not needed to watch what the band sends.
    Follow,

    /// Read and arrange the band's screens: the app menu, the swipe shortcuts,
    /// and the control center. Without --set or --hide it only reads.
    Screens {
        /// Which list to work on: menu, shortcuts, or control.
        #[arg(long, default_value = "shortcuts")]
        list: String,

        /// Replace the shown entries with exactly these, in this order. Give
        /// names such as music, or ids as printed by a plain run. --set with
        /// nothing after it shows nothing.
        #[arg(long, value_delimiter = ',', num_args = 0..)]
        set: Option<Vec<String>>,

        /// Remove these from the shown entries, leaving the rest in order.
        #[arg(long, value_delimiter = ',')]
        hide: Vec<String>,

        /// Rebuild the app menu as every installed app, then settings. This is
        /// what an install does on its own.
        #[arg(long)]
        tidy: bool,

        /// The btleplug peripheral id. Omit to use the configured one.
        #[arg(long)]
        peripheral_id: Option<String>,

        /// How long to scan before giving up, in seconds.
        #[arg(long, default_value_t = 20)]
        seconds: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_of(args: &[&str]) -> Option<Vec<String>> {
        let line = ["bandbridge", "screens"]
            .into_iter()
            .chain(args.iter().copied());
        match Cli::try_parse_from(line).unwrap().command {
            Command::Screens { set, .. } => set,
            other => panic!("not screens: {other:?}"),
        }
    }

    /// --set with nothing after it shows nothing. With no --set at all, the
    /// command only reads.
    #[test]
    fn a_set_of_nothing_is_still_a_set() {
        assert_eq!(set_of(&[]), None);
        assert_eq!(set_of(&["--set"]), Some(vec![]));
        assert_eq!(
            set_of(&["--set", "weather,music"]),
            Some(vec!["weather".to_string(), "music".to_string()])
        );
    }
}
