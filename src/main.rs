use futures_util::StreamExt;
use librespot::connect::Spirc;
use log::{error, info, warn};
use std::{
    env,
    ffi::OsStr,
    process::exit,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;

mod player_event_handler;

mod setup;
use setup::get_setup;

// Initialize a static semaphore with only one permit, which is used to
// prevent setting environment variables from running in parallel.
static PERMIT: Semaphore = Semaphore::const_new(1);
async fn set_env_var<K: AsRef<OsStr>, V: AsRef<OsStr>>(key: K, value: V) {
    let permit = PERMIT
        .acquire()
        .await
        .expect("Failed to acquire semaphore permit");

    // SAFETY: This is safe because setting the environment variable will wait if the permit is
    // already acquired by other callers.
    unsafe { env::set_var(key, value) }

    // Drop the permit manually, so the compiler doesn't optimize it away as unused variable.
    drop(permit);
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    const RUST_BACKTRACE: &str = "RUST_BACKTRACE";
    const RECONNECT_RATE_LIMIT_WINDOW: Duration = Duration::from_secs(600);
    const RECONNECT_RATE_LIMIT: usize = 5;

    if env::var(RUST_BACKTRACE).is_err() {
        set_env_var(RUST_BACKTRACE, "full").await;
    }

    let setup = get_setup().await;

    let mut spirc: Option<Spirc> = None;
    let mut spirc_task: Option<_> = None;
    let mut auto_connect_times: Vec<Instant> = vec![];
    let mut connecting = false;

    let mut session = setup.get_session();

    let mut discovery = setup.get_discovery().await;

    let mut last_credentials = setup.get_credentials(&mut connecting);

    if discovery.is_none() && (last_credentials).is_none() {
        error!(
            "Discovery is unavailable and no credentials provided. Authentication is not possible."
        );
        exit(1);
    }

    let mixer = setup.get_mixer();
    let player = setup.get_player(mixer.clone(), session.clone());

    let _player_event_handler = setup.get_player_event_handler(player.clone());

    loop {
        tokio::select! {
            credentials = async {
                match discovery.as_mut() {
                    Some(d) => d.next().await,
                    _ => None
                }
            }, if discovery.is_some() => {
                match credentials {
                    Some(credentials) => {
                        last_credentials = Some(Arc::new(credentials));
                        auto_connect_times.clear();

                        if let Some(spirc) = spirc.take() {
                            if let Err(e) = spirc.shutdown() {
                                error!("error sending spirc shutdown message: {e}");
                            }
                        }
                        if let Some(spirc_task) = spirc_task.take() {
                            // Continue shutdown in its own task
                            tokio::spawn(spirc_task);
                        }
                        if !session.is_invalid() {
                            session.shutdown();
                        }

                        connecting = true;
                    },
                    None => {
                        error!("Discovery stopped unexpectedly");
                        exit(1);
                    }
                }
            },
            _ = async {}, if connecting => {
                if let Some(credentials) = &last_credentials {
                    if session.is_invalid() {
                        session = setup.get_session();
                        player.set_session(session.clone());
                    }

                    (spirc, spirc_task) = setup.get_spirc(session.clone(),(**credentials).clone(), player.clone(), mixer.clone()).await;
                    connecting = false;
                }
            },
            _ = async {
                if let Some(task) = spirc_task.as_mut() {
                    task.await;
                }
            }, if spirc_task.is_some() && !connecting => {
                spirc_task = None;

                warn!("Spirc shut down unexpectedly");

                let mut reconnect_exceeds_rate_limit = || {
                    auto_connect_times.retain(|&t| t.elapsed() < RECONNECT_RATE_LIMIT_WINDOW);
                    auto_connect_times.len() > RECONNECT_RATE_LIMIT
                };

                if last_credentials.is_some() && !reconnect_exceeds_rate_limit() {
                    auto_connect_times.push(Instant::now());
                    if !session.is_invalid() {
                        session.shutdown();
                    }
                    connecting = true;
                } else {
                    error!("Spirc shut down too often. Not reconnecting automatically.");
                    exit(1);
                }
            },
            _ = async {}, if player.is_invalid() => {
                error!("Player shut down unexpectedly");
                exit(1);
            },
            _ = tokio::signal::ctrl_c() => {
                break;
            },
            else => break,
        }
    }

    info!("Gracefully shutting down");

    let mut shutdown_tasks = tokio::task::JoinSet::new();

    // Shutdown spirc if necessary
    if let Some(spirc) = spirc {
        if let Err(e) = spirc.shutdown() {
            error!("error sending spirc shutdown message: {e}");
        }

        if let Some(spirc_task) = spirc_task {
            shutdown_tasks.spawn(spirc_task);
        }
    }

    if let Some(discovery) = discovery {
        shutdown_tasks.spawn(discovery.shutdown());
    }

    tokio::select! {
        _ = tokio::signal::ctrl_c() => (),
        _ = shutdown_tasks.join_all() => (),
    }
}
