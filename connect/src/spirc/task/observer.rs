use crate::{
    ConnectState, SpircError,
    core::{Error, dealer::{manager::BoxedStreamResult, protocol::Message}},
    protocol::connect::{Cluster, ClusterUpdate},
    spirc::task::{SPIRC_COUNTER, SpircTask, SpircArgs},
    unwrap,
};
use futures_util::StreamExt;
use std::sync::atomic::Ordering;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

pub struct ObserverTask {
    id: usize,

    /// the state management object
    connect_state: ConnectState,
    connect_state_update: BoxedStreamResult<ClusterUpdate>,

    connection_id_update: BoxedStreamResult<String>,

    changes: UnboundedSender<ClusterUpdate>,
}

impl SpircTask for ObserverTask {
    type Args = SpircArgs;
    type Interface = UnboundedReceiver<ClusterUpdate>;
    /// Initializes a new spotify connect device as Observer.
    /// This device cannot be used to play.
    ///
    /// The returned tuple consists out of a handle to the [`Observer`] that
    /// can control the local connect device when active. And a [`Future`]
    /// which represents the [`Observer`] event loop that processes the whole
    /// connect device logic.
    async fn new(
        args: Self::Args
    ) -> Result<(Self, Self::Interface), Error> {
        let spirc_id = SPIRC_COUNTER.fetch_add(1, Ordering::AcqRel);
        debug!("new Spirc[{spirc_id}]");

        let SpircArgs { config, session, credentials } = args;

        let connect_state = ConnectState::new(config, session.clone());

        let connection_id_update = session
            .dealer()
            .listen_for("hm://pusher/v1/connections/", Self::extract_connection_id)?;

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
        let (chngs_tx, chngs_rx) = unbounded_channel();

        let task = ObserverTask {
            id: spirc_id,

            connect_state,
            connect_state_update,

            connection_id_update,

            changes: chngs_tx,
        };

        Ok((task, chngs_rx))
    }

    async fn run(mut self) {
        if let Err(why) = self.connect_state.session.dealer().start().await {
            error!("starting dealer failed: {why}");
            return;
        }

        
        while !self.connect_state.session.is_invalid() {
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

        self.connect_state.session.dealer().close().await;
    }
}

impl ObserverTask {
    async fn handle_connection_id_update(&mut self, connection_id: String) -> Result<(), Error> {
        trace!("Received connection ID update: {:?}", connection_id);
        self.connect_state.session.set_connection_id(&connection_id);

        use protobuf::Message;

        let cluster = match self.connect_state.notify_new_device_appeared().await {
            Ok(res) => Cluster::parse_from_bytes(&res).ok(),
            Err(why) => {
                error!("{why:?}");
                None
            }
        }
        .ok_or(SpircError::FailedDealerSetup)?;

        debug!(
            "successfully put connect state for {} with connection-id {connection_id}",
            self.connect_state.session.device_id()
        );

        let same_session = cluster.player_state.session_id
            == self.connect_state.session.session_id()
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
        }

        Ok(())
    }

    async fn handle_cluster_update(&mut self, cluster_update: ClusterUpdate) -> Result<(), Error> {
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
        debug!("drop Observer[{}]", self.id);
    }
}
