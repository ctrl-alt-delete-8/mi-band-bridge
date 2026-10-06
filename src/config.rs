//! The config file: the band, how BandBridge behaves, and a list of named
//! actions.
//!
//! One set of types reads and writes the file, so the file the window writes
//! is the file it reads. Loading checks every rule, and a message names the
//! action that breaks one, by its name or by its place in the file.
//!
//! This module also names the folder BandBridge keeps its files in, and keeps
//! the band's auth key in the system's secret store, which is the one place
//! the key lives.

use crate::error::BandError;
use crate::keys;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The folder BandBridge keeps its files in: `BandBridge` inside the system's
/// config folder. That is `~/Library/Application Support/BandBridge/` on
/// macOS, and `%APPDATA%\BandBridge\` on Windows.
///
/// The config, the learned app names and the socket all live here, and this
/// is the one place that names it. Naming it creates nothing. Each write
/// creates the folder when it is missing, and a missing file reads as an empty
/// one.
///
/// None when the system names no config folder, which means no home folder.
pub fn folder() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("BandBridge"))
}

/// The 16-byte Zepp pairing key. Never printed.
///
/// It lives in the system's secret store, under the band's MAC, and never in
/// `config.toml`, a log line, an error message or a reply. [`AuthKey::load`]
/// finds it for a connection.
#[derive(Clone)]
pub struct AuthKey([u8; 16]);

/// The system's secret store, by the name a person knows it by.
pub const STORE: &str = if cfg!(target_os = "macos") {
    "the Keychain"
} else if cfg!(windows) {
    "Credential Manager"
} else {
    "the secret store"
};

/// The service every band's key is kept under in the store. The account is
/// the band's MAC.
///
/// This crate's tests get a service of their own. They use the real store,
/// and a band's key there may be the only copy of it, so no test can reach
/// that key, even through a slip that names a real band's MAC.
const SERVICE: &str = if cfg!(test) { "BandBridge-test" } else { "BandBridge" };

impl AuthKey {
    /// Read a key written as 32 hex digits, with or without `0x` in front,
    /// as the account login prints it. Spaces and line breaks are dropped.
    ///
    /// The reason for a refusal gives the length at most, never the text.
    pub fn from_hex(input: &str) -> Result<Self, BandError> {
        let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
        let cleaned = cleaned
            .strip_prefix("0x")
            .or_else(|| cleaned.strip_prefix("0X"))
            .unwrap_or(&cleaned)
            .to_string();

        if cleaned.chars().count() != 32 {
            return Err(BandError::AuthKey(format!(
                "a key is 32 hex digits, with or without 0x in front, and this one has {}",
                cleaned.chars().count()
            )));
        }

        let raw = hex::decode(&cleaned).map_err(|_| {
            BandError::AuthKey(
                "a key is 32 hex digits, 0 to 9 and a to f, and this one has another character"
                    .to_string(),
            )
        })?;

        let mut key = [0u8; 16];
        key.copy_from_slice(&raw);
        Ok(Self(key))
    }

    /// The key for a connection to the band with this MAC, the one in
    /// `[device]`.
    ///
    /// `BAND_AUTH_KEY` first, so a key given for one run wins. Then the
    /// system's secret store, under the band's MAC.
    ///
    /// A window opened from the Applications folder inherits no environment,
    /// so the environment alone is not enough. The store keeps the key out of
    /// the config, so people can share their actions, and out of shell
    /// history.
    pub fn load(mac: Option<&str>) -> Result<Self, BandError> {
        first_key(mac, std::env::var("BAND_AUTH_KEY").ok(), Self::read)
    }

    /// Keep the key in the system's secret store, under the band's MAC, in
    /// place of any key already there.
    pub fn store(&self, mac: &str) -> Result<(), BandError> {
        store_under(&account(mac)?, self)
    }

    /// The key the system's secret store holds for the band with this MAC,
    /// or None when it holds none.
    pub fn read(mac: &str) -> Result<Option<Self>, BandError> {
        read_under(&account(mac)?)
    }

    /// Take the band's key out of the system's secret store. A key that is
    /// not there is gone already, which is not an error.
    pub fn delete(mac: &str) -> Result<(), BandError> {
        delete_under(&account(mac)?)
    }

    /// Keep the key under the band's MAC, as [`AuthKey::store`] does, unless
    /// the store holds this very key there already.
    ///
    /// A band paired again on the same Mac finds its own key in the store.
    /// Leaving that item alone means a re-pair never rewrites the one copy of
    /// a key, and never asks the store for more than a read.
    pub fn keep_for(&self, mac: &str) -> Result<(), BandError> {
        keep_under(&account(mac)?, self)
    }

    /// The key as 32 hex digits, the way the account login prints it.
    ///
    /// For the store and the clipboard only. Never log it, and never send it
    /// anywhere else.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    pub fn bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl std::fmt::Debug for AuthKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AuthKey(redacted)")
    }
}

/// The first key found, in the order [`AuthKey::load`] gives. The store is
/// asked only when the environment has no key, so a key in the environment
/// never touches the store.
///
/// A store that cannot be read gives its own reason, rather than "no auth
/// key": there may well be a key in it.
fn first_key(
    mac: Option<&str>,
    env: Option<String>,
    read: impl FnOnce(&str) -> Result<Option<AuthKey>, BandError>,
) -> Result<AuthKey, BandError> {
    if let Some(raw) = env {
        tracing::debug!("the auth key is from BAND_AUTH_KEY");
        return AuthKey::from_hex(&raw);
    }
    let Some(mac) = mac else {
        return Err(BandError::MissingAuthKey(format!(
            "BAND_AUTH_KEY is not set, and no band is paired, so there is no MAC to \
             look the key up by in {STORE}"
        )));
    };
    match read(mac)? {
        Some(key) => {
            tracing::debug!("the auth key is from {STORE}");
            Ok(key)
        }
        None => Err(BandError::MissingAuthKey(redact(&format!(
            "BAND_AUTH_KEY is not set, and {STORE} holds no key for {mac}"
        )))),
    }
}

/// The account a band's key is kept under: its MAC in upper case with colons,
/// as `00:00:5E:00:53:01`, however `[device]` spells it.
fn account(mac: &str) -> Result<String, BandError> {
    let digits: Vec<char> = mac.trim().chars().filter(|c| !matches!(c, ':' | '-')).collect();
    if digits.len() != 12 || !digits.iter().all(char::is_ascii_hexdigit) {
        // Not quoted: a key pasted here by mistake would come back in it.
        return Err(BandError::Config(
            "[device] mac is not a MAC address, such as 00:00:5E:00:53:01".to_string(),
        ));
    }
    Ok(digits
        .chunks(2)
        .map(|pair| pair.iter().collect::<String>().to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join(":"))
}

/// One band's entry in the system's secret store.
fn entry(account: &str) -> Result<keyring::Entry, BandError> {
    keyring::Entry::new(SERVICE, account)
        .map_err(|err| store_failed("cannot hold a key for", account, &err))
}

fn store_under(account: &str, key: &AuthKey) -> Result<(), BandError> {
    entry(account)?
        .set_password(&key.to_hex())
        .map_err(|err| store_failed("cannot keep the key for", account, &err))
}

fn read_under(account: &str) -> Result<Option<AuthKey>, BandError> {
    match entry(account)?.get_password() {
        // A key put in the store by hand may not be one. The reason gives its
        // length at most, never the text.
        Ok(text) => AuthKey::from_hex(&text).map(Some).map_err(|err| {
            BandError::KeyStore(redact(&format!("{STORE} holds a bad key for {account}: {err}")))
        }),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(store_failed("cannot read the key for", account, &err)),
    }
}

fn delete_under(account: &str) -> Result<(), BandError> {
    match entry(account)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(store_failed("cannot delete the key for", account, &err)),
    }
}

/// Store the key under this account, unless it is there already. A store
/// that holds another key, or something that is not a key, gets this one.
fn keep_under(account: &str, key: &AuthKey) -> Result<(), BandError> {
    match read_under(account) {
        Ok(Some(held)) if held.bytes() == key.bytes() => Ok(()),
        _ => store_under(account, key),
    }
}

/// A store failure as one line that names the band and never the key.
///
/// Two of the store's errors carry what it holds, which may be the key. Their
/// Display leaves it out, but their Debug does not, so they are described here
/// instead, and no store error is ever printed with `{:?}`.
fn store_failed(what: &str, account: &str, err: &keyring::Error) -> BandError {
    let why = match err {
        keyring::Error::BadEncoding(_) | keyring::Error::BadDataFormat(..) => {
            "what it holds is not text".to_string()
        }
        other => other.to_string(),
    };
    BandError::KeyStore(redact(&format!("{STORE} {what} {account}: {why}")))
}

/// Delete what BandBridge keeps outside its install folder: the key of the
/// band `config.toml` names, from the system's secret store, and then
/// BandBridge's [`folder`], with the config and the rest in it. Windows'
/// uninstaller asks for this when a person ticks "Delete the application
/// data". What is not there is gone already.
pub fn delete_app_data() -> Result<(), BandError> {
    match folder() {
        Some(folder) => delete_app_data_in(&folder, AuthKey::delete),
        None => Ok(()),
    }
}

/// [`delete_app_data`] for one folder. The key leaves the store first,
/// through `delete_key`, which is given the band's MAC as the file spells
/// it. A key the store keeps keeps the folder too, so the config still names
/// the band whose key is left.
fn delete_app_data_in(
    folder: &Path,
    delete_key: impl FnOnce(&str) -> Result<(), BandError>,
) -> Result<(), BandError> {
    if let Some(mac) = paired_mac(&folder.join("config.toml"))? {
        // A key is only ever kept under a MAC.
        if account(&mac).is_ok() {
            delete_key(&mac)?;
        }
    }
    match std::fs::remove_dir_all(folder) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(BandError::Config(format!("{}: {err}", folder.display()))),
    }
}

/// The MAC `[device]` names, read from the file as plain tables, so a config
/// that a broken action keeps from loading still names its band. None for no
/// file, or no band. An error for a file that is not TOML, whose band is then
/// unknown.
fn paired_mac(path: &Path) -> Result<Option<String>, BandError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(BandError::Config(format!("{}: {err}", path.display()))),
    };
    let table: toml::Table = toml::from_str(&text).map_err(|err| located(&text, &err))?;
    Ok(table
        .get("device")
        .and_then(|device| device.get("mac"))
        .and_then(toml::Value::as_str)
        .map(str::to_string))
}

/// An action as the window's list showed it: its place in the list, its
/// name and its signal. The window names the action it edits or deletes this
/// way, because two actions may share a signal (D6). The name and the signal
/// catch a file that changed after the list was read.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Listed {
    pub at: usize,
    pub name: String,
    pub signal: String,
}

/// Everything in `config.toml`. The auth key is not here.
///
/// An unknown key is refused, never ignored. An ignored key is a setting the
/// person believes they made, and a file in the old format would otherwise
/// load as an empty one, with no word about why nothing fires.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// The band, once one is paired. With none, the window opens on setup.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<Device>,
    pub settings: Settings,
    pub actions: Vec<Action>,
}

/// The band, as the person named it and as this Mac reaches it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Device {
    /// What the person calls the band.
    pub name: String,
    /// The band's Bluetooth address, read from its System ID.
    pub mac: String,
    /// The id macOS made up for the band on this Mac. BandBridge finds it and
    /// records it for a fast reconnect, and nobody types it. With none,
    /// BandBridge finds the band itself. On Windows the id is the band's
    /// Bluetooth address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
}

impl Device {
    /// The band a pair found: the name the person gave it, the MAC it
    /// reported, and this computer's id for it.
    ///
    /// The id is kept in lower case, the way btleplug prints it. It is
    /// compared as a UUID when it is read back, so the case no longer decides
    /// whether the band is found, but the file stays in one spelling.
    pub fn found(name: &str, mac: &str, id: &str) -> Self {
        Self {
            name: name.trim().to_string(),
            mac: mac.to_string(),
            uuid: Some(id.trim().to_ascii_lowercase()),
        }
    }
}

/// What happens to the actions and settings made for a band when it is
/// forgotten, and what the next pair does with the ones a forget kept.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Setup {
    /// Keep them, for the next band to use.
    #[default]
    Keep,
    /// Delete every action, and put every setting back to its default.
    Delete,
}

/// What the file holds for the next pair: the band, while one is paired, and
/// the actions and settings a forget kept. The window asks for it, to offer
/// the kept ones to the next band or to start fresh.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedSetup {
    /// The paired band, or None when no band is paired.
    pub device: Option<PairedBand>,
    /// How many actions the file holds.
    pub actions: usize,
    /// Whether any setting differs from its default.
    pub settings_changed: bool,
}

/// A paired band, as the window names it. This computer's id for the band is
/// left out: nobody reads it or types it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PairedBand {
    pub name: String,
    pub mac: String,
}

/// How BandBridge behaves.
///
/// The window changes these, and each change takes effect with no restart:
/// the band task follows the file, and the window sets its own theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// Reconnect after the link drops. A disconnect the person asked for is
    /// never undone.
    pub auto_reconnect: bool,
    /// Keep the app menu as the newly installed app, then the other apps, then
    /// settings, every time the band connects.
    ///
    /// Two reasons this is rebuilt rather than written once. The band re-adds
    /// screens of its own when it starts, so music comes back after a restart
    /// even though it was taken out. And the band drops a freshly installed app
    /// wherever it likes, while the reason you installed it is to press it.
    pub new_app_on_top: bool,
    /// The window's theme.
    pub appearance: Appearance,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_reconnect: true,
            new_app_on_top: true,
            appearance: Appearance::System,
        }
    }
}

/// Light, dark, or whatever macOS is set to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    Light,
    Dark,
    #[default]
    System,
}

/// A change to some of the settings, as the window asks for one: a switch
/// or a choice names its own setting, and each one left out stays as the
/// file has it.
///
/// A name that is not a setting is refused rather than ignored. An ignored
/// change is a switch that moves and does nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsChange {
    pub auto_reconnect: Option<bool>,
    pub new_app_on_top: Option<bool>,
    pub appearance: Option<Appearance>,
}

impl Settings {
    /// These settings, with a change made to them.
    pub fn changed(self, change: &SettingsChange) -> Self {
        Self {
            auto_reconnect: change.auto_reconnect.unwrap_or(self.auto_reconnect),
            new_app_on_top: change.new_app_on_top.unwrap_or(self.new_app_on_top),
            appearance: change.appearance.unwrap_or(self.appearance),
        }
    }
}

/// One named thing a press does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Action {
    /// What the person calls it. The window and the event log show it.
    ///
    /// Missing, it reads as empty, so the loader can name the action by its
    /// place instead of serde reporting a missing field with nothing to point
    /// at. The same goes for `signal`.
    #[serde(default)]
    pub name: String,
    /// The bytes a band app sends, in hex, such as `bb01`.
    #[serde(default)]
    pub signal: String,
    /// What happens: one key in the file, named for its kind.
    #[serde(flatten)]
    pub does: Does,
}

/// What an action does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Does {
    /// A key, or a media key, posted to the frontmost app.
    Keystroke(Keystroke),
    /// A command line, run by a shell, so pipes and quotes work: `sh -c` on
    /// the Mac, and PowerShell with no profile on Windows.
    Shell(String),
    /// A script file. The Mac runs it with `sh`. Windows runs a .ps1 with
    /// PowerShell, and opens any other file as a double-click opens it. A
    /// leading `~` is the home folder.
    Script(String),
    /// A shortcut in Shortcuts, by the name it shows there.
    Shortcut(String),
    /// An HTTP call.
    WebRequest(WebRequest),
}

impl Does {
    /// The key each kind has in the file, in the order of the variants.
    pub const KINDS: [&'static str; 5] = ["keystroke", "shell", "script", "shortcut", "web_request"];

    /// The key this kind has in the file.
    pub fn kind(&self) -> &'static str {
        match self {
            Does::Keystroke(_) => "keystroke",
            Does::Shell(_) => "shell",
            Does::Script(_) => "script",
            Does::Shortcut(_) => "shortcut",
            Does::WebRequest(_) => "web_request",
        }
    }

    fn check(&self) -> Result<(), Broken> {
        match self {
            Does::Keystroke(keystroke) => keystroke.check(),
            Does::Shell(text) | Does::Script(text) | Does::Shortcut(text) => {
                if text.trim().is_empty() {
                    return Err(Broken::new(
                        self.kind(),
                        format!("{} is empty", self.kind()),
                    ));
                }
                // Shortcuts is a Mac app, and a system without it refuses the
                // kind (W5).
                if let (Does::Shortcut(_), Some(why)) =
                    (self, crate::platform::actions::SHORTCUTS_REFUSED)
                {
                    return Err(Broken::new("shortcut", why));
                }
                Ok(())
            }
            Does::WebRequest(request) => request.check(),
        }
    }
}

/// A rule an action breaks: the field it is about, by the name the file gives
/// it, and why. The loader puts the action's name in front of the why. The
/// window's editor shows the why beside the field: see [`Problem`].
#[derive(Debug)]
struct Broken {
    field: &'static str,
    why: String,
}

impl Broken {
    fn new(field: &'static str, why: impl Into<String>) -> Self {
        Self {
            field,
            why: why.into(),
        }
    }
}

/// A key, by the name Karabiner-EventViewer shows for it (D3).
///
/// Exactly one of `key_code` and `consumer_key_code`. A media key is a
/// consumer key, and takes no modifiers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keystroke {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumer_key_code: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<String>,
}

impl Keystroke {
    fn check(&self) -> Result<(), Broken> {
        match (&self.key_code, &self.consumer_key_code) {
            (Some(_), Some(_)) => Err(Broken::new(
                "keystroke",
                "a keystroke takes key_code or consumer_key_code, not both",
            )),
            (None, None) => Err(Broken::new(
                "keystroke",
                "a keystroke needs key_code or consumer_key_code",
            )),
            (Some(name), None) => {
                if !keys::sends_key(name) {
                    // Karabiner names some keys macOS cannot send, and Windows
                    // cannot send a few that macOS can (W2). Say which, so a
                    // name copied from Karabiner-EventViewer does not read as
                    // a typo.
                    return Err(Broken::new(
                        "keystroke.key_code",
                        match keys::unsupported_key(name) {
                            Some(why) => format!("key_code {name} cannot be sent. {why}"),
                            None => format!("unknown key_code {name}"),
                        },
                    ));
                }
                for modifier in &self.modifiers {
                    if !keys::sends_modifier(modifier) {
                        return Err(Broken::new(
                            "keystroke.modifiers",
                            match keys::unsupported_modifier(modifier) {
                                Some(why) => format!("the modifier {modifier} cannot be sent. {why}"),
                                None => format!(
                                    "unknown modifier {modifier}. Use one of: {}",
                                    keys::modifier_names().join(", ")
                                ),
                            },
                        ));
                    }
                }
                Ok(())
            }
            (None, Some(name)) => {
                if !keys::sends_media_key(name) {
                    return Err(Broken::new(
                        "keystroke.consumer_key_code",
                        match keys::unsupported_media_key(name) {
                            Some(why) => format!("consumer_key_code {name} cannot be sent. {why}"),
                            None => format!(
                                "unknown consumer_key_code {name}. Use one of: {}",
                                keys::media_key_names().join(", ")
                            ),
                        },
                    ));
                }
                if !self.modifiers.is_empty() {
                    return Err(Broken::new(
                        "keystroke.modifiers",
                        "a media key takes no modifiers",
                    ));
                }
                Ok(())
            }
        }
    }
}

/// An HTTP call.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebRequest {
    /// GET by default, or POST when there is a body. See [`WebRequest::method`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default)]
    pub url: String,
    /// Header lines, each `Name: value`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

/// The methods a web request may name.
pub const METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"];

impl WebRequest {
    /// The method to send, in upper case.
    ///
    /// A body with no method means POST: a body is how you send something,
    /// and GET with a body surprises most servers.
    pub fn method(&self) -> String {
        match (&self.method, &self.body) {
            (Some(method), _) => method.trim().to_ascii_uppercase(),
            (None, Some(_)) => "POST".to_string(),
            (None, None) => "GET".to_string(),
        }
    }

    fn check(&self) -> Result<(), Broken> {
        if !self.url.starts_with("http://") && !self.url.starts_with("https://") {
            return Err(Broken::new(
                "web_request.url",
                "the url must start with http:// or https://",
            ));
        }
        let method = self.method();
        if !METHODS.contains(&method.as_str()) {
            return Err(Broken::new(
                "web_request.method",
                format!(
                    "unknown method {method}. Use one of: {}",
                    METHODS.join(", ")
                ),
            ));
        }
        for (place, header) in self.headers.iter().enumerate() {
            match header.split_once(':') {
                Some((name, _)) if !name.trim().is_empty() => {}
                // Named by its place, never quoted: a header is where a token
                // lives.
                _ => {
                    return Err(Broken::new(
                        "web_request.headers",
                        format!("header {} must read Name: value", place + 1),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Why an action with no name is refused, after the words that name it.
const NEEDS_A_NAME: &str = "needs a name, which the window and the event log show";

/// Why an action with no signal is refused, after the words that name it.
const NEEDS_A_SIGNAL: &str = "needs a signal, such as bb04";

impl Action {
    /// Check the rules that need no other action, and give back the signal as
    /// the band sends it.
    fn check(&self, position: usize) -> Result<String, String> {
        let who = who(position, &self.name);
        if self.name.trim().is_empty() {
            return Err(format!("{who} {NEEDS_A_NAME}"));
        }
        if self.signal.trim().is_empty() {
            return Err(format!("{who} {NEEDS_A_SIGNAL}"));
        }
        let signal = crate::signal::normalize(&self.signal).map_err(|why| format!("{who}: {why}"))?;
        self.does
            .check()
            .map_err(|broken| format!("{who}: {}", broken.why))?;
        Ok(signal)
    }

    /// An action as the window's editor sends it, read and checked by the
    /// file's rules. It comes in the file's own shape, as JSON: a name, a
    /// signal, and one kind, such as
    /// `{"name": "Open Music", "signal": "bb03", "shell": "open -a Music"}`.
    ///
    /// Every problem comes back at once, at most one for each field, so the
    /// editor can show each beside its field. The name is kept without the
    /// spaces around it, and the signal as the band sends it, in lower case.
    ///
    /// No two actions may share a signal (D6). That rule needs the other
    /// actions, so [`Config::save_action_at`] checks it.
    pub fn from_editor(written: &serde_json::Value) -> Result<Action, Refused> {
        let Some(fields) = written.as_object() else {
            return Err(Refused::general(&not_an_action()));
        };
        let mut refused = Refused::default();

        let name = text_field(fields, "name", &mut refused);
        if name.trim().is_empty() {
            refused.add(Problem::at("name", &format!("the action {NEEDS_A_NAME}")));
        } else if holds_a_key(&name) {
            // Copy key puts the band's key on the clipboard in this same
            // window. Pasted here, it would go into the file and every line
            // of the event log.
            refused.add(Problem::at(
                "name",
                "the name looks like a key. Give the action a name to know it by, such as \
                 Open Music.",
            ));
        }

        let written_signal = text_field(fields, "signal", &mut refused);
        let signal = if written_signal.trim().is_empty() {
            refused.add(Problem::at(
                "signal",
                &format!("the action {NEEDS_A_SIGNAL}"),
            ));
            None
        } else {
            crate::signal::normalize(&written_signal)
                .map_err(|why| refused.add(Problem::at("signal", &why)))
                .ok()
        };

        let does = read_kind(fields, &mut refused);
        match (signal, does) {
            (Some(signal), Some(does)) if refused.errors.is_empty() => Ok(Action {
                name: name.trim().to_string(),
                signal,
                does,
            }),
            _ => {
                refused.in_field_order();
                Err(refused)
            }
        }
    }
}

impl Does {
    /// What an action from the window's editor does, read and checked by the
    /// file's rules, for Try it now. The written action has the shape
    /// [`Action::from_editor`] reads, and its name and signal are not looked
    /// at: an action can be tried before it has either.
    pub fn from_editor(written: &serde_json::Value) -> Result<Does, Refused> {
        let Some(fields) = written.as_object() else {
            return Err(Refused::general(&not_an_action()));
        };
        let mut refused = Refused::default();
        match read_kind(fields, &mut refused) {
            Some(does) if refused.errors.is_empty() => Ok(does),
            _ => {
                refused.in_field_order();
                Err(refused)
            }
        }
    }
}

/// A rule an action from the window's editor breaks, named by its field, so
/// the editor can show it beside that field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    /// The field, by the name the file gives it: `name`, `signal`, `kind`
    /// (for none, or more than one), a kind (`keystroke`, `shell`, `script`,
    /// `shortcut`, `web_request`), or one field of a kind
    /// (`keystroke.key_code`, `keystroke.consumer_key_code`,
    /// `keystroke.modifiers`, `web_request.url`, `web_request.method`,
    /// `web_request.headers`).
    ///
    /// None for a problem that is no one field's: the file could not be read
    /// or written, the action being edited is gone from it, or the action
    /// has a key that is not part of one.
    pub field: Option<&'static str>,
    /// Why, in words fit to show. A long run of hex digits is cut out, as in
    /// every message about the file.
    pub message: String,
}

impl Problem {
    fn at(field: &'static str, message: &str) -> Self {
        Self {
            field: Some(field),
            message: redact(message),
        }
    }

    fn general(message: &str) -> Self {
        Self {
            field: None,
            message: redact(message),
        }
    }
}

impl From<Broken> for Problem {
    fn from(broken: Broken) -> Self {
        Problem::at(broken.field, &broken.why)
    }
}

/// Why an action from the window's editor was not saved or tried: every
/// problem, at most one for each field. The page gets it as
/// `{"errors": [{"field": "signal", "message": "…"}]}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Refused {
    pub errors: Vec<Problem>,
}

impl Refused {
    fn general(message: &str) -> Self {
        Self {
            errors: vec![Problem::general(message)],
        }
    }

    /// Keep a problem, unless its field has one already: the first rule a
    /// field breaks is the one to show beside it.
    fn add(&mut self, problem: Problem) {
        let shown =
            problem.field.is_some() && self.errors.iter().any(|seen| seen.field == problem.field);
        if !shown {
            self.errors.push(problem);
        }
    }

    /// Put the problems in the order the editor shows its fields: the name,
    /// the signal, what the action does, and last the problems that are no
    /// field's.
    fn in_field_order(&mut self) {
        self.errors.sort_by_key(|problem| match problem.field {
            Some("name") => 0,
            Some("signal") => 1,
            Some(_) => 2,
            None => 3,
        });
    }
}

/// A file that cannot be read or written, as one problem that is no field's.
impl From<BandError> for Refused {
    fn from(err: BandError) -> Self {
        Refused::general(&err.to_string())
    }
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let said: Vec<String> = self
            .errors
            .iter()
            .map(|problem| match problem.field {
                Some(field) => format!("{field}: {}", problem.message),
                None => problem.message.clone(),
            })
            .collect();
        f.write_str(&said.join("; "))
    }
}

/// What an action from the editor must be, when it is not even a table.
fn not_an_action() -> String {
    format!(
        "an action is a name, a signal, and one of {}",
        Does::KINDS.join(", ")
    )
}

/// A text field of an action from the editor: empty when it is left out, and
/// a problem when it is not text.
fn text_field(
    fields: &serde_json::Map<String, serde_json::Value>,
    field: &'static str,
    refused: &mut Refused,
) -> String {
    match fields.get(field) {
        None | Some(serde_json::Value::Null) => String::new(),
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(_) => {
            refused.add(Problem::at(field, &format!("the {field} must be text")));
            String::new()
        }
    }
}

/// What an action from the editor does: its one kind, read and checked. A
/// key that is not part of an action, no kind or more than one, and each rule
/// the kind breaks, is a problem.
fn read_kind(
    fields: &serde_json::Map<String, serde_json::Value>,
    refused: &mut Refused,
) -> Option<Does> {
    for key in fields.keys() {
        if !is_part_of_an_action(key) {
            refused.add(Problem::general(&not_part_of_an_action(key)));
        }
    }
    // In the order of the kinds, whatever order the map keeps its keys in.
    let set: Vec<&'static str> = Does::KINDS
        .into_iter()
        .filter(|kind| fields.contains_key(*kind))
        .collect();
    let [kind] = set[..] else {
        if let Some(why) = kinds_problem("the action", &set) {
            refused.add(Problem::at("kind", &why));
        }
        return None;
    };
    let one = serde_json::Map::from_iter([(kind.to_string(), fields[kind].clone())]);
    match serde_json::from_value::<Does>(serde_json::Value::Object(one)) {
        Ok(does) => {
            if let Err(broken) = does.check() {
                refused.add(broken.into());
            }
            Some(does)
        }
        Err(err) => {
            refused.add(Problem::at(kind, &err.to_string()));
            None
        }
    }
}

/// The first line of every file BandBridge writes.
const HEADER: &str = "# Mi Band Bridge writes this file. Change it in the window.\n";

/// Held for the whole of every read, change and save of the file in this
/// process. See [`Config::update_at`].
static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The file where BandBridge keeps the config, or why there is none.
fn home_file() -> Result<PathBuf, BandError> {
    Config::path().ok_or_else(|| {
        BandError::Config("there is no home folder to keep the config in".to_string())
    })
}

impl Config {
    /// Where the file lives, in BandBridge's [`folder`].
    pub fn path() -> Option<PathBuf> {
        Some(folder()?.join("config.toml"))
    }

    /// Read the file where BandBridge keeps it. See [`Config::load_from`].
    pub fn load() -> Result<Self, BandError> {
        match Self::path() {
            Some(path) => Self::load_from(&path),
            None => Ok(Self::default()),
        }
    }

    /// [`Config::load_from`], under this process's config lock, which every
    /// change holds. A write replaces the file whole, so a read without the
    /// lock sees a whole file too. The lock makes the read wait for a change
    /// in progress, so it sees that change. See [`Config::update_at`].
    pub fn load_under_lock(path: &Path) -> Result<Self, BandError> {
        let _held = WRITING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self::load_from(path)
    }

    /// A missing file means the defaults. A malformed file, or an action that
    /// breaks a rule, is an error that says where.
    pub fn load_from(path: &Path) -> Result<Self, BandError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(BandError::Config(format!("{}: {err}", path.display()))),
        }
    }

    /// Read a config from its text, and check every rule.
    pub fn parse(text: &str) -> Result<Self, BandError> {
        // First as plain tables. The kind is flattened into its action, and
        // serde takes the first kind it finds and drops every other key, so a
        // second kind, or a field left over from the old flat format, would
        // vanish without a word. Here each one is named instead.
        let table: toml::Table = toml::from_str(text).map_err(|err| located(text, &err))?;
        if let Some(toml::Value::Array(actions)) = table.get("actions") {
            for (position, written) in actions.iter().enumerate() {
                check_written(position, written)?;
            }
        }
        let config: Config = toml::from_str(text).map_err(|err| located(text, &err))?;
        config.check()?;
        Ok(config)
    }

    /// Check the rules a well formed file can still break.
    ///
    /// Actions may share a signal (D6): a press runs every action on its
    /// signal, one by one, in the order of the file.
    pub fn check(&self) -> Result<(), BandError> {
        for (position, action) in self.actions.iter().enumerate() {
            action.check(position).map_err(|why| config_error(&why))?;
        }
        Ok(())
    }

    /// The whole file, as BandBridge writes it: a header that says so, then
    /// `[device]`, `[settings]`, and one `[[actions]]` table per action, with a
    /// keystroke or a web request on one line.
    pub fn to_toml(&self) -> Result<String, BandError> {
        let mut out = String::from(HEADER);
        if let Some(device) = &self.device {
            out.push_str("\n[device]\n");
            out.push_str(&toml::to_string(device).map_err(unwritable)?);
        }
        out.push_str("\n[settings]\n");
        out.push_str(&toml::to_string(&self.settings).map_err(unwritable)?);
        for action in &self.actions {
            let does = match &action.does {
                Does::Keystroke(keystroke) => inline(keystroke)?,
                Does::Shell(text) | Does::Script(text) | Does::Shortcut(text) => inline(text)?,
                Does::WebRequest(request) => inline(request)?,
            };
            out.push_str(&format!(
                "\n[[actions]]\nname = {}\nsignal = {}\n{} = {does}\n",
                inline(&action.name)?,
                inline(&action.signal)?,
                action.does.kind(),
            ));
        }
        Ok(out)
    }

    /// Write the whole file where BandBridge keeps it. See [`Config::save_to`].
    ///
    /// This writes back a config as it was read, whenever that was. A change
    /// to the file goes through [`Config::update`], which reads, changes and
    /// writes it under this process's config lock.
    pub fn save(&self) -> Result<(), BandError> {
        self.save_to(&home_file()?)
    }

    /// Write the whole file.
    ///
    /// Refused while a rule is broken, so the file on disk always loads.
    pub fn save_to(&self, path: &Path) -> Result<(), BandError> {
        self.check()?;
        let text = self.to_toml()?;
        // Through a link to the file it points at, so a config kept elsewhere
        // stays there.
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let cannot = |err: std::io::Error| {
            BandError::Config(format!("cannot write {}: {err}", path.display()))
        };
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).map_err(cannot)?;
        }
        // Written whole beside the file, then moved over it in one step. The
        // band task reads the file whenever it changes, and must never see
        // half of one.
        let mut partial = path.clone().into_os_string();
        partial.push(".partial");
        let partial = PathBuf::from(partial);
        std::fs::write(&partial, text).map_err(cannot)?;
        std::fs::rename(&partial, &path).map_err(cannot)?;
        Ok(())
    }

    /// Read the file where BandBridge keeps it, change it, and write it
    /// whole. See [`Config::update_at`].
    pub fn update<T, E: From<BandError>>(
        change: impl FnOnce(&mut Config) -> Result<T, E>,
    ) -> Result<T, E> {
        Self::update_at(&home_file()?, change)
    }

    /// Read the file at `path`, change it, and write it whole, with no other
    /// change in between: this process's config lock is held throughout.
    ///
    /// Four things write the file, and each writes all of it: the window's
    /// settings and its actions, and the band task's pair and forget. Two of
    /// them at once, with no lock, would each write back what it had read,
    /// and the change of the first to save would be lost.
    ///
    /// `change` gives the answer, or why the change cannot be made, and then
    /// nothing is written. A change that leaves the config as it was writes
    /// nothing, and the band task, which reads the file whenever it changes,
    /// has nothing to read again. A file that does not load is not written,
    /// so a change to one part never replaces a part it could not read.
    pub fn update_at<T, E: From<BandError>>(
        path: &Path,
        change: impl FnOnce(&mut Config) -> Result<T, E>,
    ) -> Result<T, E> {
        let _held = WRITING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut config = Self::load_from(path)?;
        let before = config.clone();
        let answer = change(&mut config)?;
        if config != before {
            config.save_to(path)?;
        }
        Ok(answer)
    }

    /// Change some settings in the file where BandBridge keeps it. See
    /// [`Config::change_settings_at`].
    pub fn change_settings(change: &SettingsChange) -> Result<Settings, BandError> {
        Self::change_settings_at(&home_file()?, change)
    }

    /// Change the settings a change names, in the file at `path`, and give
    /// back the settings the file then holds. `[device]` and the actions are
    /// written back as they were read. See [`Config::update_at`].
    pub fn change_settings_at(path: &Path, change: &SettingsChange) -> Result<Settings, BandError> {
        Self::update_at(path, |config| {
            config.settings = config.settings.changed(change);
            Ok(config.settings)
        })
    }

    /// Put an action from the window's editor into the file where BandBridge
    /// keeps it. See [`Config::save_action_at`].
    pub fn save_action(
        was: Option<&Listed>,
        written: &serde_json::Value,
    ) -> Result<Vec<Action>, Refused> {
        Self::save_action_at(&home_file()?, was, written)
    }

    /// Put an action from the window's editor into the file at `path`, and
    /// give back the actions the file then holds.
    ///
    /// `written` is the action in the file's shape, which
    /// [`Action::from_editor`] reads and checks. An edited action takes the
    /// place of the one the list showed as `was`. A new one, with no `was`,
    /// goes after the others. Any signal will do, one another action has too
    /// (D6).
    ///
    /// Nothing is written while the action breaks a rule. Every problem comes
    /// back at once, in the order of the editor's fields. An action that is
    /// no longer where the list showed it is not saved as a new one, because
    /// the person edited that one.
    pub fn save_action_at(
        path: &Path,
        was: Option<&Listed>,
        written: &serde_json::Value,
    ) -> Result<Vec<Action>, Refused> {
        Self::update_at(path, |config| {
            let place = match was {
                None => None,
                Some(was) => Some(config.place_of(was).ok_or_else(|| {
                    Refused::general(&format!(
                        "{} is no longer where the list showed it, so it was not saved. The \
                         file changed after the list was read.",
                        who(was.at, &was.name)
                    ))
                })?),
            };
            let action = Action::from_editor(written).map_err(|mut refused| {
                refused.in_field_order();
                refused
            })?;
            match place {
                Some(place) => config.actions[place] = action,
                None => config.actions.push(action),
            }
            Ok(config.actions.clone())
        })
    }

    /// Take an action out of the file where BandBridge keeps it. See
    /// [`Config::delete_action_at`].
    pub fn delete_action(was: &Listed) -> Result<Vec<Action>, BandError> {
        Self::delete_action_at(&home_file()?, was)
    }

    /// Take the action the list showed as `was` out of the file at `path`,
    /// and give back the actions the file then holds, in their order.
    pub fn delete_action_at(path: &Path, was: &Listed) -> Result<Vec<Action>, BandError> {
        Self::update_at(path, |config| {
            let place = config.place_of(was).ok_or_else(|| {
                config_error(&format!(
                    "{} is no longer where the list showed it, so nothing was deleted. The \
                     file changed after the list was read.",
                    who(was.at, &was.name)
                ))
            })?;
            config.actions.remove(place);
            Ok(config.actions.clone())
        })
    }

    /// Where the action the list showed as `was` is in the file now: at the
    /// same place, with the same name and the same signal, in either case.
    /// A signal no longer names an action, since two may share one (D6).
    fn place_of(&self, was: &Listed) -> Option<usize> {
        let action = self.actions.get(was.at)?;
        let same_signal = match (
            crate::signal::normalize(&action.signal),
            crate::signal::normalize(&was.signal),
        ) {
            (Ok(now), Ok(then)) => now == then,
            _ => false,
        };
        (action.name == was.name && same_signal).then_some(was.at)
    }

    /// This Mac's id for the band, when BandBridge has recorded one.
    pub fn uuid(&self) -> Option<&str> {
        self.device.as_ref()?.uuid.as_deref()
    }

    /// The band's MAC, once a band is paired. Its key is kept under it.
    pub fn mac(&self) -> Option<&str> {
        Some(self.device.as_ref()?.mac.as_str())
    }

    /// The band, and what a forget kept, for the window to show.
    pub fn saved_setup(&self) -> SavedSetup {
        SavedSetup {
            device: self.device.as_ref().map(|device| PairedBand {
                name: device.name.clone(),
                mac: device.mac.clone(),
            }),
            actions: self.actions.len(),
            settings_changed: self.settings != Settings::default(),
        }
    }

    /// Keep the actions and settings, or delete them: no actions, and every
    /// setting at its default.
    pub fn keep_or_delete(&mut self, setup: Setup) {
        if setup == Setup::Delete {
            self.actions.clear();
            self.settings = Settings::default();
        }
    }

    /// Forget the paired band, and keep or delete its actions and settings.
    ///
    /// The band's key leaves the store first, through `delete_key`, which is
    /// given the band's MAC. Only then does `[device]` leave the config. A
    /// store that will not give the key up leaves the config as it was, so a
    /// second try still knows which key to take out. The caller saves the
    /// file.
    ///
    /// Returns the band that was forgotten, or None when none was paired.
    /// With none, the store is not asked, and the choice still applies.
    pub fn forget(
        &mut self,
        setup: Setup,
        delete_key: impl FnOnce(&str) -> Result<(), BandError>,
    ) -> Result<Option<Device>, BandError> {
        if let Some(device) = &self.device {
            delete_key(&device.mac)?;
        }
        let forgotten = self.device.take();
        self.keep_or_delete(setup);
        Ok(forgotten)
    }
}

/// Whether a text holds sixteen hex digits in a row, which is half a key at
/// least, in some spelling. A name or a message that does is refused or cut.
pub(crate) fn holds_a_key(text: &str) -> bool {
    redact(text) != text
}

/// How a message names an action: by its name, or by its place in the file
/// when it has none.
fn who(position: usize, name: &str) -> String {
    if name.trim().is_empty() {
        format!("action {}", position + 1)
    } else {
        format!("action {name:?}")
    }
}

/// Check one action as written, for what serde cannot see through the
/// flattened kind: a key that is not part of an action, and anything but
/// exactly one kind. Then read it on its own, so that a problem inside it is
/// named too.
fn check_written(position: usize, written: &toml::Value) -> Result<(), BandError> {
    // Not a table: reading the whole file says so, with its line.
    let Some(fields) = written.as_table() else {
        return Ok(());
    };
    let name = fields
        .get("name")
        .and_then(toml::Value::as_str)
        .unwrap_or_default();
    let who = who(position, name);

    for key in fields.keys() {
        if !is_part_of_an_action(key) {
            return Err(config_error(&format!(
                "{who}: {}",
                not_part_of_an_action(key)
            )));
        }
    }
    let set: Vec<&str> = fields
        .keys()
        .map(String::as_str)
        .filter(|key| Does::KINDS.contains(key))
        .collect();
    if let Some(why) = kinds_problem(&who, &set) {
        return Err(config_error(&why));
    }
    Action::deserialize(written.clone())
        .map_err(|err| config_error(&format!("{who}: {}", err.message())))?;
    Ok(())
}

/// Whether a key belongs in an action: its name, its signal, or a kind.
fn is_part_of_an_action(key: &str) -> bool {
    key == "name" || key == "signal" || Does::KINDS.contains(&key)
}

/// Why a key that is not part of an action is refused. The loader names the
/// action in front of it.
fn not_part_of_an_action(key: &str) -> String {
    format!(
        "{key} is not part of an action. An action has a name, a signal, and one of {}.",
        Does::KINDS.join(", ")
    )
}

/// Why an action that sets these kinds is refused, when it sets none or more
/// than one: `who` names it.
fn kinds_problem(who: &str, set: &[&str]) -> Option<String> {
    match set {
        [_] => None,
        [] => Some(format!(
            "{who} does nothing. Give it one of {}.",
            Does::KINDS.join(", ")
        )),
        several => Some(format!(
            "{who} sets {}. Give it one kind.",
            several.join(" and ")
        )),
    }
}

/// A TOML error as one line that says where, and never quotes the file.
///
/// The toml crate quotes the offending line, and a line of this file can hold
/// a token in a web request header. One line also reads better in the window.
fn located(text: &str, err: &toml::de::Error) -> BandError {
    let message = err
        .message()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(": ");
    match err.span() {
        Some(span) => {
            let line = text.get(..span.start).unwrap_or(text).matches('\n').count() + 1;
            config_error(&format!("line {line}: {message}"))
        }
        None => config_error(&message),
    }
}

/// An error in what the file says.
///
/// Every message about the file's content is built here, and it quotes what
/// the file says: a name, a signal, a key name. The auth key does not belong
/// in the file, but a person may paste it in by mistake, and it must never
/// come back in a message. So every spelling of it is cut out on the way.
fn config_error(message: &str) -> BandError {
    BandError::Config(redact(message))
}

/// A message with every long run of hex digits cut out.
///
/// serde quotes a value of the wrong type: "invalid type: integer `42`". A key
/// written without its quotes reads as a number, and would be quoted whole, in
/// decimal. Sixteen hex digits in a row are half a key at least, and little
/// else in the file runs that long: a uuid breaks every twelve digits or
/// fewer, and a MAC every two. A signal of eight bytes loses its digits too,
/// which costs a message some detail and nothing more.
///
/// The socket uses it too, for a request it cannot read: serde quotes the
/// value it could not take, and that value can be a key.
pub(crate) fn redact(message: &str) -> String {
    fn flush(out: &mut String, run: &mut String) {
        if run.len() >= 16 {
            out.push('…');
        } else {
            out.push_str(run);
        }
        run.clear();
    }
    let mut out = String::with_capacity(message.len());
    let mut run = String::new();
    for c in message.chars() {
        if c.is_ascii_hexdigit() {
            run.push(c);
        } else {
            flush(&mut out, &mut run);
            out.push(c);
        }
    }
    flush(&mut out, &mut run);
    out
}

/// One value on one line, a table in braces, as the file writes a keystroke
/// or a web request.
fn inline<T: Serialize + ?Sized>(value: &T) -> Result<String, BandError> {
    let mut out = String::new();
    value
        .serialize(toml::ser::ValueSerializer::new(&mut out))
        .map_err(unwritable)?;
    Ok(out)
}

fn unwritable(err: toml::ser::Error) -> BandError {
    BandError::Config(format!("cannot write the config: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- the auth key, which lives outside this file -----------------------

    #[test]
    fn parses_plain_hex() {
        let key = AuthKey::from_hex("000102030405060708090a0b0c0d0e0f").unwrap();
        assert_eq!(key.bytes()[0], 0x00);
        assert_eq!(key.bytes()[15], 0x0f);
    }

    #[test]
    fn accepts_prefix_and_whitespace() {
        let key = AuthKey::from_hex(" 0x00 0102030405060708090a0b0c0d0e0f \n").unwrap();
        assert_eq!(key.bytes()[15], 0x0f);
    }

    #[test]
    fn rejects_wrong_length() {
        assert!(AuthKey::from_hex("0001020304").is_err());
    }

    #[test]
    fn rejects_non_hex() {
        assert!(AuthKey::from_hex("zz0102030405060708090a0b0c0d0e0f").is_err());
    }

    #[test]
    fn debug_redacts_the_key() {
        let key = AuthKey::from_hex("000102030405060708090a0b0c0d0e0f").unwrap();
        let shown = format!("{key:?}");
        assert!(!shown.contains("0102"), "debug output leaked the key: {shown}");
        assert!(shown.contains("redacted"));
    }

    /// The form the account login prints, and the form the store keeps.
    #[test]
    fn a_key_goes_back_to_32_hex_digits() {
        let key = AuthKey::from_hex("0x000102030405060708090A0B0C0D0E0F").unwrap();
        assert!(key.to_hex() == FAKE_KEY, "not the key in lower case hex");
        assert!(AuthKey::from_hex(&key.to_hex()).unwrap().bytes() == key.bytes());
    }

    // --- one folder --------------------------------------------------------

    #[test]
    fn everything_is_kept_in_one_folder() {
        let ours = folder().expect("a home folder");
        assert_eq!(ours, dirs::config_dir().unwrap().join("BandBridge"));
        assert_eq!(Config::path(), Some(ours.join("config.toml")));
        #[cfg(target_os = "macos")]
        {
            let home = PathBuf::from(std::env::var_os("HOME").unwrap());
            assert_eq!(ours, home.join("Library/Application Support/BandBridge"));
        }
    }

    // --- the key in the system's secret store ------------------------------

    /// A MAC for the tests. No test puts it in the real store.
    const MAC: &str = "0A:1B:2C:3D:4E:5F";
    /// Other keys for the tests. None of them opens a band.
    const ENV_KEY: &str = "0f0e0d0c0b0a09080706050403020100";
    const STORED_KEY: &str = "101112131415161718191a1b1c1d1e1f";

    fn key(hex: &str) -> AuthKey {
        AuthKey::from_hex(hex).unwrap()
    }

    /// Whether this is the key written as `hex`. It prints nothing, so a
    /// failed test shows no key.
    fn is(found: &AuthKey, hex: &str) -> bool {
        found.bytes() == key(hex).bytes()
    }

    #[test]
    fn a_band_is_kept_under_its_mac_in_upper_case_with_colons() {
        for written in [MAC, "0a:1b:2c:3d:4e:5f", "0A-1B-2C-3D-4E-5F", " 0a1B2c3D4e5F "] {
            assert_eq!(account(written).unwrap(), MAC, "{written}");
        }
        for bad in ["", "0A:1B:2C:3D:4E", "0A:1B:2C:3D:4E:5F:60", "0G:1B:2C:3D:4E:5F", FAKE_KEY] {
            let err = account(bad).unwrap_err().to_string();
            assert!(err.contains("[device] mac is not a MAC address"), "{bad}: {err}");
            assert!(!quotes_a_key(&err), "{err}");
        }
    }

    /// BAND_AUTH_KEY wins, and the store is not even asked: a Keychain that
    /// asks the person first would ask for nothing.
    #[test]
    fn a_key_in_the_environment_comes_first() {
        let found = first_key(Some(MAC), Some(ENV_KEY.to_string()), |_| {
            panic!("the store was asked")
        })
        .unwrap();
        assert!(is(&found, ENV_KEY));

        // A bad one is an error, not a reason to look elsewhere.
        let err = first_key(Some(MAC), Some("0001".to_string()), |_| {
            panic!("the store was asked")
        })
        .unwrap_err();
        assert!(matches!(err, BandError::AuthKey(_)), "{err}");
    }

    #[test]
    fn the_store_comes_next_under_the_bands_mac() {
        let found = first_key(Some(MAC), None, |mac| {
            assert_eq!(mac, MAC);
            Ok(Some(key(STORED_KEY)))
        })
        .unwrap();
        assert!(is(&found, STORED_KEY));
    }

    /// No key in either place: the message says where it looked, names the
    /// band, and points at no file, because the store is the one place the
    /// key lives.
    #[test]
    fn no_key_says_where_it_looked() {
        let err = first_key(Some(MAC), None, |_| Ok(None)).unwrap_err();
        let err = err.to_string();
        assert!(err.starts_with("no auth key"), "{err}");
        assert!(err.contains("BAND_AUTH_KEY"), "{err}");
        assert!(err.contains(&format!("{STORE} holds no key for {MAC}")), "{err}");
        assert!(!err.contains(".config"), "{err}");

        let err = first_key(None, None, |_| panic!("no band, no MAC")).unwrap_err();
        assert!(err.to_string().contains("no band is paired"), "{err}");

        // A store that could not be read says why, rather than that there is
        // no key: there may well be one.
        let locked = |_: &str| Err(BandError::KeyStore("the Keychain is locked".to_string()));
        let err = first_key(Some(MAC), None, locked).unwrap_err();
        assert_eq!(err.to_string(), "the Keychain is locked");
    }

    /// Two of the store's errors carry what it holds. Nothing it holds comes
    /// back in a message.
    #[test]
    fn a_store_failure_never_quotes_what_the_store_holds() {
        let held = FAKE_KEY.as_bytes().to_vec();
        let platform = || -> Box<dyn std::error::Error + Send + Sync> { FAKE_KEY.into() };
        for err in [
            keyring::Error::BadEncoding(held.clone()),
            keyring::Error::BadDataFormat(held.clone(), platform()),
            keyring::Error::PlatformFailure(platform()),
            keyring::Error::Invalid("user".to_string(), FAKE_KEY.to_string()),
        ] {
            let message = store_failed("cannot read the key for", MAC, &err).to_string();
            assert!(message.contains(MAC), "{message}");
            assert!(!quotes_a_key(&message), "the message quoted the key: {message}");
        }
    }

    /// The store tests below use the real store, where a band's key may be
    /// the only copy. They keep to a service of their own.
    #[test]
    fn tests_keep_their_keys_under_a_service_of_their_own() {
        assert_eq!(SERVICE, "BandBridge-test");
    }

    /// The real store, under an account no band can have, because a MAC is
    /// never spelled this way. A guard deletes the item even when an
    /// assertion fails, so a failed run leaves nothing behind.
    #[test]
    fn the_store_keeps_replaces_reads_and_deletes_a_key() {
        let _turn = real_store();
        struct Cleanup(String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = delete_under(&self.0);
            }
        }
        let test_account = format!(
            "BandBridge-test-{}-{}",
            std::process::id(),
            rand::random::<u32>()
        );
        assert!(account(&test_account).is_err(), "a test account must never be a MAC");
        let _cleanup = Cleanup(test_account.clone());

        assert!(read_under(&test_account).unwrap().is_none(), "a new account holds a key");

        store_under(&test_account, &key(FAKE_KEY)).unwrap();
        let found = read_under(&test_account).unwrap().expect("the key just kept");
        assert!(is(&found, FAKE_KEY), "the store gave back another key");

        store_under(&test_account, &key(STORED_KEY)).unwrap();
        let found = read_under(&test_account).unwrap().expect("the key just kept");
        assert!(is(&found, STORED_KEY), "a second key did not replace the first");

        // Something put there by hand that is not a key is named, not quoted.
        entry(&test_account).unwrap().set_password("not a key at all").unwrap();
        let err = read_under(&test_account).unwrap_err().to_string();
        assert!(err.contains("holds a bad key for BandBridge-test-"), "{err}");
        assert!(!err.contains("not a key at all"), "{err}");

        delete_under(&test_account).unwrap();
        assert!(read_under(&test_account).unwrap().is_none(), "the delete left the key");
        delete_under(&test_account).expect("deleting a key that is gone is not an error");
    }

    // --- pairing: the key a person gives -----------------------------------

    /// 32 hex digits, with or without 0x, in either case. Anything else is
    /// named by what is wrong with it, and never quoted.
    #[test]
    fn a_key_is_32_hex_digits_with_or_without_0x() {
        for good in [
            FAKE_KEY.to_string(),
            format!("0x{FAKE_KEY}"),
            format!("0X{}", FAKE_KEY.to_ascii_uppercase()),
            format!("  {FAKE_KEY}\n"),
        ] {
            assert!(is(&AuthKey::from_hex(&good).unwrap(), FAKE_KEY), "{good:?} is the key");
        }

        let short = &FAKE_KEY[..31];
        let err = AuthKey::from_hex(short).unwrap_err().to_string();
        assert_eq!(
            err,
            "invalid auth key: a key is 32 hex digits, with or without 0x in front, and this one has 31"
        );

        // The length of a key in the form a UUID is printed in: 36.
        let dashed = "00010203-0405-0607-0809-0a0b0c0d0e0f";
        let err = AuthKey::from_hex(dashed).unwrap_err().to_string();
        assert!(err.ends_with("and this one has 36"), "{err}");

        let not_hex = format!("{}g", &FAKE_KEY[..31]);
        let err = AuthKey::from_hex(&not_hex).unwrap_err().to_string();
        assert!(err.contains("another character"), "{err}");
        for err in [err, AuthKey::from_hex(short).unwrap_err().to_string()] {
            assert!(!err.contains(&FAKE_KEY[..16]), "the message quoted the key: {err}");
        }
    }

    // --- forgetting a band -------------------------------------------------

    /// The plan's example: a band, settings that are not the defaults, and
    /// five actions.
    fn paired_example() -> Config {
        let mut config = example();
        config.settings.new_app_on_top = false;
        config
    }

    #[test]
    fn forget_with_keep_takes_the_band_and_its_key_and_keeps_the_rest() {
        let mut config = paired_example();
        let mut asked = Vec::new();
        let forgotten = config
            .forget(Setup::Keep, |mac| {
                asked.push(mac.to_string());
                Ok(())
            })
            .unwrap();

        assert_eq!(asked, ["00:00:5E:00:53:01"], "the key is taken out under the band's MAC");
        assert_eq!(forgotten.map(|device| device.name).as_deref(), Some("Xiaomi Smart Band 7"));
        assert_eq!(config.device, None);
        assert_eq!(config.actions, paired_example().actions);
        assert_eq!(config.settings, paired_example().settings);
    }

    #[test]
    fn forget_with_delete_also_clears_the_actions_and_the_settings() {
        let mut config = paired_example();
        let mut asked = 0;
        config
            .forget(Setup::Delete, |_| {
                asked += 1;
                Ok(())
            })
            .unwrap();

        assert_eq!(asked, 1);
        assert_eq!(config, Config::default(), "nothing is left but the defaults");
    }

    /// A key the store will not give up leaves the band paired, so a second
    /// try knows which key to take out.
    #[test]
    fn a_key_the_store_keeps_leaves_everything_as_it_was() {
        let mut config = paired_example();
        let err = config
            .forget(Setup::Delete, |_| {
                Err(BandError::KeyStore("the Keychain is locked".to_string()))
            })
            .unwrap_err();
        assert_eq!(err.to_string(), "the Keychain is locked");
        assert_eq!(config, paired_example());
    }

    /// With no band paired there is no key to take out. Delete still clears
    /// what a forget kept, which is how the next pair starts fresh.
    #[test]
    fn forget_with_no_band_asks_the_store_nothing() {
        let mut config = paired_example();
        config.device = None;
        let kept = config.clone();

        let forgotten = config
            .forget(Setup::Keep, |_| panic!("the store was asked"))
            .unwrap();
        assert_eq!(forgotten, None);
        assert_eq!(config, kept);

        config
            .forget(Setup::Delete, |_| panic!("the store was asked"))
            .unwrap();
        assert_eq!(config, Config::default());
    }

    /// Forget takes the key out of the real store. The store is asked under
    /// a test account, which no band can have, so no band's key is touched.
    #[test]
    fn forget_takes_the_key_out_of_the_real_store() {
        let test = TestAccount::new();
        store_under(&test.name, &key(FAKE_KEY)).unwrap();
        assert!(read_under(&test.name).unwrap().is_some());

        let mut config = paired_example();
        config
            .forget(Setup::Keep, |mac| {
                assert_eq!(mac, "00:00:5E:00:53:01");
                delete_under(&test.name)
            })
            .unwrap();

        assert!(read_under(&test.name).unwrap().is_none(), "the key is still in the store");
        assert_eq!(config.device, None);
    }

    /// A pair keeps the key the band took. A key the store holds already is
    /// left as it is, and a different one is replaced.
    #[test]
    fn a_pair_keeps_the_key_the_band_took() {
        let test = TestAccount::new();

        keep_under(&test.name, &key(FAKE_KEY)).unwrap();
        assert!(is(&read_under(&test.name).unwrap().unwrap(), FAKE_KEY), "a new key is kept");

        keep_under(&test.name, &key(FAKE_KEY)).unwrap();
        assert!(is(&read_under(&test.name).unwrap().unwrap(), FAKE_KEY), "the same key stays");

        keep_under(&test.name, &key(STORED_KEY)).unwrap();
        assert!(is(&read_under(&test.name).unwrap().unwrap(), STORED_KEY), "another key replaces it");
    }

    /// An account in the real store that no band can have, because a MAC is
    /// never spelled this way. Dropping it deletes whatever it holds, so a
    /// test that fails leaves nothing behind. It holds the store's turn
    /// until then: see [`real_store`].
    struct TestAccount {
        name: String,
        _turn: std::sync::MutexGuard<'static, ()>,
    }

    impl TestAccount {
        fn new() -> Self {
            let turn = real_store();
            let name = format!(
                "BandBridge-test-{}-{}",
                std::process::id(),
                rand::random::<u32>()
            );
            assert!(account(&name).is_err(), "a test account must never be a MAC");
            Self { name, _turn: turn }
        }
    }

    /// The tests that use the real store take turns. Windows Credential
    /// Manager can lose a key written while another write is under way, and
    /// a read then finds nothing.
    fn real_store() -> std::sync::MutexGuard<'static, ()> {
        static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
        TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    impl Drop for TestAccount {
        fn drop(&mut self) {
            let _ = delete_under(&self.name);
        }
    }

    #[test]
    fn the_saved_setup_says_what_the_next_pair_can_reuse() {
        let setup = paired_example().saved_setup();
        assert_eq!(
            setup.device,
            Some(PairedBand {
                name: "Xiaomi Smart Band 7".to_string(),
                mac: "00:00:5E:00:53:01".to_string(),
            })
        );
        assert_eq!(setup.actions, 5);
        assert!(setup.settings_changed);

        let json = serde_json::to_value(&setup).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "device": { "name": "Xiaomi Smart Band 7", "mac": "00:00:5E:00:53:01" },
                "actions": 5,
                "settingsChanged": true,
            }),
            "the window reads these names"
        );
        assert!(!json.to_string().contains("01234567"), "the id is never shown");

        let empty = Config::default().saved_setup();
        assert_eq!((empty.device, empty.actions, empty.settings_changed), (None, 0, false));
    }

    #[test]
    fn a_found_band_keeps_its_id_in_lower_case() {
        let device = Device::found(" Band ", "00:00:5E:00:53:01", "01234567-89AB-4CDE-8F01-23456789ABCD");
        assert_eq!(device.name, "Band");
        assert_eq!(device.uuid.as_deref(), Some("01234567-89ab-4cde-8f01-23456789abcd"));
    }

    #[test]
    fn a_choice_is_written_keep_or_delete() {
        assert_eq!(serde_json::to_string(&Setup::Keep).unwrap(), "\"keep\"");
        assert_eq!(serde_json::from_str::<Setup>("\"delete\"").unwrap(), Setup::Delete);
        assert_eq!(Setup::default(), Setup::Keep, "keeping loses nothing, so it is the default");
    }

    // --- the uninstaller's "Delete the application data" -------------------

    /// A folder like BandBridge's, in a folder of its own that the test
    /// deletes: the config, and the learned app names.
    fn kept_folder(config: &str) -> (tempfile::TempDir, PathBuf) {
        let home = tempfile::tempdir().unwrap();
        let folder = home.path().join("BandBridge");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("config.toml"), config).unwrap();
        std::fs::write(folder.join("app-names.toml"), "58E537B5 = \"BandBridge\"\n").unwrap();
        (home, folder)
    }

    #[test]
    fn deleting_app_data_takes_the_key_then_the_folder() {
        let (_home, folder) = kept_folder(&paired_example().to_toml().unwrap());
        let mut asked = Vec::new();
        delete_app_data_in(&folder, |mac| {
            assert!(folder.join("config.toml").exists(), "the folder went before the key");
            asked.push(mac.to_string());
            Ok(())
        })
        .unwrap();

        assert_eq!(asked, ["00:00:5E:00:53:01"], "the key is taken out under the band's MAC");
        assert!(!folder.exists(), "the folder is still there");
    }

    #[test]
    fn deleting_app_data_with_no_band_takes_only_the_folder() {
        let (_home, folder) = kept_folder(&Config::default().to_toml().unwrap());
        delete_app_data_in(&folder, |_| panic!("the store was asked")).unwrap();
        assert!(!folder.exists());
    }

    #[test]
    fn deleting_app_data_that_is_not_there_is_not_an_error() {
        let home = tempfile::tempdir().unwrap();
        delete_app_data_in(&home.path().join("BandBridge"), |_| panic!("the store was asked"))
            .unwrap();
    }

    /// A key the store will not give up keeps the folder, so the config
    /// still names the band whose key is left.
    #[test]
    fn a_key_the_store_keeps_keeps_the_folder() {
        let (_home, folder) = kept_folder(&paired_example().to_toml().unwrap());
        let err = delete_app_data_in(&folder, |_| {
            Err(BandError::KeyStore("Credential Manager is locked".to_string()))
        })
        .unwrap_err();

        assert!(err.to_string().contains("Credential Manager is locked"), "{err}");
        assert!(folder.join("config.toml").exists(), "the config went with the key still kept");
    }

    /// The band is read from the file as plain tables, so a config that a
    /// broken action keeps from loading still names its band.
    #[test]
    fn a_config_with_a_broken_action_still_names_its_band() {
        let text = "[device]\nname = \"Band\"\nmac = \"00-00-5e-00-53-01\"\n\n\
                    [[actions]]\nname = \"Old\"\nsignal = \"bb01\"\nshell = \"x\"\nwhen = \"now\"\n";
        assert!(Config::parse(text).is_err(), "the example must not load");
        let (_home, folder) = kept_folder(text);
        let mut asked = Vec::new();
        delete_app_data_in(&folder, |mac| {
            asked.push(mac.to_string());
            Ok(())
        })
        .unwrap();

        assert_eq!(asked, ["00-00-5e-00-53-01"], "the store takes the MAC as the file spells it");
        assert!(!folder.exists());
    }

    /// A key is only ever kept under a MAC, so a band named by something
    /// else has none to take.
    #[test]
    fn a_band_with_no_mac_address_has_no_key_to_take() {
        let (_home, folder) = kept_folder("[device]\nname = \"Band\"\nmac = \"not a mac\"\n");
        delete_app_data_in(&folder, |_| panic!("the store was asked")).unwrap();
        assert!(!folder.exists());
    }

    /// A file that is not TOML names no band, so the key cannot be found.
    /// Everything stays, and the error says where the file breaks.
    #[test]
    fn a_config_that_is_not_toml_keeps_everything() {
        let (_home, folder) = kept_folder("[device\nmac = \"00:00:5E:00:53:01\"\n");
        let err = delete_app_data_in(&folder, |_| panic!("the store was asked")).unwrap_err();
        assert!(err.to_string().contains("line 1"), "{err}");
        assert!(folder.join("config.toml").exists());
    }

    // --- the file format ---------------------------------------------------

    /// The design notes' example config ("The config format"), with example
    /// values in place of the band's address and id.
    const PLAN_EXAMPLE: &str = r#"# ~/Library/Application Support/BandBridge/config.toml
# BandBridge writes this file. Change it in the window.

[device]
name = "Xiaomi Smart Band 7"
mac = "00:00:5E:00:53:01"   # read from the band
uuid = "01234567-89AB-4CDE-8F01-23456789ABCD"   # how this Mac reaches it; found by BandBridge, never shown
# The auth key is in the system's secret store, not here.

[settings]
auto_reconnect = true
new_app_on_top = true
appearance = "system"       # light, dark, or system

[[actions]]
name = "Screenshot a region"
signal = "bb01"
keystroke = { key_code = "4", modifiers = ["left_command", "left_shift"] }

[[actions]]
name = "Start a focus block"
signal = "bb02"
shortcut = "Start Focus"

[[actions]]
name = "Open Music"
signal = "bb03"
shell = "open -a Music"

[[actions]]
name = "Toggle the desk lamp"
signal = "bb06"
script = "~/scripts/toggle-desk-lamp.sh"

[[actions]]
name = "Turn on the desk lights"
signal = "bb04"
web_request = { method = "POST", url = "http://lights.local/webhook", headers = ["Authorization: Bearer …"], body = '{"on": true}' }
"#;

    /// What BandBridge writes for the plan's example: the plan's own lines,
    /// less its comments.
    const SAVED: &str = r#"# Mi Band Bridge writes this file. Change it in the window.

[device]
name = "Xiaomi Smart Band 7"
mac = "00:00:5E:00:53:01"
uuid = "01234567-89AB-4CDE-8F01-23456789ABCD"

[settings]
auto_reconnect = true
new_app_on_top = true
appearance = "system"

[[actions]]
name = "Screenshot a region"
signal = "bb01"
keystroke = { key_code = "4", modifiers = ["left_command", "left_shift"] }

[[actions]]
name = "Start a focus block"
signal = "bb02"
shortcut = "Start Focus"

[[actions]]
name = "Open Music"
signal = "bb03"
shell = "open -a Music"

[[actions]]
name = "Toggle the desk lamp"
signal = "bb06"
script = "~/scripts/toggle-desk-lamp.sh"

[[actions]]
name = "Turn on the desk lights"
signal = "bb04"
web_request = { method = "POST", url = "http://lights.local/webhook", headers = ["Authorization: Bearer …"], body = '{"on": true}' }
"#;

    /// The plan's example as this system loads it. Windows has no
    /// Shortcuts, so there the example's shortcut is a shell line of the same
    /// name (W5). The rest is the plan's, word for word.
    fn plan_example() -> String {
        on_this_system(PLAN_EXAMPLE)
    }

    /// What BandBridge writes for [`plan_example`].
    fn saved() -> String {
        on_this_system(SAVED)
    }

    fn on_this_system(text: &str) -> String {
        match crate::platform::actions::SHORTCUTS_REFUSED {
            Some(_) => text.replace("shortcut = \"Start Focus\"", "shell = \"Start Focus\""),
            None => text.to_string(),
        }
    }

    /// What the example's focus block does on this system: see
    /// [`plan_example`].
    fn focus() -> Does {
        match crate::platform::actions::SHORTCUTS_REFUSED {
            Some(_) => Does::Shell("Start Focus".to_string()),
            None => Does::Shortcut("Start Focus".to_string()),
        }
    }

    fn example() -> Config {
        Config::parse(&plan_example()).unwrap()
    }

    fn action(name: &str, signal: &str, does: Does) -> Action {
        Action {
            name: name.to_string(),
            signal: signal.to_string(),
            does,
        }
    }

    #[test]
    fn the_plan_example_loads() {
        let config = example();

        let device = config.device.as_ref().expect("[device]");
        assert_eq!(device.name, "Xiaomi Smart Band 7");
        assert_eq!(device.mac, "00:00:5E:00:53:01");
        assert_eq!(config.uuid(), Some("01234567-89AB-4CDE-8F01-23456789ABCD"));
        assert_eq!(config.settings, Settings::default());

        assert_eq!(
            config.actions,
            vec![
                action(
                    "Screenshot a region",
                    "bb01",
                    Does::Keystroke(Keystroke {
                        key_code: Some("4".to_string()),
                        consumer_key_code: None,
                        modifiers: vec!["left_command".to_string(), "left_shift".to_string()],
                    }),
                ),
                action("Start a focus block", "bb02", focus()),
                action("Open Music", "bb03", Does::Shell("open -a Music".to_string())),
                action(
                    "Toggle the desk lamp",
                    "bb06",
                    Does::Script("~/scripts/toggle-desk-lamp.sh".to_string()),
                ),
                action(
                    "Turn on the desk lights",
                    "bb04",
                    Does::WebRequest(WebRequest {
                        method: Some("POST".to_string()),
                        url: "http://lights.local/webhook".to_string(),
                        headers: vec!["Authorization: Bearer …".to_string()],
                        body: Some("{\"on\": true}".to_string()),
                    }),
                ),
            ]
        );
    }

    /// Save writes the format the plan draws, line for line.
    #[test]
    fn save_writes_the_plan_format() {
        assert_eq!(example().to_toml().unwrap(), saved());
    }

    /// What save wrote loads as an equal config, and a file BandBridge wrote
    /// goes through a load and a save unchanged.
    #[test]
    fn a_saved_file_round_trips_unchanged() {
        let config = example();
        let saved = config.to_toml().unwrap();
        let loaded = Config::parse(&saved).unwrap();
        assert_eq!(loaded, config);
        assert_eq!(loaded.to_toml().unwrap(), saved);
    }

    /// Every kind is written under the key serde reads it from, including
    /// what the plan's example leaves out.
    #[test]
    fn every_kind_round_trips() {
        let config = Config {
            device: Some(Device {
                name: "Band".to_string(),
                mac: "00:00:5E:00:53:01".to_string(),
                uuid: None,
            }),
            settings: Settings {
                auto_reconnect: false,
                new_app_on_top: false,
                appearance: Appearance::Dark,
            },
            actions: vec![
                action(
                    "Play",
                    "bb05",
                    Does::Keystroke(Keystroke {
                        consumer_key_code: Some("play_or_pause".to_string()),
                        ..Keystroke::default()
                    }),
                ),
                action("Say hi", "bb07", Does::Shell("say \"hi\" | cat".to_string())),
                action("Lamp", "bb08", Does::Script("/tmp/lamp.sh".to_string())),
                action("Focus", "bb09", Does::Shortcut("Start Focus".to_string())),
                action(
                    "Ping",
                    "bb0a",
                    Does::WebRequest(WebRequest {
                        url: "https://example.com/hook".to_string(),
                        body: Some("{\n  \"on\": true\n}".to_string()),
                        ..WebRequest::default()
                    }),
                ),
            ],
        };
        // Every kind this system runs: Windows has no Shortcuts (W5).
        let mut config = config;
        let runs_shortcuts = crate::platform::actions::SHORTCUTS_REFUSED.is_none();
        config.actions.retain(|action| runs_shortcuts || action.does.kind() != "shortcut");
        let saved = config.to_toml().unwrap();
        assert_eq!(Config::parse(&saved).unwrap(), config, "{saved}");

        let kinds: Vec<&str> = config.actions.iter().map(|a| a.does.kind()).collect();
        let every_kind: Vec<&str> = Does::KINDS
            .into_iter()
            .filter(|kind| runs_shortcuts || *kind != "shortcut")
            .collect();
        assert_eq!(kinds, every_kind);
        for action in &config.actions {
            let written = toml::Table::try_from(action).unwrap();
            assert!(written.contains_key(action.does.kind()), "{written:?}");
        }
    }

    #[test]
    fn save_and_load_go_through_a_file() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("nested/config.toml");
        let config = example();
        config.save_to(&path).unwrap();
        assert_eq!(Config::load_from(&path).unwrap(), config);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.starts_with("# Mi Band Bridge writes this file."),
            "{text}"
        );
        assert!(!folder.path().join("nested/config.toml.partial").exists());
    }

    /// A config that breaks a rule is never written, so the file on disk
    /// always loads.
    #[test]
    fn save_refuses_a_config_that_breaks_a_rule() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("config.toml");
        let mut config = example();
        config.actions[1].name = " ".to_string();
        assert!(config.save_to(&path).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn a_missing_file_is_the_defaults() {
        let folder = tempfile::tempdir().unwrap();
        let config = Config::load_from(&folder.path().join("absent.toml")).unwrap();
        assert_eq!(config, Config::default());
    }

    #[test]
    fn an_empty_file_is_the_defaults() {
        let config = Config::parse("").unwrap();
        assert_eq!(config, Config::default());
        assert_eq!(config.device, None);
        assert!(config.settings.auto_reconnect, "must default to true, not false");
        assert!(config.settings.new_app_on_top, "on by default");
        assert_eq!(config.settings.appearance, Appearance::System);
        assert!(config.actions.is_empty());
        assert_eq!(Config::parse(&config.to_toml().unwrap()).unwrap(), config);
    }

    #[test]
    fn a_setting_left_out_keeps_its_default() {
        let config = Config::parse("[settings]\nauto_reconnect = false\n").unwrap();
        assert!(!config.settings.auto_reconnect);
        assert!(config.settings.new_app_on_top);
        assert_eq!(config.settings.appearance, Appearance::System);

        let config = Config::parse("[settings]\nappearance = \"dark\"\n").unwrap();
        assert_eq!(config.settings.appearance, Appearance::Dark);
    }

    // --- changing the settings from the window -----------------------------

    /// The plan's example, written where a test can change it.
    fn example_file() -> (tempfile::TempDir, PathBuf) {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("config.toml");
        std::fs::write(&path, saved()).unwrap();
        (folder, path)
    }

    fn change(json: &str) -> SettingsChange {
        serde_json::from_str(json).unwrap()
    }

    /// A change writes `[settings]`, and writes `[device]` and every action
    /// back as they were: the file differs from before in the changed lines
    /// alone.
    #[test]
    fn a_settings_change_writes_the_settings_and_nothing_else() {
        let (_folder, path) = example_file();

        let settings =
            Config::change_settings_at(&path, &change(r#"{"auto_reconnect": false}"#)).unwrap();
        assert_eq!(
            settings,
            Settings {
                auto_reconnect: false,
                ..Settings::default()
            }
        );
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            text,
            saved().replace("auto_reconnect = true", "auto_reconnect = false")
        );

        let settings = Config::change_settings_at(
            &path,
            &change(r#"{"new_app_on_top": false, "appearance": "dark"}"#),
        )
        .unwrap();
        assert_eq!(
            settings,
            Settings {
                auto_reconnect: false,
                new_app_on_top: false,
                appearance: Appearance::Dark,
            },
            "a setting left out of a change stays as the file has it"
        );
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.settings, settings);
        assert_eq!(loaded.device, example().device);
        assert_eq!(loaded.actions, example().actions);
    }

    /// A change that changes nothing leaves the file alone, so the band task
    /// has nothing to read again.
    #[test]
    fn a_settings_change_that_changes_nothing_writes_nothing() {
        let (_folder, path) = example_file();
        // Written by hand, with a comment a save would drop.
        let by_hand = format!("{}# a note of my own\n", plan_example());
        std::fs::write(&path, &by_hand).unwrap();

        let settings =
            Config::change_settings_at(&path, &change(r#"{"auto_reconnect": true}"#)).unwrap();
        assert_eq!(settings, Settings::default());
        let settings = Config::change_settings_at(&path, &change("{}")).unwrap();
        assert_eq!(settings, Settings::default());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            by_hand,
            "the file was written"
        );
    }

    /// A file that does not load is not written: a change of one setting
    /// must never replace actions it could not read.
    #[test]
    fn a_settings_change_to_a_file_that_does_not_load_writes_nothing() {
        let (_folder, path) = example_file();
        let broken = saved().replace("new_app_on_top = true", "new_app_on_top = \"yes\"");
        std::fs::write(&path, &broken).unwrap();

        let err = Config::change_settings_at(&path, &change(r#"{"appearance": "light"}"#))
            .unwrap_err()
            .to_string();
        assert!(err.contains("line 10"), "{err}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
    }

    /// With no file yet, a change makes one that holds the settings.
    #[test]
    fn a_settings_change_with_no_file_makes_one() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("config.toml");
        Config::change_settings_at(&path, &change(r#"{"appearance": "light"}"#)).unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.settings.appearance, Appearance::Light);
        assert_eq!(loaded.device, None);
        assert!(loaded.actions.is_empty());
    }

    /// The window names each setting as the file does. A name it misspells,
    /// or a value a setting cannot take, is refused rather than ignored,
    /// because an ignored change is a switch that moves and does nothing.
    #[test]
    fn a_settings_change_names_each_setting_as_the_file_does() {
        assert_eq!(
            change(r#"{"auto_reconnect": false, "new_app_on_top": true, "appearance": "system"}"#),
            SettingsChange {
                auto_reconnect: Some(false),
                new_app_on_top: Some(true),
                appearance: Some(Appearance::System),
            }
        );
        assert_eq!(change("{}"), SettingsChange::default());
        for bad in [
            r#"{"auto_reconect": false}"#,
            r#"{"appearance": "sepia"}"#,
            r#"{"auto_reconnect": "no"}"#,
        ] {
            assert!(
                serde_json::from_str::<SettingsChange>(bad).is_err(),
                "{bad} was taken"
            );
        }
        // And the settings go back to the window by the same names.
        assert_eq!(
            serde_json::to_value(Settings::default()).unwrap(),
            serde_json::json!({
                "auto_reconnect": true,
                "new_app_on_top": true,
                "appearance": "system",
            })
        );
    }

    #[test]
    fn a_device_needs_no_uuid() {
        let config = Config::parse("[device]\nname = \"Band\"\nmac = \"00:00:5E:00:53:01\"\n").unwrap();
        assert_eq!(config.uuid(), None);
        assert_eq!(config.device.map(|device| device.name).as_deref(), Some("Band"));
    }

    #[test]
    fn a_malformed_file_says_which_line_and_quotes_nothing() {
        let err = refused("[settings]\nauto_reconnect = \"yes\"\n");
        assert!(err.contains("line 2"), "{err}");

        // An inline table left open, on the line that holds a token.
        let err = refused(&open_music(
            "signal = \"bb03\"\nweb_request = { url = \"https://example.com\", headers = [\"Authorization: Bearer s3cret\"]",
        ));
        assert!(err.contains("line 4"), "{err}");
        assert!(!err.contains("s3cret"), "the message quoted the file: {err}");
    }

    // --- actions from the window's editor ----------------------------------

    /// The editor sends an action in the file's own shape, as JSON, and gets
    /// the list in it: each kind reads back as the file reads it.
    #[test]
    fn every_kind_from_the_editor_reads_as_the_file_reads_it() {
        let mut actions = example().actions;
        actions.push(action(
            "Play",
            "bb05",
            Does::Keystroke(Keystroke {
                consumer_key_code: Some("play_or_pause".to_string()),
                ..Keystroke::default()
            }),
        ));
        actions.push(action(
            "Ping",
            "bb0a",
            Does::WebRequest(WebRequest {
                url: "https://example.com/hook".to_string(),
                body: Some("{\n  \"on\": true\n}".to_string()),
                ..WebRequest::default()
            }),
        ));
        for action in actions {
            let written = serde_json::to_value(&action).unwrap();
            assert_eq!(Action::from_editor(&written).unwrap(), action, "{written}");
        }

        // The shape the page reads and writes.
        assert_eq!(
            serde_json::to_value(&example().actions[0]).unwrap(),
            serde_json::json!({
                "name": "Screenshot a region",
                "signal": "bb01",
                "keystroke": {"key_code": "4", "modifiers": ["left_command", "left_shift"]},
            })
        );
        assert_eq!(
            serde_json::to_value(&example().actions[4]).unwrap(),
            serde_json::json!({
                "name": "Turn on the desk lights",
                "signal": "bb04",
                "web_request": {
                    "method": "POST",
                    "url": "http://lights.local/webhook",
                    "headers": ["Authorization: Bearer …"],
                    "body": "{\"on\": true}",
                },
            })
        );
    }

    /// The name is kept without the spaces around it, and the signal as the
    /// band sends it.
    #[test]
    fn the_editor_keeps_a_name_and_a_signal_as_the_file_keeps_them() {
        let read = Action::from_editor(&serde_json::json!({
            "name": "  Open Music ",
            "signal": " BB03 ",
            "shell": "open -a Music",
        }))
        .unwrap();
        assert_eq!(
            read,
            action(
                "Open Music",
                "bb03",
                Does::Shell("open -a Music".to_string())
            )
        );
    }

    /// Each rule an action from the editor breaks is named by its field, so
    /// the editor shows it there. These are the file's rules, the ones the
    /// loader enforces, and one more: a name that looks like a key.
    #[test]
    fn each_problem_names_its_field() {
        let open_music = |more: serde_json::Value| {
            let mut written = serde_json::json!({"name": "Open Music", "signal": "bb03"});
            written
                .as_object_mut()
                .unwrap()
                .extend(more.as_object().unwrap().clone());
            written
        };
        let json = |text: &str| -> serde_json::Value { serde_json::from_str(text).unwrap() };
        let cases: Vec<(serde_json::Value, Option<&str>, &str)> = vec![
            (
                json(r#"{"name": " ", "signal": "bb03", "shell": "open -a Music"}"#),
                Some("name"),
                "the action needs a name, which the window and the event log show",
            ),
            (
                serde_json::json!({"name": format!("Lamp {FAKE_KEY}"), "signal": "bb03", "shell": "x"}),
                Some("name"),
                "the name looks like a key",
            ),
            (
                json(r#"{"name": 7, "signal": "bb03", "shell": "x"}"#),
                Some("name"),
                "the name must be text",
            ),
            (
                json(r#"{"name": "Open Music", "shell": "x"}"#),
                Some("signal"),
                "the action needs a signal, such as bb04",
            ),
            (
                json(r#"{"name": "Open Music", "signal": "music:play", "shell": "x"}"#),
                Some("signal"),
                "\"music:play\" is not a signal",
            ),
            (
                json(r#"{"name": "Open Music", "signal": "0201", "shell": "x"}"#),
                Some("signal"),
                "\"0201\" is not a signal. A signal is bb followed by one to seven pairs of hex \
                 digits, such as bb01 or bb0401.",
            ),
            (
                json(r#"{"name": "Open Music", "signal": "bb03"}"#),
                Some("kind"),
                "the action does nothing. Give it one of keystroke, shell, script, shortcut, \
                 web_request.",
            ),
            (
                open_music(json(
                    r#"{"script": "~/music.sh", "shell": "open -a Music"}"#,
                )),
                Some("kind"),
                "the action sets shell and script. Give it one kind.",
            ),
            (
                open_music(json(r#"{"shell": " "}"#)),
                Some("shell"),
                "shell is empty",
            ),
            (
                open_music(json(r#"{"script": ""}"#)),
                Some("script"),
                "script is empty",
            ),
            (
                open_music(json(r#"{"shortcut": ""}"#)),
                Some("shortcut"),
                "shortcut is empty",
            ),
            (
                open_music(json(r#"{"shell": 5}"#)),
                Some("shell"),
                "invalid type",
            ),
            (
                open_music(json(
                    r#"{"keystroke": {"key_code": "a", "consumer_key_code": "mute"}}"#,
                )),
                Some("keystroke"),
                "a keystroke takes key_code or consumer_key_code, not both",
            ),
            (
                open_music(json(r#"{"keystroke": {"modifiers": ["command"]}}"#)),
                Some("keystroke"),
                "a keystroke needs key_code or consumer_key_code",
            ),
            (
                open_music(json(
                    r#"{"keystroke": {"key_code": "4", "modifers": ["command"]}}"#,
                )),
                Some("keystroke"),
                "modifers",
            ),
            (
                open_music(json(r#"{"keystroke": {"key_code": "meta"}}"#)),
                Some("keystroke.key_code"),
                "unknown key_code meta",
            ),
            (
                open_music(json(r#"{"keystroke": {"key_code": "vk_none"}}"#)),
                Some("keystroke.key_code"),
                "key_code vk_none cannot be sent. this disables a key in Karabiner. It sends \
                 nothing.",
            ),
            (
                open_music(json(
                    r#"{"keystroke": {"key_code": "a", "modifiers": ["hyper"]}}"#,
                )),
                Some("keystroke.modifiers"),
                "unknown modifier hyper",
            ),
            (
                open_music(json(
                    r#"{"keystroke": {"consumer_key_code": "brightness_up"}}"#,
                )),
                Some("keystroke.consumer_key_code"),
                "unknown consumer_key_code brightness_up",
            ),
            (
                open_music(json(
                    r#"{"keystroke": {"consumer_key_code": "mute", "modifiers": ["command"]}}"#,
                )),
                Some("keystroke.modifiers"),
                "a media key takes no modifiers",
            ),
            (
                open_music(json(r#"{"web_request": {"url": "file:///etc/passwd"}}"#)),
                Some("web_request.url"),
                "the url must start with http:// or https://",
            ),
            (
                open_music(json(
                    r#"{"web_request": {"url": "https://example.com", "method": "FETCH"}}"#,
                )),
                Some("web_request.method"),
                "unknown method FETCH",
            ),
            (
                open_music(json(
                    r#"{"web_request": {"url": "https://example.com",
                        "headers": ["Accept: */*", "Authorization Bearer s3cret"]}}"#,
                )),
                Some("web_request.headers"),
                "header 2 must read Name: value",
            ),
            (
                open_music(json(r#"{"shell": "x", "modifiers": ["command"]}"#)),
                None,
                "modifiers is not part of an action",
            ),
            (
                json(r#""open -a Music""#),
                None,
                "an action is a name, a signal, and one of",
            ),
        ];
        for (written, field, says) in cases {
            let refused = Action::from_editor(&written).unwrap_err();
            assert_eq!(refused.errors.len(), 1, "{written}: {refused}");
            assert_eq!(refused.errors[0].field, field, "{written}: {refused}");
            assert!(
                refused.errors[0].message.contains(says),
                "{written}: {refused}"
            );
            assert!(!quotes_a_key(&refused.to_string()), "{refused}");
            assert!(!refused.to_string().contains("s3cret"), "{refused}");
        }
    }

    /// Every problem comes back at once, one for each field that has one, in
    /// the order the editor shows its fields: name, signal, then what the
    /// action does. The page gets them in this shape.
    #[test]
    fn every_problem_comes_back_at_once() {
        let refused = Action::from_editor(&serde_json::json!({
            "name": "",
            "signal": "bb",
            "web_request": {"url": "ftp://example.com", "method": "FETCH"},
        }))
        .unwrap_err();
        assert_eq!(
            serde_json::to_value(&refused).unwrap(),
            serde_json::json!({"errors": [
                {"field": "name",
                 "message": "the action needs a name, which the window and the event log show"},
                {"field": "signal",
                 "message": "\"bb\" is not a signal. A signal is bb followed by one to seven \
                             pairs of hex digits, such as bb01 or bb0401."},
                {"field": "web_request.url",
                 "message": "the url must start with http:// or https://"},
            ]})
        );
    }

    /// A try runs what an action does, before it has a name or a signal, so
    /// only what it does is read.
    #[test]
    fn a_try_reads_only_what_the_action_does() {
        assert_eq!(
            Does::from_editor(&serde_json::json!({"shell": "say hi"})).unwrap(),
            Does::Shell("say hi".to_string())
        );
        assert_eq!(
            Does::from_editor(&serde_json::json!({"name": "", "signal": "", "shell": "say hi"}))
                .unwrap(),
            Does::Shell("say hi".to_string())
        );
        let refused =
            Does::from_editor(&serde_json::json!({"name": "", "web_request": {"url": "x"}}))
                .unwrap_err();
        let fields: Vec<_> = refused.errors.iter().map(|problem| problem.field).collect();
        assert_eq!(fields, [Some("web_request.url")]);
        let refused = Does::from_editor(&serde_json::json!({"name": "Lamp"})).unwrap_err();
        assert_eq!(refused.errors[0].field, Some("kind"));
    }

    /// An action in the shape the editor sends it.
    fn written(action: &Action) -> serde_json::Value {
        serde_json::to_value(action).unwrap()
    }

    /// An action as the window's list showed it: its place, name and signal.
    fn listed(at: usize, name: &str, signal: &str) -> Listed {
        Listed {
            at,
            name: name.to_string(),
            signal: signal.to_string(),
        }
    }

    /// A new action goes after the others. The file keeps `[device]`, the
    /// settings and every other action as they were.
    #[test]
    fn a_new_action_goes_after_the_others() {
        let (_folder, path) = example_file();
        let hi = action("Say hi", "bb07", Does::Shell("say hi".to_string()));
        let actions = Config::save_action_at(&path, None, &written(&hi)).unwrap();

        let mut expected = example().actions;
        expected.push(hi);
        assert_eq!(actions, expected);
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.actions, expected);
        assert_eq!(
            (loaded.device, loaded.settings),
            (example().device, example().settings)
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            format!(
                "{}\n[[actions]]\nname = \"Say hi\"\nsignal = \"bb07\"\nshell = \"say hi\"\n",
                saved()
            )
        );
    }

    /// An edited action keeps its place, found by where the list showed it,
    /// with the name and the signal it had then, in either case. It may keep
    /// that signal, or take another. The file differs in its lines alone.
    #[test]
    fn an_edited_action_keeps_its_place() {
        let (_folder, path) = example_file();
        let edited = action(
            "Play Music",
            "bb05",
            Does::Shell("open -a Music".to_string()),
        );
        let was = listed(2, "Open Music", "BB03");
        let actions = Config::save_action_at(&path, Some(&was), &written(&edited)).unwrap();
        assert_eq!(actions.len(), 5);
        assert_eq!(actions[2], edited);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            saved().replace(
                "name = \"Open Music\"\nsignal = \"bb03\"",
                "name = \"Play Music\"\nsignal = \"bb05\""
            )
        );

        let renamed = action("Music", "bb05", Does::Shell("open -a Music".to_string()));
        let was = listed(2, "Play Music", "bb05");
        let actions = Config::save_action_at(&path, Some(&was), &written(&renamed)).unwrap();
        assert_eq!(actions[2], renamed, "an action may keep its own signal");
    }

    /// Actions may share a signal (D6, changed 2026-09-26). A new action on a
    /// signal another has goes after the others, and an edit may take a
    /// signal another action has.
    #[test]
    fn the_editor_may_give_an_action_a_signal_another_has() {
        let (_folder, path) = example_file();
        let lamp = action("Lamp", "BB01", Does::Shell("lamp".to_string()));
        let actions = Config::save_action_at(&path, None, &written(&lamp)).unwrap();
        assert_eq!(actions.len(), 6);
        let saved = action("Lamp", "bb01", Does::Shell("lamp".to_string()));
        assert_eq!(actions[5], saved, "saved, with its signal in lower case");
        assert_eq!(actions[0].signal, "bb01", "the first keeps its signal");

        let music = action(
            "Music too",
            "bb01",
            Does::Shell("open -a Music".to_string()),
        );
        let was = listed(2, "Open Music", "bb03");
        let actions = Config::save_action_at(&path, Some(&was), &written(&music)).unwrap();
        assert_eq!(actions[2], music);
        let on_bb01: Vec<&str> = actions
            .iter()
            .filter(|action| action.signal.eq_ignore_ascii_case("bb01"))
            .map(|action| action.name.as_str())
            .collect();
        assert_eq!(on_bb01, ["Screenshot a region", "Music too", "Lamp"]);
        assert_eq!(Config::load_from(&path).unwrap().actions, actions);
    }

    /// With two actions on one signal, an edit and a delete each reach the
    /// one the list showed at that place, and no other.
    #[test]
    fn an_edit_and_a_delete_find_their_action_by_its_place() {
        let (_folder, path) = example_file();
        let lamp = action("Lamp", "bb01", Does::Shell("lamp".to_string()));
        Config::save_action_at(&path, None, &written(&lamp)).unwrap();

        let dim = action("Dim the lamp", "bb01", Does::Shell("lamp dim".to_string()));
        let actions =
            Config::save_action_at(&path, Some(&listed(5, "Lamp", "bb01")), &written(&dim))
                .unwrap();
        assert_eq!(actions[5], dim);
        assert_eq!(actions[0], example().actions[0], "the other bb01 action");

        let actions =
            Config::delete_action_at(&path, &listed(0, "Screenshot a region", "bb01")).unwrap();
        assert_eq!(actions.len(), 5);
        assert_eq!(actions[4], dim, "the other bb01 action stays");
        assert_eq!(actions[0], example().actions[1]);
    }

    /// Every problem comes back at once, in the order of the editor's
    /// fields, so the person fixes them in one go.
    #[test]
    fn every_problem_comes_back_in_the_order_of_the_fields() {
        let (_folder, path) = example_file();
        let refused = Config::save_action_at(
            &path,
            None,
            &serde_json::json!({"name": " ", "signal": "BB04", "shell": ""}),
        )
        .unwrap_err();
        let fields: Vec<_> = refused.errors.iter().map(|problem| problem.field).collect();
        assert_eq!(fields, [Some("name"), Some("shell")], "{refused}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), saved());
    }

    /// An action that is no longer where the list showed it, or no longer
    /// has the name or signal the list showed, is not saved: the file
    /// changed after the list was read, and the person edited that one. A
    /// file that does not load is not written. Neither problem is one
    /// field's.
    #[test]
    fn an_action_is_saved_only_over_what_the_file_holds() {
        let (_folder, path) = example_file();
        let hi = action("Say hi", "bb07", Does::Shell("say hi".to_string()));
        for was in [
            listed(9, "Gone", "bb09"),
            listed(2, "Open Music", "bb09"),
            listed(2, "Something else", "bb03"),
        ] {
            let refused = Config::save_action_at(&path, Some(&was), &written(&hi)).unwrap_err();
            assert_eq!(refused.errors.len(), 1, "{refused}");
            assert_eq!(refused.errors[0].field, None);
            assert!(
                refused.errors[0]
                    .message
                    .contains("The file changed after the list was read."),
                "{refused}"
            );
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), saved());

        let broken = saved().replace("new_app_on_top = true", "new_app_on_top = \"yes\"");
        std::fs::write(&path, &broken).unwrap();
        let refused = Config::save_action_at(&path, None, &written(&hi)).unwrap_err();
        assert_eq!(refused.errors[0].field, None);
        assert!(refused.errors[0].message.contains("line 10"), "{refused}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
    }

    /// A deleted action leaves the file, found by where the list showed it,
    /// and the rest stays in order. One that is no longer there is an error,
    /// and nothing is written.
    #[test]
    fn a_deleted_action_leaves_the_file_and_nothing_else_does() {
        let (_folder, path) = example_file();
        let actions = Config::delete_action_at(&path, &listed(2, "Open Music", "BB03")).unwrap();
        let mut expected = example().actions;
        expected.remove(2);
        assert_eq!(actions, expected);
        let without = saved().replace(
            "\n[[actions]]\nname = \"Open Music\"\nsignal = \"bb03\"\nshell = \"open -a Music\"\n",
            "",
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), without);

        let err = Config::delete_action_at(&path, &listed(2, "Open Music", "bb03"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("so nothing was deleted"), "{err}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), without);
    }

    /// Two writes of the file both land, one after the other and at once.
    /// Each writer reads the file, changes it and writes it whole, under the
    /// process's config lock. Without the lock, the second writer here reads
    /// the file while the first holds it, and the first then writes back
    /// what it had read, so the second change is lost.
    #[test]
    fn two_writes_both_land() {
        let (_folder, path) = example_file();
        Config::change_settings_at(&path, &change(r#"{"new_app_on_top": false}"#)).unwrap();
        let hi = action("Say hi", "bb07", Does::Shell("say hi".to_string()));
        Config::save_action_at(&path, None, &written(&hi)).unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert!(
            !loaded.settings.new_app_on_top,
            "the first of two in a row was lost"
        );
        assert_eq!(
            loaded.actions.len(),
            6,
            "the second of two in a row was lost"
        );

        let (has_read, read) = std::sync::mpsc::channel();
        std::thread::scope(|threads| {
            let first = threads.spawn(|| {
                Config::update_at(&path, |config| {
                    has_read.send(()).unwrap();
                    // Long enough for the second writer to reach the file.
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    config.actions.retain(|action| action.signal != "bb07");
                    Ok::<_, BandError>(())
                })
            });
            read.recv().unwrap();
            Config::change_settings_at(&path, &change(r#"{"auto_reconnect": false}"#)).unwrap();
            first.join().unwrap().unwrap();
        });
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(
            loaded.actions,
            example().actions,
            "the first of two at once was lost"
        );
        assert!(
            !loaded.settings.auto_reconnect,
            "the second of two at once was lost"
        );
        assert!(!loaded.settings.new_app_on_top);
    }

    /// A read under the lock waits for a change in progress, and sees it.
    #[test]
    fn a_read_under_the_lock_waits_for_a_change() {
        let (_folder, path) = example_file();
        assert!(Config::load_from(&path).unwrap().settings.auto_reconnect);
        let (has_read, read) = std::sync::mpsc::channel();
        std::thread::scope(|threads| {
            let writer = threads.spawn(|| {
                Config::update_at(&path, |config| {
                    has_read.send(()).unwrap();
                    // Long enough for the reader to reach the file.
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    config.settings.auto_reconnect = false;
                    Ok::<_, BandError>(())
                })
            });
            read.recv().unwrap();
            let seen = Config::load_under_lock(&path).unwrap();
            assert!(
                !seen.settings.auto_reconnect,
                "the read did not wait for the change"
            );
            writer.join().unwrap().unwrap();
        });
    }

    // --- the loader rules, each broken once --------------------------------

    /// The message the loader gives for this file.
    fn refused(text: &str) -> String {
        match Config::parse(text) {
            Ok(config) => panic!("this should have been refused, and loaded as {config:?}"),
            Err(err) => err.to_string(),
        }
    }

    /// A file with one action, named Open Music, holding these lines.
    fn open_music(lines: &str) -> String {
        format!("[[actions]]\nname = \"Open Music\"\n{lines}\n")
    }

    #[test]
    fn an_action_needs_a_name() {
        // With no name, the message names the action by its place.
        let text = "[[actions]]\nname = \"First\"\nsignal = \"bb01\"\nshell = \"true\"\n\n\
                    [[actions]]\nsignal = \"bb02\"\nshell = \"true\"\n";
        let err = refused(text);
        assert!(err.contains("action 2 needs a name"), "{err}");

        let err = refused("[[actions]]\nname = \"  \"\nsignal = \"bb01\"\nshell = \"true\"\n");
        assert!(err.contains("action 1 needs a name"), "{err}");
    }

    #[test]
    fn an_action_needs_a_signal() {
        let err = refused(&open_music("shell = \"open -a Music\""));
        assert!(err.contains("action \"Open Music\" needs a signal"), "{err}");
    }

    /// A signal is bb and a byte after it, the rule the band app editor
    /// enforces. A `music:` signal is not one any more.
    #[test]
    fn a_signal_is_bb_and_a_byte() {
        for (signal, says) in [
            ("0201", "is not a signal. A signal is bb followed by"),
            ("bb", "is not a signal. A signal is bb followed by"),
            ("bb0g", "is not a signal"),
            ("music:volume_up", "is not a signal"),
        ] {
            let err = refused(&open_music(&format!("signal = \"{signal}\"\nshell = \"open -a Music\"")));
            assert!(err.contains("action \"Open Music\""), "{signal}: {err}");
            assert!(err.contains(says), "{signal}: {err}");
        }
        for signal in ["bb04", "BB1F", "bb0401"] {
            let text = open_music(&format!("signal = \"{signal}\"\nshell = \"open -a Music\""));
            assert!(Config::parse(&text).is_ok(), "{signal} is a signal");
        }
    }

    #[test]
    fn an_action_does_exactly_one_thing() {
        let err = refused(&open_music("signal = \"bb03\""));
        assert!(err.contains("action \"Open Music\" does nothing"), "{err}");

        let err = refused(&open_music(
            "signal = \"bb03\"\nshell = \"open -a Music\"\nscript = \"~/music.sh\"",
        ));
        assert!(err.contains("action \"Open Music\" sets script and shell"), "{err}");
    }

    /// A field left over from the old flat format would be dropped without a
    /// word, and the keystroke would go out without its modifiers.
    #[test]
    fn a_field_that_is_not_part_of_an_action_is_named() {
        let err = refused(&open_music(
            "signal = \"bb03\"\nkeystroke = { key_code = \"4\" }\nmodifiers = [\"command\"]",
        ));
        assert!(err.contains("action \"Open Music\": modifiers is not part of an action"), "{err}");

        let err = refused(&open_music(
            "signal = \"bb03\"\nkeystroke = { key_code = \"4\", modifers = [\"command\"] }",
        ));
        assert!(err.contains("action \"Open Music\""), "{err}");
        assert!(err.contains("modifers"), "{err}");
    }

    /// Two actions may share a signal (D6, changed 2026-09-26), whatever
    /// case each spells it in, and they keep the order of the file.
    #[test]
    fn two_actions_may_share_a_signal_and_keep_their_order() {
        let text = "[[actions]]\nname = \"Lamp on\"\nsignal = \"bb01\"\nshell = \"true\"\n\n\
                    [[actions]]\nname = \"Lamp off\"\nsignal = \"BB01\"\nshell = \"false\"\n";
        let config = Config::parse(text).unwrap();
        let names: Vec<&str> = config.actions.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Lamp on", "Lamp off"]);
    }

    #[test]
    fn a_keystroke_names_exactly_one_key() {
        let err = refused(&open_music(
            "signal = \"bb03\"\nkeystroke = { key_code = \"a\", consumer_key_code = \"mute\" }",
        ));
        assert!(err.contains("action \"Open Music\""), "{err}");
        assert!(err.contains("not both"), "{err}");

        let err = refused(&open_music("signal = \"bb03\"\nkeystroke = { modifiers = [\"command\"] }"));
        assert!(err.contains("action \"Open Music\""), "{err}");
        assert!(err.contains("needs key_code or consumer_key_code"), "{err}");
    }

    #[test]
    fn a_keystroke_names_keys_this_program_knows() {
        for (keystroke, says) in [
            ("{ key_code = \"meta\" }", "unknown key_code meta"),
            ("{ key_code = \"vk_none\" }", "key_code vk_none cannot be sent."),
            ("{ key_code = \"a\", modifiers = [\"hyper\"] }", "unknown modifier hyper"),
            ("{ consumer_key_code = \"brightness_up\" }", "unknown consumer_key_code brightness_up"),
            ("{ consumer_key_code = \"mute\", modifiers = [\"command\"] }", "a media key takes no modifiers"),
        ] {
            let err = refused(&open_music(&format!("signal = \"bb03\"\nkeystroke = {keystroke}")));
            assert!(err.contains("action \"Open Music\""), "{keystroke}: {err}");
            assert!(err.contains(says), "{keystroke}: {err}");
        }
    }

    /// Windows cannot send some keys the Mac can. The loader refuses each by
    /// its name, and says why (W2).
    #[cfg(windows)]
    #[test]
    fn windows_refuses_a_key_it_cannot_send_by_its_name() {
        for (keystroke, says) in [
            (
                "{ key_code = \"fn\" }",
                "key_code fn cannot be sent. Windows has no key event for fn.",
            ),
            (
                "{ key_code = \"k\", modifiers = [\"shift\", \"left_fn\"] }",
                "the modifier left_fn cannot be sent. Windows has no key event for fn.",
            ),
            (
                "{ consumer_key_code = \"display_brightness_increment\" }",
                "consumer_key_code display_brightness_increment cannot be sent. Windows has no \
                 key for display brightness.",
            ),
            (
                "{ key_code = \"undo\" }",
                "key_code undo cannot be sent. Choose a key from the list in the window.",
            ),
        ] {
            let err = refused(&open_music(&format!("signal = \"bb03\"\nkeystroke = {keystroke}")));
            assert!(err.contains(&format!("action \"Open Music\": {says}")), "{keystroke}: {err}");
        }
        // Command is the Win key and option is Alt, so both load, and so do
        // the keys a PC keyboard has beyond a Mac's.
        for keystroke in [
            "{ key_code = \"k\", modifiers = [\"command\", \"option\"] }",
            "{ key_code = \"print_screen\" }",
            "{ key_code = \"insert\", modifiers = [\"shift\"] }",
            "{ key_code = \"f24\" }",
        ] {
            let text = open_music(&format!("signal = \"bb03\"\nkeystroke = {keystroke}"));
            assert!(Config::parse(&text).is_ok(), "{keystroke}");
        }
    }

    /// The Mac refuses the keys macOS has no keycode for, in its own words,
    /// the PC's own keys among them.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_mac_refuses_a_key_macos_cannot_send_in_its_words() {
        for (keystroke, says) in [
            ("{ key_code = \"f21\" }", "key_code f21 cannot be sent. macOS stops at f20."),
            (
                "{ key_code = \"insert\" }",
                "key_code insert cannot be sent. macOS has no insert key. The key in that place is \
                 help.",
            ),
        ] {
            let err = refused(&open_music(&format!("signal = \"bb03\"\nkeystroke = {keystroke}")));
            assert!(err.contains(&format!("action \"Open Music\": {says}")), "{keystroke}: {err}");
        }
    }

    /// Windows has no Shortcuts. The loader refuses a shortcut action, and
    /// says what to use instead, in the file and in the window's editor
    /// alike (W5).
    #[cfg(windows)]
    #[test]
    fn windows_refuses_a_shortcut_and_says_what_to_use() {
        let why = "Shortcuts are a Mac feature. Use a Script or Shell action on Windows.";
        let err = refused(PLAN_EXAMPLE);
        assert_eq!(err, format!("config error: action \"Start a focus block\": {why}"));

        let written = serde_json::json!({"name": "Focus", "signal": "bb02", "shortcut": "Start Focus"});
        let refused = Action::from_editor(&written).unwrap_err();
        assert_eq!(
            refused.errors,
            [Problem {
                field: Some("shortcut"),
                message: why.to_string(),
            }]
        );
        let tried = Does::from_editor(&written).unwrap_err();
        assert_eq!(tried.errors[0].message, why);
    }

    #[test]
    fn a_command_a_script_and_a_shortcut_are_not_empty() {
        for kind in ["shell", "script", "shortcut"] {
            let err = refused(&open_music(&format!("signal = \"bb03\"\n{kind} = \" \"")));
            assert!(err.contains(&format!("action \"Open Music\": {kind} is empty")), "{err}");
        }
    }

    #[test]
    fn a_web_request_is_well_formed() {
        for (request, says) in [
            ("{ url = \"file:///etc/passwd\" }", "must start with http:// or https://"),
            ("{ url = \"https://example.com\", method = \"FETCH\" }", "unknown method FETCH"),
        ] {
            let err = refused(&open_music(&format!("signal = \"bb03\"\nweb_request = {request}")));
            assert!(err.contains("action \"Open Music\""), "{request}: {err}");
            assert!(err.contains(says), "{request}: {err}");
        }
    }

    /// A header is where a token lives, so a broken one is named by its place
    /// and never quoted.
    #[test]
    fn a_broken_header_is_named_by_its_place_and_not_quoted() {
        let err = refused(&open_music(
            "signal = \"bb03\"\nweb_request = { url = \"https://example.com\", headers = [\"Accept: */*\", \"Authorization Bearer s3cret\"] }",
        ));
        assert!(err.contains("action \"Open Music\": header 2 must read Name: value"), "{err}");
        assert!(!err.contains("s3cret"), "{err}");
    }

    #[test]
    fn a_method_is_read_whatever_its_case() {
        let request = |method: Option<&str>, body: Option<&str>| WebRequest {
            method: method.map(str::to_string),
            url: "https://example.com/hook".to_string(),
            headers: Vec::new(),
            body: body.map(str::to_string),
        };
        assert_eq!(request(Some("post"), None).method(), "POST");
        assert_eq!(request(None, Some("hello")).method(), "POST", "a body means POST");
        assert_eq!(request(None, None).method(), "GET");
    }

    /// A file in the old format must not load as an empty one, which would
    /// fire nothing and say nothing. Every old name is refused by name.
    #[test]
    fn a_file_in_the_old_format_is_refused_rather_than_read_as_empty() {
        for (old, name) in [
            ("peripheral_id = \"01234567\"\n", "peripheral_id"),
            ("auto_reconnect = false\n", "auto_reconnect"),
            ("[[bindings]]\nsignal = \"bb01\"\nkey_code = \"page_down\"\n", "bindings"),
            ("[settings]\nkeep_settings_last = false\n", "keep_settings_last"),
        ] {
            let err = refused(old);
            assert!(err.contains(name), "{err}");
        }
    }

    /// A key for tests. It opens no band.
    const FAKE_KEY: &str = "000102030405060708090a0b0c0d0e0f";

    /// Whether a message holds sixteen hex digits in a row, which is at least
    /// half a key in some spelling.
    fn quotes_a_key(message: &str) -> bool {
        let mut run = 0;
        for c in message.chars() {
            run = if c.is_ascii_hexdigit() { run + 1 } else { 0 };
            if run >= 16 {
                return true;
            }
        }
        false
    }

    /// The auth key does not belong in this file, but a person may paste it
    /// in. Written without its quotes it reads as a number, which serde quotes
    /// in full. No spelling of it may come back in a message.
    #[test]
    fn a_key_pasted_into_the_file_is_never_quoted() {
        let device = |line: &str| format!("[device]\nname = \"Band\"\nmac = \"00:00:5E:00:53:01\"\n{line}\n");
        for text in [
            format!("auth_key = 0x{FAKE_KEY}\n"),
            format!("auth_key = \"{FAKE_KEY}\"\n"),
            device(&format!("auth_key = 0x{FAKE_KEY}")),
            device("auth_key = 0x0001_0203_0405_0607_0809_0a0b_0c0d_0e0f"),
            device(&format!("auth_key = {FAKE_KEY}")),
            device(&format!("uuid = [\"{FAKE_KEY}\"]")),
            format!("{FAKE_KEY} = true\n"),
            format!("[settings]\nauto_reconnect = \"{FAKE_KEY}\"\n"),
            open_music(&format!("signal = \"bb03\"\nkeystroke = {{ key_code = 0x{FAKE_KEY} }}")),
            // The loader's own messages quote a key name, a signal, and an
            // action's name.
            open_music(&format!("signal = \"bb03\"\nkeystroke = {{ key_code = \"{FAKE_KEY}\" }}")),
            open_music(&format!("signal = \"{FAKE_KEY}\"\nshell = \"true\"")),
            format!("[[actions]]\nname = \"{FAKE_KEY}\"\nsignal = \"music\"\nshell = \"true\"\n"),
        ] {
            let err = refused(&text);
            assert!(!quotes_a_key(&err), "the message quoted the key: {err}");
        }
    }
}
