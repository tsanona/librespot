use std::{process::exit, io};
use librespot::{
    connect::{config::ConnectConfig, spirc::{Spirc}},
    core::{
        authentication::Credentials,
        config::{DeviceType, SessionConfig},
        session::Session,
    },
};
//use dotenv;
use env_logger;
use log::{error, warn, LevelFilter, info};

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
        can_play: false
    };

    println!("Connecting...");
    let credentials = Credentials::with_password("username", "password");
    let session = Session::new(session_config, None);

    //session.connect(credentials.clone(), false).await.unwrap();

    let (spirc_, spirc_task_) = match Spirc::new(connect_config, session.clone(), credentials.clone(), None, None).await {
        Ok((spirc_, spirc_task_)) => (spirc_, spirc_task_),
        Err(e) => {
            error!("could not initialize spirc: {}", e);
            exit(1);
        }
    };
    let spirc = spirc_;
    let spirc_task = Box::pin(spirc_task_);

    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        info!("Sending PlayPause");
        spirc.pause()
    });
    spirc_task.await
    
}