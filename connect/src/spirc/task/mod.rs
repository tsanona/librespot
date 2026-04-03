pub mod commander;
pub mod observer;
pub mod player;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::{
    SpircArgs, SpircError, core::{Error, dealer::protocol::Message},
};

use std::{sync::atomic::AtomicUsize, time::Duration};

// delay to update volume after a certain amount of time, instead on each update request
pub(crate) const VOLUME_UPDATE_DELAY: Duration = Duration::from_millis(500);
// to reduce updates to remote, we group some request by waiting for a set amount of time
pub(crate) const UPDATE_STATE_DELAY: Duration = Duration::from_millis(200);

pub(crate) const CONTEXT_FETCH_THRESHOLD: usize = 2;

pub(crate) static SPIRC_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[macro_export]
/// Simplify unwrapping of received item or parsed result
macro_rules! unwrap {
    ( $next:expr, |$some:ident| $use_some:expr ) => {
        match $next {
            Some($some) => $use_some,
            None => {
                error!("{} selected, but none received", stringify!($next));
                break;
            }
        }
    };
    ( $next:expr, match |$ok:ident| $use_ok:expr ) => {
        unwrap!($next, |$ok| match $ok {
            Ok($ok) => $use_ok,
            Err(why) => error!("could not parse {}: {}", stringify!($ok), why),
        })
    };
}

pub trait SpircTask {
    type Args;
    type Interface;

    async fn new(
        args: Self::Args
    ) -> Result<(Self, Self::Interface), Error>
    where
        Self: Sized;

    async fn run(self);

    fn extract_connection_id(msg: Message) -> Result<String, Error> {
        let connection_id = msg
            .headers
            .get("Spotify-Connection-Id")
            .ok_or_else(|| SpircError::InvalidUri(msg.uri.clone()))?;
        Ok(connection_id.to_owned())
    }
}

pub trait CanObserve {
    type Object;
    async fn recv(&mut self) -> Option<Self::Object>;
}

impl<T> CanObserve for UnboundedReceiver<T> {
    type Object = T;
    async fn recv(&mut self) -> Option<T> {
        self.recv().await
    }
}

use crate::{LoadRequest, core::{SpotifyUri, spclient::TransferRequest}};

#[derive(Debug)]
pub enum SpircCommand {
    Play,
    PlayPause,
    Pause,
    Prev,
    Next,
    VolumeUp,
    VolumeDown,
    Shutdown,
    Shuffle(bool),
    Repeat(bool),
    RepeatTrack(bool),
    Disconnect { pause: bool },
    SetPosition(u32),
    SetVolume(u16),
    Activate,
    Transfer(Option<TransferRequest>),
    Load(LoadRequest),
    AddToQueue(SpotifyUri),
}

pub(crate) trait CanCommand {
    fn send(&self, command: SpircCommand) -> Result<(), Error>;
}

impl CanCommand for UnboundedSender<SpircCommand> {
    fn send(&self, command: SpircCommand) -> Result<(), Error> {
        Ok(self.send(command)?)
    }
}

// pub trait CanPlay {
//     unimplemented!()
// }
