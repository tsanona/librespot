use crate::{
    core::{
        authentication::Credentials,
        dealer::{
            manager::BoxedStreamResult,
            protocol::Message,
        },
        Error, Session,
    },
    protocol::{
        connect::{Cluster, ClusterUpdate},
    },
    state::{
        {ConnectConfig, ConnectState},
    },
};
use futures_util::StreamExt;
use std::{
    future::Future,
    sync::atomic::{AtomicUsize, Ordering},
};
use thiserror::Error;
use tokio::sync::mpsc;

#[derive(Debug, Error)]
enum ObserverError {
    #[error("message pushed for another URI")]
    InvalidUri(String),
    #[error("failed to put connect state for new device")]
    FailedDealerSetup,
}

impl From<ObserverError> for Error {
    fn from(err: ObserverError) -> Self {
        use ObserverError::*;
        match err {
            InvalidUri(_) | FailedDealerSetup => Error::aborted(err),
        }
    }
}

struct ObserverTask {
    task_id: usize,

    connection_id_update: BoxedStreamResult<String>,

    session: Session,

    /// the state management object
    connect_state: ConnectState,
    connect_state_update: BoxedStreamResult<ClusterUpdate>,

    changes: mpsc::UnboundedSender<ClusterUpdate>,
}

static SPIRC_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// The spotify connect handle
pub struct Observer {
    pub changes: mpsc::UnboundedReceiver<ClusterUpdate>,
}

impl Observer {
    /// Initializes a new spotify connect device
    ///
    /// The returned tuple consists out of a handle to the [`Observer`] that
    /// can control the local connect device when active. And a [`Future`]
    /// which represents the [`Observer`] event loop that processes the whole
    /// connect device logic.
    pub async fn new(
        config: ConnectConfig,
        session: Session,
        credentials: Credentials,
    ) -> Result<(Observer, impl Future<Output = ()>), Error> {
        fn extract_connection_id(msg: Message) -> Result<String, Error> {
            let connection_id = msg
                .headers
                .get("Spotify-Connection-Id")
                .ok_or_else(|| ObserverError::InvalidUri(msg.uri.clone()))?;
            Ok(connection_id.to_owned())
        }

        let task_id = SPIRC_COUNTER.fetch_add(1, Ordering::AcqRel);
        debug!("new Observer[{}]", task_id);

        let connection_id_update = session
            .dealer()
            .listen_for("hm://pusher/v1/connections/", extract_connection_id)?;

        let connect_state = ConnectState::new(config, &session);

        let connect_state_update = session
            .dealer()
            .listen_for("hm://connect-state/v1/cluster", Message::from_raw)?;

        // pre-acquire client_token, preventing multiple request while running
        let _ = session.spclient().client_token().await?;

        // Connect *after* all message listeners are registered
        session.connect(credentials, true).await?;

        // pre-acquire access_token (we need to be authenticated to retrieve a token)
        let _ = session.login5().auth_token().await?;

        //let context_resolver = ContextResolver::new(session.clone());
        let (chngs_tx, chngs_rx) = mpsc::unbounded_channel();

        let task = ObserverTask {
            task_id,

            connection_id_update,

            session,

            connect_state,
            connect_state_update,

            changes: chngs_tx
        };

        let observer = Observer { changes: chngs_rx };

        Ok((observer, task.run()))
    }
}

impl ObserverTask {
    async fn run(mut self) {
        // simplify unwrapping of received item or parsed result
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

        if let Err(why) = self.session.dealer().start().await {
            error!("starting dealer failed: {why}");
            return;
        }

        while !self.session.is_invalid() {
            tokio::select! {
                // startup of the dealer requires a connection_id, which is retrieved at the very beginning
                connection_id_update = self.connection_id_update.next() => unwrap! {
                    connection_id_update,
                    match |connection_id| if let Err(why) = self.handle_connection_id_update(connection_id).await {
                        error!("failed handling connection id update: {why}");
                        break;
                    }
                },
                // main dealer update of any remote device updates
                cluster_update = self.connect_state_update.next() => unwrap! {
                    cluster_update,
                    match |cluster_update| if let Err(e) = self.handle_cluster_update(cluster_update).await {
                        error!("could not dispatch connect state update: {}", e);
                    }
                },
                else => break
            }
        }

        self.session.dealer().close().await;
    }

    async fn handle_connection_id_update(&mut self, connection_id: String) -> Result<(), Error> {
        trace!("Received connection ID update: {:?}", connection_id);
        self.session.set_connection_id(&connection_id);

        use protobuf::Message;

        let cluster = match self
            .connect_state
            .notify_new_device_appeared(&self.session)
            .await
        {
            Ok(res) => Cluster::parse_from_bytes(&res).ok(),
            Err(why) => {
                error!("{why:?}");
                None
            }
        }
        .ok_or(ObserverError::FailedDealerSetup)?;

        debug!(
            "successfully put connect state for {} with connection-id {connection_id}",
            self.session.device_id()
        );

        let same_session = cluster.player_state.session_id == self.session.session_id()
            || cluster.player_state.session_id.is_empty();
        if !cluster.active_device_id.is_empty() || !same_session {
            info!(
                "active device is <{}> with session <{}>",
                cluster.active_device_id, cluster.player_state.session_id
            );
            return Ok(());
        } else if cluster.transfer_data.is_empty() {
            debug!("got empty transfer state, do nothing");
            return Ok(());
        } else {
            info!(
                "trying to take over control automatically, session_id: {}",
                cluster.player_state.session_id
            )
        }

        Ok(())
    }

    async fn handle_cluster_update(
        &mut self,
        cluster_update: ClusterUpdate,
    ) -> Result<(), Error> {
        let reason = cluster_update.update_reason.enum_value();
        let device_ids = cluster_update.devices_that_changed.join(", ");
        debug!(
            "cluster update: {reason:?} from {device_ids}, active device: {}",
            cluster_update.cluster.active_device_id
        );

        Ok(self.changes.send(cluster_update)?)
    }
}

impl Drop for ObserverTask {
    fn drop(&mut self) {
        debug!("drop Observer[{}]", self.task_id);
    }
}