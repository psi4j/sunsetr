//! GeoClue as the coordinate source for geo mode.
//!
//! One thread owns the D-Bus connection, the GeoClue client and its signal
//! stream, and dispatches `SignalMessage::LocationUpdate` for each fix and
//! `SignalMessage::GeoclueAgent` when the authorization agent is found
//! missing or present. [`GeoclueMonitor`] is the main thread's handle for
//! starting and stopping the client as the configuration changes.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;
use zbus::{Connection, OwnedObjectPath};
use zruntime::{Runtime, Shared, mpmc};

use crate::common::constants::LOCATION_CHANGE_FLOOR_KM;
use crate::common::utils::format_coordinates;
use crate::io::signals::SignalMessage;

const DESKTOP_ID: &str = "sunsetr";
/// GClueAccuracyLevel city.
const ACCURACY_CITY: u32 = 4;
/// How long startup waits for a first fix. GetClient stays pending until an
/// authorization agent registers for the session, so the missing agent is
/// also reported after this long.
pub const FIRST_FIX_WAIT: Duration = Duration::from_secs(5);
const MAX_RESTARTS: u8 = 3;
const RESTART_DELAY: Duration = Duration::from_secs(5);

#[zbus::proxy(
    interface = "org.freedesktop.GeoClue2.Manager",
    default_service = "org.freedesktop.GeoClue2",
    default_path = "/org/freedesktop/GeoClue2/Manager"
)]
trait Manager {
    fn get_client(&self) -> zbus::Result<OwnedObjectPath>;
}

#[zbus::proxy(
    interface = "org.freedesktop.GeoClue2.Client",
    default_service = "org.freedesktop.GeoClue2"
)]
trait Client {
    fn start(&self) -> zbus::Result<()>;
    fn stop(&self) -> zbus::Result<()>;

    #[zbus(property)]
    fn set_desktop_id(&self, id: &str) -> zbus::Result<()>;
    #[zbus(property)]
    fn set_requested_accuracy_level(&self, level: u32) -> zbus::Result<()>;
    #[zbus(property)]
    fn set_distance_threshold(&self, meters: u32) -> zbus::Result<()>;
    #[zbus(property)]
    fn location(&self) -> zbus::Result<OwnedObjectPath>;

    #[zbus(signal)]
    fn location_updated(&self, old: OwnedObjectPath, new: OwnedObjectPath) -> zbus::Result<()>;
}

#[zbus::proxy(
    interface = "org.freedesktop.GeoClue2.Location",
    default_service = "org.freedesktop.GeoClue2"
)]
trait Location {
    #[zbus(property)]
    fn latitude(&self) -> zbus::Result<f64>;
    #[zbus(property)]
    fn longitude(&self) -> zbus::Result<f64>;
    #[zbus(property)]
    fn accuracy(&self) -> zbus::Result<f64>;
    #[zbus(property)]
    fn description(&self) -> zbus::Result<String>;
}

/// Main-thread handle to the GeoClue client.
///
/// The thread, and with it the GeoClue daemon it activates, starts on the
/// first `set_active(true)`, so a manual-source configuration never touches
/// GeoClue.
pub struct GeoclueMonitor {
    link: Link,
    wake: mpmc::Sender<()>,
}

/// What the thread shares with the main thread. `running` is set while the
/// thread lives and lets `set_active(true)` spawn a new one once it has exited.
#[derive(Clone)]
struct Link {
    signal_sender: Sender<SignalMessage>,
    wanted: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    wake: mpmc::Receiver<()>,
    fix_state: Arc<FixState>,
    debug_enabled: bool,
}

/// A location GeoClue reported, with the time it arrived. `cached` marks a
/// fix read back from the state directory by a later process.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fix {
    pub latitude: f64,
    pub longitude: f64,
    pub fixed_at: DateTime<Utc>,
    pub cached: bool,
}

impl Fix {
    pub fn live(latitude: f64, longitude: f64) -> Self {
        Self {
            latitude,
            longitude,
            fixed_at: Utc::now(),
            cached: false,
        }
    }
}

pub enum FixWait {
    Fix(Fix),
    /// The missing-agent notice fired before any fix arrived.
    NoAgent,
    /// The wait timed out with GetClient still pending.
    Pending,
}

/// The thread's last fix and what it knows about the authorization agent:
/// `None` while GetClient is pending, `Some(false)` once the notice fired,
/// `Some(true)` once GetClient returned.
#[derive(Default)]
struct FixState {
    slot: Mutex<FixSlot>,
    ready: Condvar,
}

#[derive(Default)]
struct FixSlot {
    fix: Option<Fix>,
    agent_present: Option<bool>,
}

impl FixState {
    fn store_fix(&self, fix: Fix) {
        self.slot.lock().unwrap().fix = Some(fix);
        self.ready.notify_all();
    }

    fn store_agent(&self, present: bool) {
        self.slot.lock().unwrap().agent_present = Some(present);
        self.ready.notify_all();
    }

    fn latest_fix(&self) -> Option<Fix> {
        self.slot.lock().unwrap().fix
    }

    fn agent_present(&self) -> Option<bool> {
        self.slot.lock().unwrap().agent_present
    }

    fn wait(&self, timeout: Duration) -> FixWait {
        let guard = self.slot.lock().unwrap();
        let (slot, _) = self
            .ready
            .wait_timeout_while(guard, timeout, |slot| {
                slot.fix.is_none() && slot.agent_present != Some(false)
            })
            .unwrap();
        match (slot.fix, slot.agent_present) {
            (Some(fix), _) => FixWait::Fix(fix),
            (None, Some(false)) => FixWait::NoAgent,
            (None, _) => FixWait::Pending,
        }
    }
}

impl GeoclueMonitor {
    pub fn new(signal_sender: Sender<SignalMessage>, debug_enabled: bool) -> Self {
        let (wake, receiver) = mpmc::bounded(std::num::NonZeroUsize::MIN);
        Self {
            link: Link {
                signal_sender,
                wanted: Arc::new(AtomicBool::new(false)),
                running: Arc::new(AtomicBool::new(false)),
                wake: receiver,
                fix_state: Arc::default(),
                debug_enabled,
            },
            wake,
        }
    }

    /// Start or stop receiving fixes. Repeats are harmless.
    pub fn set_active(&mut self, active: bool) {
        self.link.wanted.store(active, Ordering::SeqCst);
        if active && !self.link.running.swap(true, Ordering::SeqCst) {
            let link = self.link.clone();
            thread::spawn(move || {
                run(&link);
                link.running.store(false, Ordering::SeqCst);
            });
        }
        let _ = self.wake.try_send(());
    }

    /// Wait up to `timeout` for a first fix or for the missing-agent notice,
    /// whichever comes first.
    pub fn wait_for_fix(&self, timeout: Duration) -> FixWait {
        self.link.fix_state.wait(timeout)
    }

    /// The last fix the thread received, from any client it has run.
    pub fn latest_fix(&self) -> Option<Fix> {
        self.link.fix_state.latest_fix()
    }

    /// `None` until the first GetClient returns or the notice fires.
    pub fn agent_present(&self) -> Option<bool> {
        self.link.fix_state.agent_present()
    }

    #[cfg(test)]
    pub(crate) fn store_fix(&self, fix: Fix) {
        self.link.fix_state.store_fix(fix);
    }

    #[cfg(test)]
    pub(crate) fn store_agent(&self, present: bool) {
        self.link.fix_state.store_agent(present);
    }
}

fn run(link: &Link) {
    for attempt in 0..=MAX_RESTARTS {
        match monitor(link) {
            Ok(()) => return,
            Err(e) => {
                log_pipe!();
                log_warning!("GeoClue monitor error: {e}");
                if attempt < MAX_RESTARTS {
                    log_indented!(
                        "Will reconnect to GeoClue (attempt {}/{})",
                        attempt + 1,
                        MAX_RESTARTS
                    );
                    thread::sleep(RESTART_DELAY);
                } else {
                    log_indented!("Automatic location updates will not be available");
                }
            }
        }
    }
}

/// Returns `Ok(())` once the main loop is gone, `Err` when GeoClue did.
fn monitor(link: &Link) -> Result<()> {
    let debug_enabled = link.debug_enabled;
    zbus::block_on(async {
        let runtime = Runtime::<Shared>::current().context("No runtime for the GeoClue monitor")?;
        let connection = Connection::system()
            .await
            .context("Failed to connect to system D-Bus")?;
        let manager = ManagerProxy::new(&connection)
            .await
            .context("Failed to create GeoClue manager proxy")?;

        let agent_missing = Arc::new(AtomicBool::new(false));
        let notice = runtime.spawn("agent notice", {
            let runtime = runtime.clone();
            let agent_missing = agent_missing.clone();
            let link = link.clone();
            async move {
                runtime.sleep(FIRST_FIX_WAIT).await;
                agent_missing.store(true, Ordering::SeqCst);
                log_pipe!();
                log_warning!("GeoClue has no authorization agent for this session");
                log_indented!("Location updates are unavailable until one is running");
                link.fix_state.store_agent(false);
                let _ = link
                    .signal_sender
                    .send(SignalMessage::GeoclueAgent { present: false });
            }
        });
        let client_path = manager
            .get_client()
            .await
            .context("GeoClue did not provide a client")?;
        drop(notice);
        link.fix_state.store_agent(true);
        if agent_missing.load(Ordering::SeqCst) {
            log_pipe!();
            log_info!("GeoClue agent found, automatic location updates available");
            let _ = link
                .signal_sender
                .send(SignalMessage::GeoclueAgent { present: true });
        }

        let client = ClientProxy::builder(&connection)
            .path(client_path)
            .build()
            .await
            .context("Failed to create GeoClue client proxy")?;
        client.set_desktop_id(DESKTOP_ID).await?;
        client.set_requested_accuracy_level(ACCURACY_CITY).await?;
        client
            .set_distance_threshold((LOCATION_CHANGE_FLOOR_KM * 1000.0) as u32)
            .await?;
        let mut updates = client
            .receive_location_updated()
            .await
            .context("Failed to subscribe to GeoClue location updates")?;
        if debug_enabled {
            log_pipe!();
            log_debug!("GeoClue client ready");
        }

        // A restarted client keeps its last location, which falls under the
        // distance threshold, so no LocationUpdated follows the Start. The
        // Location property is read instead.
        let control = runtime.spawn("client control", {
            let client = client.clone();
            let connection = connection.clone();
            let link = link.clone();
            async move {
                let mut active = false;
                let mut stopped_before = false;
                loop {
                    let target = link.wanted.load(Ordering::SeqCst);
                    if target != active {
                        let result = if target {
                            client.start().await
                        } else {
                            client.stop().await
                        };
                        match result {
                            Ok(()) => {
                                stopped_before |= !target;
                                active = target;
                            }
                            Err(e) => {
                                log_pipe!();
                                log_warning!("GeoClue client could not be switched: {e}");
                            }
                        }
                        if active
                            && stopped_before
                            && let Ok(path) = client.location().await
                            && path.as_str() != "/"
                            && let Err(e) = report_fix(&link, &connection, path).await
                        {
                            log_pipe!();
                            log_warning!("Failed to read the GeoClue location: {e}");
                        }
                    }
                    if link.wake.recv().await.is_err() {
                        return;
                    }
                }
            }
        });

        // GeoClue replaces the location object on each update, so a read can
        // miss one that a newer update already retired.
        while let Some(signal) = updates.next().await {
            let args = signal
                .args()
                .context("Failed to parse GeoClue LocationUpdated")?;
            match report_fix(link, &connection, args.new.clone()).await {
                Ok(true) => {}
                Ok(false) => return Ok(()),
                Err(e) => {
                    log_pipe!();
                    log_warning!("Failed to read the GeoClue location: {e}");
                }
            }
        }
        drop(control);
        Err(anyhow::anyhow!("GeoClue location stream ended"))
    })
}

/// Read the location object at `path` and dispatch it. Returns `Ok(false)`
/// once the main loop is gone.
async fn report_fix(link: &Link, connection: &Connection, path: OwnedObjectPath) -> Result<bool> {
    let location = LocationProxy::builder(connection)
        .path(path)
        .build()
        .await?;
    let latitude = location.latitude().await?;
    let longitude = location.longitude().await?;
    if link.debug_enabled {
        log_pipe!();
        log_debug!(
            "GeoClue fix: {} within {} m ({})",
            format_coordinates(latitude, longitude),
            location.accuracy().await? as u64,
            location.description().await?
        );
    }

    let fix = Fix::live(latitude, longitude);
    link.fix_state.store_fix(fix);
    Ok(link
        .signal_sender
        .send(SignalMessage::LocationUpdate(fix))
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHORT: Duration = Duration::from_millis(20);

    #[test]
    fn wait_times_out_as_pending_while_nothing_is_known() {
        let state = FixState::default();
        assert!(matches!(state.wait(SHORT), FixWait::Pending));
    }

    #[test]
    fn wait_returns_the_fix_once_stored() {
        let state = Arc::new(FixState::default());
        let writer = {
            let state = state.clone();
            thread::spawn(move || state.store_fix(Fix::live(35.0, 139.0)))
        };
        let outcome = state.wait(Duration::from_secs(5));
        writer.join().unwrap();
        assert!(
            matches!(outcome, FixWait::Fix(fix) if fix.latitude == 35.0 && fix.longitude == 139.0)
        );
    }

    #[test]
    fn wait_ends_early_when_the_agent_is_missing() {
        let state = Arc::new(FixState::default());
        let writer = {
            let state = state.clone();
            thread::spawn(move || state.store_agent(false))
        };
        let outcome = state.wait(Duration::from_secs(5));
        writer.join().unwrap();
        assert!(matches!(outcome, FixWait::NoAgent));
    }

    #[test]
    fn a_present_agent_alone_keeps_waiting() {
        let state = FixState::default();
        state.store_agent(true);
        assert!(matches!(state.wait(SHORT), FixWait::Pending));
    }

    #[test]
    fn a_fix_wins_over_a_missing_agent() {
        let state = FixState::default();
        state.store_fix(Fix::live(35.0, 139.0));
        state.store_agent(false);
        assert!(matches!(state.wait(SHORT), FixWait::Fix(..)));
    }
}
