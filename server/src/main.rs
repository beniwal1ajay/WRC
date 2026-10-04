//! WARD Server — Wayland Android Remote Desktop
//!
//! Unattended Background Host Architecture.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use ward_server::auth::{AuthManager, HostConfig};
use ward_server::capture::pipewire_capture::{CaptureStats, CapturedFrame, FrameFormat, run_pipewire_capture};
use ward_server::identity::HostIdentity;
use ward_server::network::{DEFAULT_PORT, WardServer, handle_client_session_with_sink};
use ward_server::portal::InputSink;
use ward_server::portal::mutter::{MutterSession, open_pipewire_socket};
use ward_server::portal::remote_desktop::{RemoteDesktopPortal, buttons, device_types};
use ward_server::portal::screencast::{CursorMode, ScreenCastPortal, SourceType};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info"))
        )
        .with_target(true)
        .with_thread_ids(true)
        .init();

    let args: Vec<String> = std::env::args().collect();
    let first_arg = args.get(1).map(|s| s.as_str()).unwrap_or("host");

    match first_arg {
        "-h" | "--help" | "help" => {
            print_help();
            return Ok(());
        }
        "-v" | "--version" | "version" => {
            println!("WARD Server v0.2.0 (Unattended Host Architecture)");
            println!("Wayland Android Remote Desktop");
            return Ok(());
        }
        "setup" | "--setup" => {
            let mut pin_arg = None;
            let mut name_arg = None;
            let mut force = false;

            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--pin" => {
                        if i + 1 < args.len() {
                            pin_arg = Some(args[i + 1].clone());
                            i += 1;
                        }
                    }
                    "--name" => {
                        if i + 1 < args.len() {
                            name_arg = Some(args[i + 1].clone());
                            i += 1;
                        }
                    }
                    "--force" => {
                        force = true;
                    }
                    val if !val.starts_with("--") && pin_arg.is_none() => {
                        pin_arg = Some(val.to_string());
                    }
                    _ => {}
                }
                i += 1;
            }

            run_host_setup(pin_arg, name_arg, force).await?;
            return Ok(());
        }
        "set-pin" => {
            let pin = match args.get(2) {
                Some(p) if !p.is_empty() => p.clone(),
                _ => {
                    print!("Enter new permanent 6-digit PIN: ");
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                    let mut input = String::new();
                    std::io::stdin().read_line(&mut input)?;
                    input.trim().to_string()
                }
            };

            if pin.len() < 6 {
                eprintln!("Error: PIN must be at least 6 digits/characters.");
                std::process::exit(1);
            }

            HostConfig::create_or_update(&pin)?;
            if let Ok(mut auth_mgr) = AuthManager::load_default() {
                let _ = auth_mgr.unpair_all();
            }
            println!("✅ Permanent PIN updated successfully (PBKDF2-HMAC-SHA256, 600,000 iterations).");
            println!("🔒 All previous device pairings revoked. Devices must authenticate with the new PIN.");

            // Restart background service so any active session is terminated and new PIN is reloaded immediately
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "restart", "ward-host.service"])
                .status();

            return Ok(());
        }
        "enable" => {
            let mut config = HostConfig::load_default()
                .context("Host configuration not found. Run 'ward-server setup' first.")?;
            config.host_enabled = true;
            config.save_to_path(&HostConfig::default_path()?)?;
            println!("✅ WARD Host remote access ENABLED.");
            return Ok(());
        }
        "disable" => {
            let mut config = HostConfig::load_default()
                .context("Host configuration not found. Run 'ward-server setup' first.")?;
            config.host_enabled = false;
            config.save_to_path(&HostConfig::default_path()?)?;
            println!("🔒 WARD Host remote access DISABLED (all incoming connections will be rejected).");
            return Ok(());
        }
        "--status" | "status" => {
            show_status().await?;
            return Ok(());
        }
        "--devices" | "devices" | "--list-devices" => {
            let auth_mgr = AuthManager::load_default()?;
            let clients = auth_mgr.list_authorized_clients();
            println!("Authorized WARD Devices (Total: {}):", clients.len());
            for (id, name, last_seen) in clients {
                println!("- Name: {}\n  Device ID: {}\n  Last active: {}s ago", name, id, ward_server::auth::current_timestamp().saturating_sub(last_seen));
            }
            return Ok(());
        }
        "--unpair" | "unpair" => {
            let dev_id = match args.get(2) {
                Some(id) if !id.is_empty() => id.as_str(),
                _ => {
                    eprintln!("Error: Missing DEVICE_ID argument.");
                    eprintln!("Usage: ward-server unpair <DEVICE_ID | --all>");
                    std::process::exit(1);
                }
            };
            let mut auth_mgr = AuthManager::load_default()?;
            if dev_id == "--all" {
                auth_mgr.unpair_all()?;
                println!("✅ Successfully unpaired all devices.");
            } else if auth_mgr.is_authorized(dev_id) {
                auth_mgr.remove_authorized_client(dev_id)?;
                println!("✅ Successfully unpaired device: {}", dev_id);
            } else {
                println!("Device ID '{}' was not found in authorized clients list.", dev_id);
            }
            return Ok(());
        }
        "tls-test" | "--tls-test" => {
            let port = args.get(2)
                .and_then(|p| p.parse::<u16>().ok())
                .unwrap_or(DEFAULT_PORT);

            info!("Starting WARD Standalone TLS Diagnostic Listener on port {}", port);
            let server = WardServer::bind(port).await?;
            let local_addr = server.local_addr()?;
            info!("✅ TLS Diagnostic Server listening on {}", local_addr);

            while let Ok((tls_stream, peer_addr)) = server.accept().await {
                info!("🎉 Successfully completed TLS 1.3 handshake with {}!", peer_addr);
                let (mut reader, _writer) = tokio::io::split(tls_stream);
                match tokio::time::timeout(Duration::from_secs(3), ward_server::protocol::read_message(&mut reader)).await {
                    Ok(Ok((header, msg))) => {
                        info!("📩 Initial message from {}: Header={:?}, Message={:?}", peer_addr, header, msg);
                    }
                    Ok(Err(e)) => {
                        info!("Client disconnected or read error: {}", e);
                    }
                    Err(_) => {
                        info!("Timeout waiting for protocol message (TLS handshake was 100% SUCCESSFUL!)");
                    }
                }
            }
            return Ok(());
        }
        "capture" | "--capture" => {
            info!("Mode: Milestone 1 — Screen Capture Proof");
            run_screen_capture_poc().await?;
            return Ok(());
        }
        "input" | "--input" => {
            info!("Mode: Milestone 2 — Remote Input Proof");
            run_remote_input_poc().await?;
            return Ok(());
        }
        _ => {
            // Determine port from args if given (e.g. `ward-server host 9740` or `ward-server stream 9740` or `ward-server 9740`)
            let port = if first_arg == "-p" || first_arg == "--port" {
                args.get(2).and_then(|p| p.parse::<u16>().ok()).unwrap_or(DEFAULT_PORT)
            } else if let Ok(p) = first_arg.parse::<u16>() {
                p
            } else if first_arg == "host" || first_arg == "stream" {
                args.get(2).and_then(|p| p.parse::<u16>().ok()).unwrap_or(DEFAULT_PORT)
            } else {
                DEFAULT_PORT
            };

            let force_portal = args.iter().any(|a| a == "--portal");
            run_host_server(port, force_portal).await?;
            return Ok(());
        }
    }
}

fn print_help() {
    println!("WARD Server — Wayland Android Remote Desktop");
    println!("Unattended Host Architecture (GNOME Wayland)");
    println!();
    println!("USAGE:");
    println!("    ward-server [COMMAND | OPTIONS]");
    println!("    ward-host   [COMMAND | OPTIONS]");
    println!();
    println!("COMMANDS:");
    println!("    setup [--pin <PIN>]   Run one-time idempotent host setup (verifies Wayland, sets permanent PIN, configures service)");
    println!("    host [PORT]           Start the unattended background streaming host (default: port 9740)");
    println!("    stream [PORT]         Alias for 'host'");
    println!("    status                Show host status, identity, PIN status, and paired devices");
    println!("    set-pin [PIN]         Set or change the permanent host PIN");
    println!("    enable                Enable remote desktop access");
    println!("    disable               Disable remote desktop access (refuse all incoming connections)");
    println!("    devices               List all paired and authorized Android devices");
    println!("    unpair <DEVICE_ID>    Revoke authorization for a client device (use '--all' for all)");
    println!("    tls-test [PORT]       Start standalone TLS diagnostic listener");
    println!("    capture               Run ScreenCast capture test");
    println!("    input                 Run RemoteDesktop input injection test");
    println!();
    println!("OPTIONS:");
    println!("    -p, --port <PORT>     Specify listening port (default: 9740)");
    println!("    --portal              Force XDG Desktop Portal backend instead of native Mutter backend");
    println!("    -v, --version         Show WARD version");
    println!("    -h, --help            Show this help message");
    println!();
}

/// Run one-time idempotent host setup flow.
async fn run_host_setup(pin_opt: Option<String>, name_opt: Option<String>, force: bool) -> Result<()> {
    println!("==================================================");
    println!("   WARD Host — One-Time Setup & Authorization     ");
    println!("==================================================");
    println!();

    // 1. Detect Ubuntu version
    let os_release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    let pretty_name = os_release
        .lines()
        .find(|l| l.starts_with("PRETTY_NAME="))
        .map(|l| l.trim_start_matches("PRETTY_NAME=").trim_matches('"'))
        .unwrap_or("Ubuntu Linux");
    println!("[1/8] Operating System:          {}", pretty_name);

    // 2. Detect Desktop Environment
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "unknown".into());
    let gnome_ver = std::process::Command::new("gnome-shell")
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "GNOME Shell".into());
    println!("[2/8] Desktop Environment:       {} ({})", gnome_ver, desktop);

    // 3. Detect Wayland
    let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".into());
    println!("[3/8] Display Server:            {} (Wayland session active)", session_type);
    if session_type != "wayland" {
        println!("⚠️ Warning: Current session type is '{}', expected 'wayland'.", session_type);
    }

    // 4. Detect PipeWire
    let pw_ver = std::process::Command::new("pipewire")
        .arg("--version")
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .unwrap_or("PipeWire")
                .to_string()
        })
        .unwrap_or_else(|_| "PipeWire".into());
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
        let uid = unsafe { libc::getuid() };
        format!("/run/user/{}", uid)
    });
    let pw_socket = PathBuf::from(&runtime_dir).join("pipewire-0");
    println!("[4/8] Multimedia Server:         {} (Socket: {})", pw_ver, pw_socket.display());
    if !pw_socket.exists() {
        bail!("PipeWire socket does not exist at {}. Ensure PipeWire user service is active.", pw_socket.display());
    }

    // 5. Detect Portals & GNOME Remote Desktop
    let grd_status = std::process::Command::new("grdctl")
        .arg("status")
        .output()
        .map(|_| "Installed & Available")
        .unwrap_or("Not installed (using direct Mutter)");
    println!("[5/8] GNOME Remote Desktop:      {}", grd_status);

    // 6. Verify Mutter Session D-Bus APIs with zero dialog test probe
    print!("[6/8] Wayland Native Backend:     Testing Mutter ScreenCast & RemoteDesktop... ");
    use std::io::Write;
    let _ = std::io::stdout().flush();

    let _connection = zbus::Connection::session().await
        .context("Failed to connect to user session D-Bus. Is the graphical user session active?")?;

    match MutterSession::new().await {
        Ok(test_session) => {
            println!("PASS ✅ (node={})", test_session.screencast.node_id);
            let _ = test_session.stop().await;
        }
        Err(e) => {
            println!("FAIL ❌");
            bail!("Mutter native session probe failed: {:#}. Ensure you are logged into a GNOME Wayland session.", e);
        }
    }

    // 7. Create/Verify Cryptographic Host Identity
    let identity = HostIdentity::load_or_create(name_opt)?;
    println!("[7/8] Cryptographic Identity:    '{}' (ID: {}...)", identity.device_name, &identity.device_id[..16]);

    // 8. Permanent PIN Configuration
    let existing_config = HostConfig::load_default().ok();
    let final_pin = if let Some(pin) = pin_opt {
        pin
    } else if let Some(ref cfg) = existing_config {
        if cfg.has_pin() && !force {
            println!("[8/8] Permanent Host PIN:        Configured ✅ (PBKDF2-HMAC-SHA256, 600,000 iterations)");
            "".to_string()
        } else {
            prompt_for_pin()?
        }
    } else {
        prompt_for_pin()?
    };

    if !final_pin.is_empty() {
        HostConfig::create_or_update(&final_pin)?;
        if let Ok(mut auth_mgr) = AuthManager::load_default() {
            let _ = auth_mgr.unpair_all();
        }
        println!("[8/8] Permanent Host PIN:        Saved securely to ~/.config/ward/host_config.json ✅ (All device pairings revoked)");
    }

    // 9. Install and configure systemd user background service
    install_systemd_service().await?;

    println!();
    println!("==================================================");
    println!("🎉 WARD Host Setup COMPLETE & ACTIVE!");
    println!("==================================================");
    println!("Host Name:        {}", identity.device_name);
    println!("Host Device ID:   {}", identity.device_id);
    println!("Host Service:     ward-host.service (active)");
    println!("Listening Port:   {} (TLS 1.3)", DEFAULT_PORT);
    println!("Unattended Mode:  ENABLED (Mutter native compositor backend)");
    println!("==================================================");
    println!();
    println!("To connect from your Android device:");
    println!("  1. Open WARD on Android");
    println!("  2. Select '{}'", identity.device_name);
    println!("  3. Enter your permanent PIN");
    println!("  4. Remote desktop streams instantly — no interaction needed on this PC!");
    println!();

    Ok(())
}

fn prompt_for_pin() -> Result<String> {
    print!("Enter permanent 6-digit PIN for remote access: ");
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    if trimmed.len() < 6 {
        bail!("PIN must be at least 6 digits/characters.");
    }
    Ok(trimmed.to_string())
}

/// Install systemd user service and symlink.
async fn install_systemd_service() -> Result<()> {
    let home = std::env::var("HOME").context("HOME environment variable not set")?;
    let bin_dir = PathBuf::from(&home).join(".local/bin");
    let systemd_user_dir = PathBuf::from(&home).join(".config/systemd/user");
    std::fs::create_dir_all(&bin_dir)?;
    std::fs::create_dir_all(&systemd_user_dir)?;

    // Ensure ~/.local/bin/ward-server exists
    let current_exe = std::env::current_exe()?;
    let target_exe = bin_dir.join("ward-server");
    if current_exe != target_exe {
        std::fs::copy(&current_exe, &target_exe)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&target_exe, std::fs::Permissions::from_mode(0o755));
        }
    }

    // Symlink ward-host -> ward-server
    let host_symlink = bin_dir.join("ward-host");
    let _ = std::fs::remove_file(&host_symlink);
    #[cfg(unix)]
    let _ = std::os::unix::fs::symlink("ward-server", &host_symlink);

    // Unit definition
    let service_content = format!(r#"[Unit]
Description=WARD Host - Unattended Wayland Remote Desktop Server
Documentation=https://github.com/beniwal1ajay/WRC
After=graphical-session.target pipewire.service
PartOf=graphical-session.target

[Service]
Type=simple
ExecStart={}/ward-server host
Restart=on-failure
RestartSec=3s
Environment=RUST_BACKTRACE=1
PassEnvironment=WAYLAND_DISPLAY DISPLAY XDG_CURRENT_DESKTOP XDG_SESSION_TYPE XDG_RUNTIME_DIR DBUS_SESSION_BUS_ADDRESS

[Install]
WantedBy=graphical-session.target
"#, bin_dir.display());

    let service_file = systemd_user_dir.join("ward-host.service");
    std::fs::write(&service_file, service_content)?;

    // Also write ward.service alias
    let ward_alias = systemd_user_dir.join("ward.service");
    let _ = std::fs::remove_file(&ward_alias);
    #[cfg(unix)]
    let _ = std::os::unix::fs::symlink("ward-host.service", &ward_alias);

    // Reload systemd and enable service
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();

    let enable_status = std::process::Command::new("systemctl")
        .args(["--user", "enable", "ward-host.service"])
        .status();

    if let Ok(st) = enable_status {
        if !st.success() {
            warn!("Failed to enable ward-host.service via systemctl");
        }
    }

    let start_status = std::process::Command::new("systemctl")
        .args(["--user", "restart", "ward-host.service"])
        .status();

    if let Ok(st) = start_status {
        if st.success() {
            info!("Systemd user service 'ward-host.service' started successfully.");
        }
    }

    Ok(())
}

/// Unattended background streaming host server.
async fn run_host_server(port: u16, force_portal: bool) -> Result<()> {
    info!("==================================================");
    info!("   WARD Host — Unattended Remote Desktop Server  ");
    info!("==================================================");

    let identity = HostIdentity::load_or_create(None)?;
    let auth_mgr = AuthManager::load_default()?;

    if let Some(config) = auth_mgr.host_config() {
        if !config.has_pin() {
            warn!("⚠️ No permanent PIN configured. Run 'ward-server setup' to set PIN.");
        }
        if !config.host_enabled {
            warn!("⚠️ WARD Host is currently DISABLED. Incoming connections will be rejected.");
        }
    } else {
        warn!("⚠️ No host configuration found. Run 'ward-server setup' first.");
    }

    info!("Host identity: '{}' (ID: {}...)", identity.device_name, &identity.device_id[..16]);
    info!("Backend: {}", if force_portal { "XDG Desktop Portal (Modal Dialog)" } else { "Native Mutter Compositor (Unattended, Zero Prompts)" });

    let server = WardServer::bind(port).await?;
    let local_addr = server.local_addr()?;
    info!("🚀 WARD Host listening on {}", local_addr);
    info!("Waiting for incoming connections from authorized devices...");

    let running = Arc::new(AtomicBool::new(true));
    let ctrlc_running = running.clone();

    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            info!("Received shutdown signal. Stopping WARD Host...");
            ctrlc_running.store(false, Ordering::Relaxed);
        }
    });

    while running.load(Ordering::Relaxed) {
        match server.accept().await {
            Ok((tls_stream, peer_addr)) => {
                info!("New connection from: {}", peer_addr);
                let session_running = Arc::new(AtomicBool::new(true));

                if !force_portal {
                    // Native Mutter Unattended Backend
                    match MutterSession::new().await {
                        Ok(mutter) => {
                            let pw_fd = match open_pipewire_socket() {
                                Ok(fd) => fd,
                                Err(e) => {
                                    error!("Failed to connect to PipeWire socket: {:#}", e);
                                    continue;
                                }
                            };

                            let (pw_tx, pw_rx) = std::sync::mpsc::sync_channel::<CapturedFrame>(2);
                            let pw_running = session_running.clone();
                            let pw_stats = Arc::new(CaptureStats::default());

                            let capture_handle = match run_pipewire_capture(
                                pw_fd,
                                mutter.screencast.node_id,
                                pw_tx,
                                pw_running,
                                pw_stats,
                            ) {
                                Ok(h) => h,
                                Err(e) => {
                                    error!("Failed to start PipeWire capture: {:#}", e);
                                    let _ = mutter.stop().await;
                                    continue;
                                }
                            };

                            let (client_tx, client_rx) = tokio::sync::mpsc::channel::<CapturedFrame>(1);

                            // Bridge thread between sync PipeWire channel and async client channel
                            let bridge_running = session_running.clone();
                            std::thread::spawn(move || {
                                while bridge_running.load(Ordering::Relaxed) {
                                    match pw_rx.recv_timeout(Duration::from_millis(50)) {
                                        Ok(frame) => {
                                            let _ = client_tx.try_send(frame);
                                        }
                                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                                    }
                                }
                            });

                            let input_sink = InputSink::Mutter(mutter.remote_desktop.clone());

                            if let Err(e) = handle_client_session_with_sink(
                                tls_stream,
                                peer_addr,
                                client_rx,
                                Some(input_sink),
                                session_running.clone(),
                            ).await {
                                warn!("[DIAG] Session closed for {} with error: {:#}", peer_addr, e);
                            } else {
                                info!("[DIAG] Session closed for {} cleanly", peer_addr);
                            }

                            session_running.store(false, Ordering::Relaxed);
                            let _ = mutter.stop().await;
                            let _ = capture_handle.join();
                            info!("Mutter session released. Awaiting next connection...");
                        }
                        Err(e) => {
                            error!("Failed to initialize Mutter native session: {:#}", e);
                        }
                    }
                } else {
                    // Fallback Portal Backend
                    warn!("Using fallback XDG Desktop Portal backend (modal dialog may appear)...");
                    match run_portal_client_session(tls_stream, peer_addr, session_running).await {
                        Ok(()) => info!("Portal session completed"),
                        Err(e) => warn!("Portal session failed: {:#}", e),
                    }
                }
            }
            Err(e) => {
                if running.load(Ordering::Relaxed) {
                    let err_str = format!("{:#}", e);
                    if !err_str.contains("TCP reachability probe") {
                        warn!("Error accepting connection: {:#}", e);
                    }
                }
            }
        }
    }

    info!("WARD Host stopped cleanly.");
    Ok(())
}

async fn run_portal_client_session(
    tls_stream: tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
    peer_addr: std::net::SocketAddr,
    session_running: Arc<AtomicBool>,
) -> Result<()> {
    let mut portal = RemoteDesktopPortal::new().await?;
    portal.create_session().await?;
    portal.select_devices(device_types::KEYBOARD | device_types::POINTER).await?;
    portal.select_sources(SourceType::Monitor, CursorMode::Embedded).await?;

    let streams = portal.start("").await?;
    let stream = streams.first().context("No capture streams returned by portal")?;
    let pw_fd = portal.open_pipewire_remote().await?;

    let (pw_tx, pw_rx) = std::sync::mpsc::sync_channel::<CapturedFrame>(8);
    let pw_running = session_running.clone();
    let pw_stats = Arc::new(CaptureStats::default());

    let capture_handle = run_pipewire_capture(
        pw_fd,
        stream.pipewire_node_id,
        pw_tx,
        pw_running,
        pw_stats,
    )?;

    let (client_tx, client_rx) = tokio::sync::mpsc::channel::<CapturedFrame>(4);
    let bridge_running = session_running.clone();
    std::thread::spawn(move || {
        while bridge_running.load(Ordering::Relaxed) {
            match pw_rx.recv_timeout(Duration::from_millis(50)) {
                Ok(frame) => {
                    let _ = client_tx.try_send(frame);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });

    let portal_arc = Arc::new(tokio::sync::Mutex::new(portal));
    let input_sink = InputSink::Portal(portal_arc.clone());

    let res = handle_client_session_with_sink(
        tls_stream,
        peer_addr,
        client_rx,
        Some(input_sink),
        session_running.clone(),
    ).await;

    session_running.store(false, Ordering::Relaxed);
    let _ = capture_handle.join();
    let _ = portal_arc.lock().await.close().await;

    res
}

async fn show_status() -> Result<()> {
    let identity = HostIdentity::load_or_create(None)?;
    let auth_mgr = AuthManager::load_default()?;
    let clients = auth_mgr.list_authorized_clients();
    let config = auth_mgr.host_config();

    let svc_status = std::process::Command::new("systemctl")
        .args(["--user", "is-active", "ward-host.service"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".into());

    println!("==================================================");
    println!("   WARD Host — Status & Identity                  ");
    println!("==================================================");
    println!("Host Device Name:     {}", identity.device_name);
    println!("Host Device ID:       {}", identity.device_id);
    println!("Host Public Key:      {}", identity.public_key_hex);
    println!("Session Type:         {}", std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".into()));
    println!("Desktop:              {}", std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "unknown".into()));
    println!("Default Port:         {}", DEFAULT_PORT);
    println!("Service Status:       {} (ward-host.service)", svc_status);
    println!("Remote Access:        {}", if config.map(|c| c.host_enabled).unwrap_or(true) { "ENABLED ✅" } else { "DISABLED 🔒" });
    println!("Permanent PIN:        {}", if config.map(|c| c.has_pin()).unwrap_or(false) { "Configured ✅" } else { "NOT SET ⚠️ (Run 'ward-server setup')" });
    println!("Authorized Devices:   {}", clients.len());
    println!("--------------------------------------------------");
    if clients.is_empty() {
        println!("No devices paired yet. Connect from Android with permanent PIN to pair.");
    } else {
        for (id, name, last_seen) in clients {
            println!("• Device: {:<20} ID: {} (Last active: {}s ago)", name, id, ward_server::auth::current_timestamp().saturating_sub(last_seen));
        }
    }
    println!("==================================================");
    Ok(())
}

/// Milestone 2: Proof of Remote Input injection via combined RemoteDesktop + ScreenCast portal.
async fn run_remote_input_poc() -> Result<()> {
    info!("=== Milestone 2: Remote Input Proof of Concept ===");

    let mut portal = RemoteDesktopPortal::new().await?;
    portal.create_session().await?;
    portal.select_devices(device_types::KEYBOARD | device_types::POINTER).await?;
    portal.select_sources(SourceType::Monitor, CursorMode::Embedded).await?;

    info!("Starting combined RemoteDesktop + ScreenCast session...");
    let streams = portal.start("").await?;

    info!("RemoteDesktop session active! Stream count: {}", streams.len());
    for (i, stream) in streams.iter().enumerate() {
        info!("  Stream {}: PipeWire node={}, size={:?}", i, stream.pipewire_node_id, stream.size);
    }

    info!("Starting input injection sequence in 1 second...");
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Relative Pointer Motion
    for _ in 0..10 {
        portal.notify_pointer_motion(5.0, 0.0).await?;
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    info!("✅ Relative pointer motion completed");

    portal.click_pointer_button(buttons::BTN_LEFT).await?;
    info!("✅ Mouse click completed");

    portal.close().await?;
    info!("Session closed cleanly.");
    Ok(())
}

/// Milestone 1: Proof of Screen Capture via XDG ScreenCast Portal.
async fn run_screen_capture_poc() -> Result<()> {
    info!("=== Milestone 1: Screen Capture Proof of Concept ===");
    let mut portal = ScreenCastPortal::new().await?;
    portal.create_session().await?;
    portal.select_sources(SourceType::Monitor, CursorMode::Embedded, false).await?;

    let streams = portal.start("").await?;
    let stream = streams.first().context("No capture streams returned by portal")?;
    let pw_fd = portal.open_pipewire_remote().await?;

    let running = Arc::new(AtomicBool::new(true));
    let stats = Arc::new(CaptureStats::default());
    let (pw_tx, pw_rx) = std::sync::mpsc::sync_channel::<CapturedFrame>(8);

    let capture_handle = run_pipewire_capture(
        pw_fd,
        stream.pipewire_node_id,
        pw_tx,
        running.clone(),
        stats.clone(),
    )?;

    let output_dir = std::path::Path::new("captured_frames");
    std::fs::create_dir_all(output_dir)?;

    let mut frames_saved = 0;
    let deadline = std::time::Instant::now() + Duration::from_secs(3);

    while std::time::Instant::now() < deadline && frames_saved < 3 {
        if let Ok(frame) = pw_rx.recv_timeout(Duration::from_millis(500)) {
            frames_saved += 1;
            let _ = save_frame_as_png(&frame, output_dir, frames_saved);
            info!("Saved frame #{}: {}x{}", frames_saved, frame.width, frame.height);
        }
    }

    running.store(false, Ordering::Relaxed);
    let _ = capture_handle.join();
    let _ = portal.close().await;

    info!("✅ Screen capture proof completed (saved {} frames)", frames_saved);
    Ok(())
}

fn save_frame_as_png(
    frame: &CapturedFrame,
    output_dir: &Path,
    index: usize,
) -> Result<String> {
    let path = output_dir.join(format!("frame_{:04}.png", index));
    let rgba_data = convert_to_rgba(frame)?;

    let img = image::RgbaImage::from_raw(frame.width, frame.height, rgba_data)
        .context("Failed to create image from frame data")?;

    img.save(&path).context("Failed to save PNG")?;
    Ok(path.to_string_lossy().into_owned())
}

fn convert_to_rgba(frame: &CapturedFrame) -> Result<Vec<u8>> {
    let pixel_count = (frame.width * frame.height) as usize;
    let mut rgba = Vec::with_capacity(pixel_count * 4);

    match frame.format {
        FrameFormat::BGRx | FrameFormat::BGRA => {
            for y in 0..frame.height {
                let row_start = (y * frame.stride) as usize;
                for x in 0..frame.width {
                    let offset = row_start + (x as usize) * 4;
                    if offset + 3 < frame.data.len() {
                        rgba.push(frame.data[offset + 2]);
                        rgba.push(frame.data[offset + 1]);
                        rgba.push(frame.data[offset]);
                        rgba.push(255);
                    }
                }
            }
        }
        FrameFormat::RGBx | FrameFormat::RGBA => {
            for y in 0..frame.height {
                let row_start = (y * frame.stride) as usize;
                for x in 0..frame.width {
                    let offset = row_start + (x as usize) * 4;
                    if offset + 3 < frame.data.len() {
                        rgba.push(frame.data[offset]);
                        rgba.push(frame.data[offset + 1]);
                        rgba.push(frame.data[offset + 2]);
                        rgba.push(255);
                    }
                }
            }
        }
        FrameFormat::RGB => {
            for y in 0..frame.height {
                let row_start = (y * frame.stride) as usize;
                for x in 0..frame.width {
                    let offset = row_start + (x as usize) * 3;
                    if offset + 2 < frame.data.len() {
                        rgba.push(frame.data[offset]);
                        rgba.push(frame.data[offset + 1]);
                        rgba.push(frame.data[offset + 2]);
                        rgba.push(255);
                    }
                }
            }
        }
        FrameFormat::Unknown(_) => {
            for y in 0..frame.height {
                let row_start = (y * frame.stride) as usize;
                for x in 0..frame.width {
                    let offset = row_start + (x as usize) * 4;
                    if offset + 3 < frame.data.len() {
                        rgba.push(frame.data[offset + 2]);
                        rgba.push(frame.data[offset + 1]);
                        rgba.push(frame.data[offset]);
                        rgba.push(255);
                    }
                }
            }
        }
    }

    Ok(rgba)
}
