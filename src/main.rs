mod cli;

use bandbridge::{
    ble, binding, config, control, keys, screens, session, supervisor,
};

/// Show what a library call produced.
fn print_lines(lines: Vec<String>) {
    for line in lines {
        println!("{line}");
    }
}

use crate::cli::{Cli, Command};
use bandbridge::error::BandError;
use bandbridge::supervisor::{Command as SupCommand, ConnectOptions, Status, Supervisor};
use btleplug::api::Peripheral as _;
use clap::Parser;
use std::time::Duration;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<(), BandError> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "bandbridge=info".into()),
        )
        .init();

    match Cli::parse().command {
        Command::Scan { seconds } => run_scan(seconds).await,
        Command::Inspect { peripheral_id, seconds } => run_inspect(&peripheral_id, seconds).await,
        Command::Listen {
            peripheral_id,
            raw,
            mtu,
            subscribe_all,
            no_auto_reconnect,
            seconds,
        } => run_listen(peripheral_id, raw, mtu, subscribe_all, seconds, no_auto_reconnect).await,
        Command::Install {
            file,
            peripheral_id,
            seconds,
        } => run_install(&file, peripheral_id, seconds).await,
        Command::Device => run_device().await,
        Command::Follow => run_follow().await,
        Command::Apps {
            remove,
            all,
            peripheral_id,
            seconds,
        } => run_apps(remove, all, peripheral_id, seconds).await,
        Command::Screens {
            list,
            set,
            hide,
            tidy,
            peripheral_id,
            seconds,
        } => run_screens(&list, set, hide, tidy, peripheral_id, seconds).await,
    }
}

/// Print what the process holding the band knows about it.
async fn run_device() -> Result<(), BandError> {
    if !bandbridge::ipc::forward(&bandbridge::ipc::Request::Device).await? {
        return Err(BandError::Device(
            "nothing is holding the band. Start the window, or run listen.".to_string(),
        ));
    }
    Ok(())
}

/// Watch band events from the process that holds the band.
async fn run_follow() -> Result<(), BandError> {
    if !bandbridge::ipc::forward(&bandbridge::ipc::Request::Follow).await? {
        return Err(BandError::Config(
            "nothing is holding the band. Start the window, or run listen."
                .to_string(),
        ));
    }
    Ok(())
}

/// List the apps on the band, or remove one.
async fn run_apps(
    remove: Option<String>,
    all: bool,
    peripheral_id: Option<String>,
    seconds: u64,
) -> Result<(), BandError> {
    // Another process may hold the band. Ask it rather than fight for the link.
    if bandbridge::ipc::forward(&bandbridge::ipc::Request::Apps {
        remove: remove.clone(),
        all,
    })
    .await?
    {
        return Ok(());
    }

    let config = config::Config::load()?;
    let auth_key = config::AuthKey::load(config.mac())?;
    let peripheral_id = peripheral_id.or_else(|| config.uuid().map(str::to_string));

    println!("Opening Bluetooth adapter...");
    let adapter = ble::adapter().await?;
    let peripheral =
        ble::find_band(&adapter, Duration::from_secs(seconds), peripheral_id.as_deref()).await?;
    let chunked = ble::connect_and_discover(&peripheral).await?;
    let mut session =
        session::Session::open(peripheral.clone(), chunked, false, None, false).await?;
    println!("Authenticating...");
    supervisor::authenticate(&mut session, auth_key).await?;
    println!("Authenticated\n");

    let result = match &remove {
        Some(id) => bandbridge::apps::remove(&mut session, id)
            .await
            .map(|removed| vec![bandbridge::apps::removed_line(&removed)]),
        None => bandbridge::apps::list_lines(&mut session, all).await,
    }
    .map(print_lines);

    if let Err(err) = peripheral.disconnect().await {
        tracing::warn!("could not release the band: {err}");
    }
    result
}

async fn run_screens(
    list: &str,
    set: Option<Vec<String>>,
    hide: Vec<String>,
    tidy: bool,
    peripheral_id: Option<String>,
    seconds: u64,
) -> Result<(), BandError> {
    let kind = screens::kind_from_name(list)?;

    // Another process may hold the band. Ask it rather than fight for the link.
    if bandbridge::ipc::forward(&bandbridge::ipc::Request::Screens {
        list: list.to_string(),
        set: set.clone(),
        hide: hide.clone(),
        tidy,
    })
    .await?
    {
        return Ok(());
    }

    let config = config::Config::load()?;
    let auth_key = config::AuthKey::load(config.mac())?;
    let peripheral_id = peripheral_id.or_else(|| config.uuid().map(str::to_string));

    println!("Opening Bluetooth adapter...");
    let adapter = ble::adapter().await?;
    let peripheral =
        ble::find_band(&adapter, Duration::from_secs(seconds), peripheral_id.as_deref()).await?;
    println!(
        "Found {}",
        ble::peripheral_name(&peripheral)
            .await
            .unwrap_or_else(|| "-".into())
    );

    let chunked = ble::connect_and_discover(&peripheral).await?;
    let mut session =
        session::Session::open(peripheral.clone(), chunked, false, None, false).await?;
    println!("Authenticating...");
    supervisor::authenticate(&mut session, auth_key).await?;
    println!("Authenticated\n");

    let result = screens::serve(&mut session, kind, set.as_deref(), &hide, tidy)
        .await
        .map(print_lines);

    if let Err(err) = peripheral.disconnect().await {
        tracing::warn!("could not release the band: {err}");
    }
    result
}



async fn run_scan(seconds: u64) -> Result<(), BandError> {
    println!("Opening Bluetooth adapter...");
    let adapter = ble::adapter().await?;
    println!("Scanning for {seconds}s...");
    for p in ble::scan(&adapter, Duration::from_secs(seconds)).await? {
        let name = ble::peripheral_name(&p).await.unwrap_or_else(|| "-".into());
        println!("{}  {}", p.id(), name);
    }
    Ok(())
}

async fn run_inspect(peripheral_id: &str, seconds: u64) -> Result<(), BandError> {
    println!("Opening Bluetooth adapter...");
    let adapter = ble::adapter().await?;
    let p = ble::find_band(&adapter, Duration::from_secs(seconds), Some(peripheral_id)).await?;
    let name = ble::peripheral_name(&p).await.unwrap_or_else(|| "-".into());
    println!("Peripheral: {}", p.id());
    println!("Name: {name}");

    ble::connect_and_discover(&p).await?;
    println!("MTU: {}", p.mtu());

    println!("Services:");
    for service in p.services() {
        println!("  {}", service.uuid);
        for c in &service.characteristics {
            let flags = if c.uuid == ble::CHAR_WRITE {
                " <- chunked write"
            } else if c.uuid == ble::CHAR_READ {
                " <- chunked read"
            } else {
                ""
            };
            println!("    {}  {:?}{}", c.uuid, c.properties, flags);
        }
    }

    p.disconnect().await?;
    Ok(())
}

async fn run_listen(
    peripheral_id: Option<String>,
    raw: bool,
    mtu: Option<u16>,
    subscribe_all: bool,
    seconds: u64,
    no_auto_reconnect: bool,
) -> Result<(), BandError> {
    // The loader has checked every action by now, and names any it refused.
    let config = config::Config::load()?;
    let auth_key = config::AuthKey::load(config.mac())?;

    let actions = config.actions.clone();
    if !actions.is_empty() {
        // Only a keystroke needs the permission. A config of shell commands,
        // scripts, shortcuts, and web requests runs on a Mac that has granted
        // nothing.
        if binding::any_posts_keys(&actions) && !keys::trusted(false) {
            return Err(BandError::Config(
                "an action posts a keystroke but this process cannot post key \
                 events. macOS should have shown a permission dialog. Choose \
                 Open System Settings there, switch bandbridge on, then run \
                 again. The pane is Privacy & Security, Device Control and Data \
                 Access, named Accessibility before macOS 27."
                    .to_string(),
            ));
        }
        println!("{} action(s) active", actions.len());
    }

    let peripheral_id = peripheral_id.or_else(|| config.uuid().map(str::to_string));
    let auto_reconnect = config.settings.auto_reconnect && !no_auto_reconnect;

    let opts = ConnectOptions {
        peripheral_id,
        mac: config.mac().map(str::to_string),
        raw,
        mtu,
        subscribe_all,
        scan_seconds: seconds,
    };

    let (cmd_tx, cmd_rx) = mpsc::channel::<SupCommand>(8);
    let (status_tx, mut status_rx) = mpsc::channel::<Status>(64);

    // Whoever holds the band answers for it. Another process reaching the
    // socket calls the same functions this one does.
    let (req_tx, req_rx) = mpsc::channel::<bandbridge::ipc::Incoming>(8);
    let supervisor = Supervisor::new(Some(auth_key), opts, cmd_rx, status_tx)
        .serving(req_rx)
        .following(&config)
        .never_reconnecting(no_auto_reconnect);
    tokio::spawn(async move {
        if let Err(err) = bandbridge::ipc::serve(req_tx).await {
            tracing::debug!("not serving other processes: {err}");
        }
    });

    control::spawn_stdin_reader(cmd_tx.clone());
    spawn_signal_handler(cmd_tx.clone());

    println!("{}", control::CONTROL_HELP);
    if !auto_reconnect {
        println!("auto-reconnect is off; a dropped link will stay down");
    }

    // The printer runs as its own task so it can drain status messages while
    // the supervisor runs. The supervisor itself is NOT spawned: it owns a
    // live Session, whose notification stream is Send but not Sync, so a
    // future holding a Session across an await is not Send and tokio::spawn
    // would refuse it. Awaiting it in place on this task needs no such bound.
    // The same words `follow` prints, from the same function.
    let printer = tokio::spawn(async move {
        while let Some(status) = status_rx.recv().await {
            if let Some(line) = status.terminal_line() {
                println!("{line}");
            }
        }
    });

    let result = supervisor.run().await;

    // Dropping the supervisor's status sender (above) closes the channel, so
    // the printer task drains any buffered messages and returns on its own.
    let _ = printer.await;

    result
}

async fn run_install(
    file: &str,
    peripheral_id: Option<String>,
    seconds: u64,
) -> Result<(), BandError> {
    // The holder has a working directory of its own, so send a path that means
    // the same thing from anywhere. This also fails early on a bad path.
    let full = std::fs::canonicalize(file)
        .map_err(|err| BandError::Install(format!("cannot read {file}: {err}")))?;

    // Another process may hold the band. Ask it rather than fight for the link.
    if bandbridge::ipc::forward(&bandbridge::ipc::Request::Install {
        file: full.display().to_string(),
    })
    .await?
    {
        return Ok(());
    }

    let config = config::Config::load()?;
    let auth_key = config::AuthKey::load(config.mac())?;
    let peripheral_id = peripheral_id.or_else(|| config.uuid().map(str::to_string));

    let lines =
        bandbridge::flow::install_app(auth_key, peripheral_id.as_deref(), seconds, file).await?;
    print_lines(lines);
    Ok(())
}



/// Release the band on Ctrl-C, and on the Mac on SIGTERM as well.
///
/// Only the Ctrl-C path used to disconnect. A process killed with SIGTERM left
/// the BLE link held, which starves the user's phone of the band.
fn spawn_signal_handler(tx: mpsc::Sender<SupCommand>) {
    tokio::spawn(async move {
        stop_asked().await;
        println!("\nreleasing the band...");
        let _ = tx.send(SupCommand::Shutdown).await;
    });
}

/// Wait for Ctrl-C or SIGTERM.
#[cfg(unix)]
async fn stop_asked() {
    match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
        Ok(mut term) => {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
        }
        Err(err) => {
            // Still honor Ctrl-C, or nothing releases the band.
            tracing::warn!("cannot listen for SIGTERM: {err}");
            let _ = tokio::signal::ctrl_c().await;
        }
    }
}

/// Wait for Ctrl-C. Windows has no SIGTERM, and a process that ends there
/// without a word lets go of the band all the same: Windows drops a
/// process's Bluetooth links when it ends.
#[cfg(windows)]
async fn stop_asked() {
    let _ = tokio::signal::ctrl_c().await;
}
