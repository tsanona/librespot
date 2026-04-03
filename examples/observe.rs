use std::process::exit;

use librespot::{
    connect::{ConnectConfig, Observer},
    discovery::DeviceType,
    core::{
        Error, authentication::Credentials, cache::Cache, config::SessionConfig, session::Session,
    },
};

use log::LevelFilter;

const CACHE: &str = ".cache";
const CACHE_FILES: &str = ".cache/files";

#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::builder()
        .filter_module("librespot", LevelFilter::Debug)
        .init();

    let session_config = SessionConfig::default();
    let connect_config = ConnectConfig { name: String::from("test_observer"), device_type: DeviceType::Observer, can_play: false, ..Default::default() };

    let cache = Cache::new(Some(CACHE), Some(CACHE), Some(CACHE_FILES), None)?;
    let credentials = cache
        .credentials()
        .ok_or(Error::unavailable("credentials not cached"))
        .or_else(|_| {
            librespot_oauth::OAuthClientBuilder::new(
                &session_config.client_id,
                "http://127.0.0.1:8898/login",
                vec!["streaming"],
            )
            .open_in_browser()
            .build()?
            .get_access_token()
            .map(|t| Credentials::with_access_token(t.access_token))
        })?;

    let session = Session::new(session_config, Some(cache));

    let (mut observer, observer_task) =
        Observer::new(connect_config, session.clone(), credentials).await?;

    let mut observer_task = Box::pin(observer_task);

    loop {
        tokio::select! {
            Some(change) = observer.changes.recv() => {
                println!("Got server update: {:?}", change.update_reason);
            }
            _ = observer_task.as_mut() => {
                println!("Spirc shut down unexpectedly");
                exit(1)
            },
            _ = tokio::signal::ctrl_c() => {
                break;
            },
            else => break,
        }
    }

    // println!("Gracefully shutting down");

    // let mut shutdown_tasks = tokio::task::JoinSet::new();

    // // Shutdown spirc if necessary
    // if let Some(spirc) = spirc {
    //     if let Err(e) = spirc.shutdown() {
    //         error!("error sending spirc shutdown message: {e}");
    //     }

    //     if let Some(spirc_task) = spirc_task {
    //         shutdown_tasks.spawn(spirc_task);
    //     }
    // }

    // tokio::select! {
    //     _ = tokio::signal::ctrl_c() => (),
    //     _ = shutdown_tasks.join_all() => (),
    // }

    Ok(())
}