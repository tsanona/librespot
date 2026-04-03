mod task;

use librespot_core::spclient::TransferRequest;
pub use task::{commander::CommanderTask, observer::ObserverTask, player::PlayerTask, SpircCommand};

use crate::{
    LoadRequest, core::{Error, Session, SpotifyUri, authentication::Credentials}, playback::{mixer::Mixer, player::Player}, spirc::task::{CanCommand, CanObserve, SpircTask}, state::ConnectConfig
};
use std::{future::Future, sync::Arc};
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum SpircError {
    #[error("response payload empty")]
    NoData,
    #[error("{0} had no uri")]
    NoUri(&'static str),
    #[error("message pushed for another URI")]
    InvalidUri(String),
    #[error("failed to put connect state for new device")]
    FailedDealerSetup,
    #[error("unknown endpoint: {0:#?}")]
    UnknownEndpoint(serde_json::Value),
}

impl From<SpircError> for Error {
    fn from(err: SpircError) -> Self {
        use SpircError::*;
        match err {
            NoData | NoUri(_) => Error::unavailable(err),
            InvalidUri(_) | FailedDealerSetup => Error::aborted(err),
            UnknownEndpoint(_) => Error::unimplemented(err),
        }
    }
}

/// The spotify connect handle
pub struct Spirc<T: SpircTask> {
    interface: T::Interface,
}

pub struct SpircArgs {
    config: ConnectConfig,
    session: Session,
    credentials: Credentials
}

impl<T: SpircTask<Args = SpircArgs>> Spirc<T> {
    /// Initializes a new spotify connect device
    ///
    /// The returned tuple consists out of a handle to the [`Spirc`] that
    /// can control the local connect device when active. And a [`Future`]
    /// which represents the [`Spirc`] event loop that processes the whole
    /// connect device logic.
    pub async fn new(
        config: ConnectConfig,
        session: Session,
        credentials: Credentials
    ) -> Result<(Spirc<T>, impl Future<Output = ()>), Error> {
        let (task, interface) = T::new(T::Args {config, session, credentials}).await?;
        let spirc = Spirc { interface };
        Ok((spirc, task.run()))
    }
}

pub struct SpircPlayerArgs {
    config: ConnectConfig,
    session: Session,
    credentials: Credentials,
    player: Arc<Player>,
    mixer: Arc<dyn Mixer>
}

impl<T: SpircTask<Args = SpircPlayerArgs>> Spirc<T> {
    /// Initializes a new spotify connect device
    ///
    /// The returned tuple consists out of a handle to the [`Spirc`] that
    /// can control the local connect device when active. And a [`Future`]
    /// which represents the [`Spirc`] event loop that processes the whole
    /// connect device logic.
    pub async fn new_with_player(
        config: ConnectConfig,
        session: Session,
        credentials: Credentials,
        player: Arc<Player>,
        mixer: Arc<dyn Mixer>
    ) -> Result<(Spirc<T>, impl Future<Output = ()>), Error> {
        let (task, interface) = T::new(T::Args {config, session, credentials, player, mixer}).await?;
        let spirc = Spirc { interface };
        Ok((spirc, task.run()))
    }
}

impl<T: SpircTask> Spirc<T>
where
    T::Interface: CanObserve,
{
    pub async fn recv(&mut self) -> Option<<T::Interface as CanObserve>::Object> {
        self.interface.recv().await
    }
}

impl<T: SpircTask> Spirc<T>
where
    T::Interface: CanCommand,
{
    /// Safely shutdowns the spirc.
    ///
    /// This pauses the playback, disconnects the connect device and
    /// bring the future initially returned to an end.
    pub fn shutdown(&self) -> Result<(), Error> {
        self.interface.send(SpircCommand::Shutdown)
    }

    /// Resumes the playback
    ///
    /// Does nothing if we are not the active device, or it isn't paused.
    pub fn play(&self) -> Result<(), Error> {
        self.interface.send(SpircCommand::Play)
    }

    /// Resumes or pauses the playback
    ///
    /// Does nothing if we are not the active device.
    pub fn play_pause(&self) -> Result<(), Error> {
        self.interface.send(SpircCommand::PlayPause)
    }

    /// Pauses the playback
    ///
    /// Does nothing if we are not the active device, or if it isn't playing.
    pub fn pause(&self) -> Result<(), Error> {
        self.interface.send(SpircCommand::Pause)
    }

    /// Seeks to the beginning or skips to the previous track.
    ///
    /// Seeks to the beginning when the current track position
    /// is greater than 3 seconds.
    ///
    /// Does nothing if we are not the active device.
    pub fn prev(&self) -> Result<(), Error> {
        self.interface.send(SpircCommand::Prev)
    }

    /// Skips to the next track.
    ///
    /// Does nothing if we are not the active device.
    pub fn next(&self) -> Result<(), Error> {
        self.interface.send(SpircCommand::Next)
    }

    /// Increases the volume by configured steps of [ConnectConfig].
    ///
    /// Does nothing if we are not the active device.
    pub fn volume_up(&self) -> Result<(), Error> {
        self.interface.send(SpircCommand::VolumeUp)
    }

    /// Decreases the volume by configured steps of [ConnectConfig].
    ///
    /// Does nothing if we are not the active device.
    pub fn volume_down(&self) -> Result<(), Error> {
        self.interface.send(SpircCommand::VolumeDown)
    }

    /// Shuffles the playback according to the value.
    ///
    /// If true shuffles/reshuffles the playback. Otherwise, does
    /// nothing (if not shuffled) or unshuffles the playback while
    /// resuming at the position of the current track.
    ///
    /// Does nothing if we are not the active device.
    pub fn shuffle(&self, shuffle: bool) -> Result<(), Error> {
        self.interface.send(SpircCommand::Shuffle(shuffle))
    }

    /// Repeats the playback context according to the value.
    ///
    /// Does nothing if we are not the active device.
    pub fn repeat(&self, repeat: bool) -> Result<(), Error> {
        self.interface.send(SpircCommand::Repeat(repeat))
    }

    /// Repeats the current track if true.
    ///
    /// Does nothing if we are not the active device.
    ///
    /// Skipping to the next track disables the repeating.
    pub fn repeat_track(&self, repeat: bool) -> Result<(), Error> {
        self.interface.send(SpircCommand::RepeatTrack(repeat))
    }

    /// Update the volume to the given value.
    ///
    /// Does nothing if we are not the active device.
    pub fn set_volume(&self, volume: u16) -> Result<(), Error> {
        self.interface.send(SpircCommand::SetVolume(volume))
    }

    /// Updates the position to the given value.
    ///
    /// Does nothing if we are not the active device.
    ///
    /// If value is greater than the track duration,
    /// the update is ignored.
    pub fn set_position_ms(&self, position_ms: u32) -> Result<(), Error> {
        self.interface.send(SpircCommand::SetPosition(position_ms))
    }

    pub fn add_to_queue(&self, uri: SpotifyUri) -> Result<(), Error> {
        if !matches!(
            uri,
            SpotifyUri::Track { .. }
                | SpotifyUri::Episode { .. }
                | SpotifyUri::Album { .. }
                | SpotifyUri::Playlist { .. }
        ) {
            return Err(Error::invalid_argument("uri"));
        }
        self.interface.send(SpircCommand::AddToQueue(uri))
    }

    /// Disconnects the current device and pauses the playback according the value.
    ///
    /// Does nothing if we are not the active device.
    pub fn disconnect(&self, pause: bool) -> Result<(), Error> {
        self.interface.send(SpircCommand::Disconnect { pause })
    }
}

impl Spirc<PlayerTask> {
    /// Load a new context and replace the current.
    ///
    /// Does nothing if we are not the active device.
    ///
    /// Does not overwrite the queue.
    pub fn load(&self, command: LoadRequest) -> Result<(), Error> {
        Ok(self.interface.send(SpircCommand::Load(command))?)
    }

    /// Acquires the control as active connect device.
    ///
    /// Does not [Spirc::transfer] the playback. Does nothing if we are not the active device.
    pub fn activate(&self) -> Result<(), Error> {
        Ok(self.interface.send(SpircCommand::Activate)?)
    }

    /// Acquires the control as active connect device over the transfer flow.
    ///
    /// Does nothing if we are not the active device.
    pub fn transfer(&self, transfer_request: Option<TransferRequest>) -> Result<(), Error> {
        Ok(self
            .interface
            .send(SpircCommand::Transfer(transfer_request))?)
    }
}