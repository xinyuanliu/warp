use std::sync::Arc;
use std::time::Duration;

use async_channel::Sender;
use async_io::Timer;
use instant::Instant;
use parking_lot::FairMutex;
use session_sharing_protocol::common::{Scrollback, WindowSize};
use session_sharing_protocol::viewer::UpstreamMessage;
use warpui::{App, ModelHandle, SingletonEntity};

use super::{Network, PtyBytesBatchStatus, Stage};
use crate::network::NetworkStatus;
use crate::system::SystemStats;
use crate::terminal::event_listener::ChannelEventListener;
use crate::terminal::shared_session::shared_handlers::RemoteUpdateGuard;
use crate::terminal::shared_session::viewer::event_loop::{
    EventLoop, SharedSessionInitialLoadMode,
};
use crate::terminal::shared_session::SharedSessionStatus;
use crate::terminal::TerminalModel;
use crate::test_util::add_window_with_terminal;
use crate::test_util::terminal::initialize_app_for_terminal_view;

fn create_network(app: &mut App) -> (ModelHandle<Network>, Sender<Vec<u8>>) {
    initialize_app_for_terminal_view(app);
    let terminal_view = add_window_with_terminal(app, None).downgrade();
    let terminal_model = Arc::new(FairMutex::new(TerminalModel::mock(None, None)));
    let channel_event_proxy = ChannelEventListener::new_for_test();
    let (write_to_pty_events_tx, write_to_pty_events_rx) = async_channel::unbounded();

    let network = app.add_model(|ctx| {
        Network::new_for_test(
            channel_event_proxy,
            terminal_view,
            terminal_model,
            write_to_pty_events_rx,
            RemoteUpdateGuard::new(),
            ctx,
        )
    });

    network.update(app, |network, _| {
        network.stage = Stage::JoinedSuccessfully;
    });

    (network, write_to_pty_events_tx)
}

/// Drives the network into a fully-joined state with a live [`EventLoop`], which
/// [`Network::reconnect_websocket`] requires in order to actually attempt a reconnect.
fn join_network_with_event_loop(app: &mut App, network: &ModelHandle<Network>) {
    network.update(app, |network, ctx| {
        // `EventLoop::new` loads scrollback into the terminal model, which debug-asserts the
        // model is a viewer, so mark it as one first.
        network.terminal_model.lock().set_shared_session_status(
            SharedSessionStatus::ActiveViewer {
                role: Default::default(),
            },
        );
        let event_loop = ctx.add_model(|ctx| {
            EventLoop::new(
                network.terminal_model.clone(),
                network.terminal_view.clone(),
                network.channel_event_proxy.clone(),
                WindowSize::default(),
                Scrollback {
                    blocks: vec![],
                    is_alt_screen_active: false,
                },
                None,
                SharedSessionInitialLoadMode::ReplaceFromSessionScrollback,
                network.remote_update_guard.clone(),
                ctx,
            )
        });
        network.event_loop = Some(event_loop);
        network.stage = Stage::JoinedSuccessfully;
    });
}

#[test]
fn test_cpu_wake_triggers_reconnect_when_joined() {
    App::test((), |mut app| async move {
        let (network, _) = create_network(&mut app);
        join_network_with_event_loop(&mut app, &network);

        // Sanity check: we start out joined, not reconnecting.
        network.read(&app, |network, _| {
            assert!(matches!(network.stage, Stage::JoinedSuccessfully));
        });

        // A CPU wake signal indicates the socket may have silently died while asleep, so the
        // viewer should proactively replace it (transitioning into `Reconnecting`).
        let system_stats = SystemStats::handle(&app);
        system_stats.update(&mut app, |system_stats, ctx| {
            system_stats.dispatch_cpu_was_awakened(ctx);
        });

        network.read(&app, |network, _| {
            assert!(
                matches!(network.stage, Stage::Reconnecting { .. }),
                "CpuWasAwakened while joined should trigger a reconnect"
            );
        });
    });
}

#[test]
fn test_network_online_triggers_reconnect_when_joined() {
    App::test((), |mut app| async move {
        let (network, _) = create_network(&mut app);
        join_network_with_event_loop(&mut app, &network);

        // Take the network offline (this tears down the live socket) and then back online.
        // Coming back online indicates connectivity was restored, so the viewer should
        // proactively reconnect rather than keep writing into a possibly-dead socket.
        let network_status = NetworkStatus::handle(&app);
        network_status.update(&mut app, |network_status, ctx| {
            network_status.reachability_changed(false, ctx);
        });
        network_status.update(&mut app, |network_status, ctx| {
            network_status.reachability_changed(true, ctx);
        });

        network.read(&app, |network, _| {
            assert!(
                matches!(network.stage, Stage::Reconnecting { .. }),
                "NetworkStatusKind::Online while joined should trigger a reconnect"
            );
        });
    });
}

#[test]
fn test_send_pty_write_event_advances_event_no() {
    App::test((), |mut app| async move {
        let (network, _) = create_network(&mut app);

        // Event number should start at 0.
        network.read(&app, |network, _ctx| {
            assert_eq!(network.write_to_pty_event_no.as_usize(), 0);
        });

        // Try to send a write to pty event message to the server.
        network.update(&mut app, |network, ctx| {
            let abort_handle = ctx.spawn_abortable(
                Timer::after(Duration::from_millis(1)),
                move |_, _, _| {},
                |_, _| {},
            );
            network.pty_bytes_batch_status = PtyBytesBatchStatus::Batching {
                accumulated: "a".into(),
                abort_handle,
            };
        });

        network.update(&mut app, |network, _| {
            network.send_write_to_pty();
        });

        // Event number is advanced to 1.
        network.read(&app, |network, _ctx| {
            assert_eq!(network.write_to_pty_event_no.as_usize(), 1);
        });
    });
}

#[test]
fn test_send_pty_write_event_while_batching() {
    App::test((), |mut app| async move {
        let (network, tx) = create_network(&mut app);
        let ws_proxy_rx = network.read(&app, |network, _ctx| network.ws_proxy_rx.clone());
        let init_time = Instant::now();

        // Reset batching status.
        network.update(&mut app, |network, _ctx| {
            network.pty_bytes_batch_status = PtyBytesBatchStatus::NotBatching {
                last_sent_at: init_time,
            };
        });

        // Try to send write to pty events.
        tx.try_send("a".into())
            .expect("Can send event over write_to_pty_tx");
        tx.try_send("b".into())
            .expect("Can send event over write_to_pty_tx");

        // Ensure the accumulated event is sent to the server, and the item in ws_proxy_tx is correct.
        let item = ws_proxy_rx.recv().await;
        assert!(
            matches!(item.unwrap(), UpstreamMessage::WriteToPty { bytes, .. } if bytes == b"ab")
        );

        // The batch status should be updated.
        network.read(&app, |network, _ctx| {
            assert!(matches!(network.pty_bytes_batch_status, PtyBytesBatchStatus::NotBatching { last_sent_at } if last_sent_at > init_time));
        });
    });
}

#[test]
fn test_send_pty_write_event_while_not_batching() {
    App::test((), |mut app| async move {
        let (network, _) = create_network(&mut app);
        let ws_proxy_rx = network.read(&app, |network, _ctx| network.ws_proxy_rx.clone());
        let init_time = Instant::now();

        // Set batch status to not batching.
        network.update(&mut app, |network, _ctx| {
            network.pty_bytes_batch_status = PtyBytesBatchStatus::NotBatching {
                last_sent_at: init_time,
            };
        });

        // Try to send write to pty message to server.
        network.update(&mut app, |network, _| {
            network.send_write_to_pty();
        });

        // Make sure we didn't try to send anything to the server.
        assert_eq!(ws_proxy_rx.len(), 0);

        // The batch status should be unchanged.
        network.read(&app, |network, _ctx| {
            assert!(matches!(network.pty_bytes_batch_status, PtyBytesBatchStatus::NotBatching { last_sent_at } if last_sent_at == init_time));
        });
    });
}
