use librespot::{
    connect::{config::ConnectConfig, spirc::Spirc},
    core::{
        authentication::Credentials,
        config::{DeviceType, SessionConfig},
        session::Session,
    },
};
use std::{io, process::exit};
//use dotenv;
use env_logger;
use log::{error, info, warn, LevelFilter};

#[tokio::main]
async fn main() {
    let mut builder = env_logger::Builder::new();
    builder.filter_level(LevelFilter::Info);
    builder.init();

    let session_config = SessionConfig::default();
    let connect_config = ConnectConfig {
        name: "Logger".to_string(),
        device_type: DeviceType::Observer,
        initial_volume: None,
        has_volume_ctrl: false,
        can_play: false,
    };

    println!("Connecting...");
    let credentials = Credentials::with_password("username", "password");
    let session = Session::new(session_config, None);

    //session.connect(credentials.clone(), false).await.unwrap();

    let (spirc_, spirc_task_, events_) = match Spirc::new(
        connect_config,
        session.clone(),
        credentials.clone(),
        None,
        None,
    )
    .await
    {
        Ok((spirc_, spirc_task_, events_)) => (spirc_, spirc_task_, events_),
        Err(e) => {
            error!("could not initialize spirc: {}", e);
            exit(1);
        }
    };
    let spirc = spirc_;
    let mut spirc_task = Box::pin(spirc_task_.run());
    let mut events = Box::pin(events_);

    loop {
        tokio::select! {
            _ = async{
                spirc_task.as_mut().await
            } => (),
            _ = async {
                if let Ok(frame) = events.recv().await {
                    info!("{frame:?}")
                }
            } => (),
             _ = tokio::signal::ctrl_c() => {
                break;
            },
            else => break,
        }
    }

    // tokio::spawn(async move {
    //     tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    //     println!("Sending Sending commands");
    //     spirc.play_pause();
    //     tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    //     spirc.play_pause();
    //     tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    //     spirc.prev();
    //     tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    //     spirc.next();
    //     tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    //     spirc.volume_down();
    //     tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    //     spirc.volume_down();
    //     tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    //     spirc.volume_down();
    //     tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    //     spirc.volume_down();
    //     tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    //     spirc.volume_down();
    //     tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    //     spirc.volume_down();
    //     tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    //     spirc.volume_up();
    //     tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    //     spirc.volume_up();
    //     tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    //     spirc.volume_up();
    //     tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    //     spirc.volume_up();
    //     tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    //     spirc.volume_up();
    //     tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    //     spirc.volume_up();
    //     tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    //     spirc.shuffle(true);
    //     tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    //     spirc.shuffle(false);
    //     tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    //     spirc.repeat(true);
    //     tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    //     spirc.repeat(false);
    //     tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    //     spirc.set_position_ms(60000);
    //     tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    //     spirc.deactivate();
    //     tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    //     spirc.shutdown();
    // });
}
