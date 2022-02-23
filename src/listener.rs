//! This module handles functions relating listening to events from mpd and setting stats to a song based on the
//! events
use crate::{stats, ConnType};
// logging macros no need to warn if unused
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use mpd::{idle::Subsystem, Idle};
use notify_rust::{Notification, Urgency};
use signal_hook::{consts::TERM_SIGNALS, iterator::Signals};
use std::path::PathBuf;
use std::process::exit;
use std::time::Instant;

/// alternate to mpd::song::Id with implementation of required traits
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Id(u32);

impl From<mpd::song::QueuePlace> for Id {
    fn from(q: mpd::song::QueuePlace) -> Self {
        Self(q.id.0)
    }
}
impl TryFrom<Option<mpd::song::QueuePlace>> for Id {
    type Error = ();
    fn try_from(value: Option<mpd::song::QueuePlace>) -> Result<Self, Self::Error> {
        if let Some(q) = value {
            Ok(Self(q.id.0))
        } else {
            Err(())
        }
    }
}
impl From<Id> for mpd::song::Id {
    fn from(val: Id) -> Self {
        mpd::song::Id(val.0)
    }
}

/// specifies last action of the mpd event. It is different from mpd events that mpd events only
/// mentions subsystems which can't be used to determine the status without some calculations
#[derive(Debug)]
enum Action {
    /// last event skipped the playing song.
    Skipped(Id),
    /// last event successfully played complete song
    Played(Id),
    /// doesn't matter if other type of event has occurred
    WhoCares,
}

/// This represents the state of the mpd. This will act as state machine
#[derive(Debug)]
enum ListenerState {
    /// mpd is Currently Playing.
    Playing {
        /// curr indicates id of current song
        curr: (Id, u64),
        /// next indicates id of next song
        next: Option<Id>,
        /// start time of playing
        st: Instant,
    },
    /// mpd is in Paused/Stopped state.
    Paused {
        /// curr indicates id of current song
        curr: Id,
        /// next indicates id of next song
        next: Option<Id>,
    },
    /// mpd disconnected or there are no songs in the queue/Currently there is no song
    Invalid,
}

impl ListenerState {
    /// takes mpd current status and returns Action based on the current state.
    fn handle_event(&mut self, status: mpd::Status) -> Action {
        // here self will be the last state and current state will be in status,
        // so if curr is specified then its last song.
        match *self {
            ListenerState::Playing { curr, next, st } => match status.state {
                mpd::State::Stop => {
                    info!("{:?} to {:?}", self, status.state);
                    *self = ListenerState::Invalid;
                    Action::WhoCares
                }
                mpd::State::Pause => {
                    info!("{:?} to {:?}", self, status.state);
                    let mut ret = Action::WhoCares;
                    if let Some(s) = next {
                        // if single is set then it is possible that state to change from play to paused and song changed
                        if s.0 == status.song.unwrap().id.0 {
                            if status.single && st.elapsed().as_secs() + 1 > curr.1 {
                                // +1 so to eliminate delay introduced by computation, etc
                                ret = Action::Played(curr.0);
                            } else {
                                error!("next song is played when the new state is pause");
                                debug!("current state: {self:?}, new status: {status:?}");
                            }
                        }
                    }
                    if st.elapsed().as_secs() + 1 > curr.1 {
                        // +1 so to eliminate delay introduced by computation, etc
                        // if only one song is there in the playlist it is possible that play->pause after completely played
                        ret = Action::Played(curr.0);
                    }
                    *self = ListenerState::Paused {
                        curr: status.song.try_into().unwrap(),
                        next: status.nextsong.map(|s| s.into()),
                    };
                    ret
                }
                mpd::State::Play => {
                    info!("{:?} to {:?}", self, status.state);
                    let mut ret = Action::WhoCares;
                    // if the current song is same as previous and repeat is enabled then it is possibl that song is played
                    if curr.0 == status.song.unwrap().into()
                        && status.repeat
                        && st.elapsed().as_secs() + 1 >= curr.1
                    // +1 to cover some timing errors
                    {
                        ret = Action::Played(curr.0);
                    } else if let Some(n) = next {
                        // if the currently playing song is next of previous then either it is skipped or played.
                        if n == status.song.unwrap().into() {
                            debug!(
                                "next {:?}, curr.time:{}, instant : {:?}, and status {:?}",
                                n, curr.1, st, status
                            );
                            if st.elapsed().as_secs() + 1 >= curr.1 {
                                // +1 so that it will cover if some errors
                                ret = Action::Played(curr.0);
                            } else {
                                ret = Action::Skipped(curr.0);
                            }
                        }
                    }
                    *self = ListenerState::Playing {
                        curr: (
                            status.song.try_into().unwrap(),
                            (status.duration.unwrap() - status.elapsed.unwrap()).as_secs(),
                        ),
                        next: status.nextsong.map(|s| s.into()),
                        st: Instant::now(),
                    };
                    debug!(
                        "updating listener {:?}, with elapsed {:?}",
                        self, status.elapsed
                    );
                    ret
                }
            },
            // check if the next is currrent playing song then it is skipped. else just update the state
            ListenerState::Paused { curr, next } => match status.state {
                mpd::State::Stop => {
                    info!("{:?} to {:?}", self, status.state);
                    *self = ListenerState::Invalid;
                    Action::WhoCares
                }
                // it doesn't matter whether it is playing or Paused if the next song is in queue then it is skipped else sequence changed
                mpd::State::Play | mpd::State::Pause => {
                    info!("{:?} to {:?}", self, status.state);
                    *self = ListenerState::Playing {
                        curr: (
                            status
                                .song
                                .expect("report!!! This shouldn't be None")
                                .into(),
                            (status.duration.expect("status doesn't contains time")
                                - status.elapsed.unwrap())
                            .as_secs(),
                        ),
                        next: status.nextsong.map(|s| s.into()),
                        st: Instant::now(), // if it started from pause then add the elapsed time
                    };
                    debug!(
                        "updating listener {:?}, with elapsed {:?}",
                        self, status.elapsed
                    );
                    if let Some(s) = next {
                        if s.0 == status.song.expect("report!!! This should not be NULL").id.0 {
                            return Action::Skipped(curr);
                        }
                    };
                    Action::WhoCares
                }
            },
            // if last state is invalid then whatever happened doesn't matter just update the state and continue
            ListenerState::Invalid => {
                info!("{:?} to {:?}", self, status.state);
                match status.state {
                    mpd::State::Play => {
                        *self = ListenerState::Playing {
                            curr: (
                                status
                                    .song
                                    .expect("report!!! This shouldn't be None")
                                    .into(),
                                (status.duration.expect("status time is None")
                                    - status.elapsed.unwrap())
                                .as_secs(),
                            ),
                            next: status.nextsong.map(|s| s.into()),
                            st: Instant::now(),
                        };
                        debug!(
                            "updating listener {:?}, with elapsed {:?}",
                            self, status.elapsed
                        );
                    }
                    mpd::State::Pause => {
                        warn!(
                            "report!!! This should be unreachable, may lead to undefined behavior"
                        );
                        *self = ListenerState::Paused {
                            curr: status
                                .song
                                .try_into()
                                .expect("report!!! This shouldn't be None"),
                            next: status.nextsong.map(|s| s.into()),
                        }
                    }
                    mpd::State::Stop => (),
                }
                Action::WhoCares
            }
        }
    }
    /// takes current status of mpd and initiates respective state.
    fn with_status(status: mpd::Status) -> Self {
        match status.state {
            mpd::status::State::Stop => Self::Invalid,
            mpd::status::State::Pause => Self::Paused {
                curr: status.song.unwrap().into(),
                next: status.nextsong.map(|s| s.into()),
            },
            mpd::status::State::Play => Self::Playing {
                curr: (
                    status.song.try_into().unwrap(),
                    status
                        .duration
                        .expect("status should Contain time")
                        .as_secs(),
                ),
                next: status.nextsong.map(|s| s.into()),
                st: Instant::now(),
            },
        }
    }
}

/// checks if any other instance of listener is running, if not then create flag file indicating that a listener is running
fn init_listener(notif: &mut notify_rust::Notification) {
    let lock_file = "/tmp/mp_rater.lck";
    if let Err(err) = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(lock_file)
    {
        match err.kind() {
            std::io::ErrorKind::AlreadyExists => {
                println!(
                    "Already another instance is running!!!\n\
                    kill that instance to start another.\n\
                    if not running then remove /tmp/mp_rater.lck file"
                );
            }
            _ => error!("failed to check for instance {:?}", err),
        }
        notif.body("Failed to start mp_rater may be already started").show().ok();
        exit(1);
    }
    // initialize signal handler
    let mut signals = Signals::new(TERM_SIGNALS).expect("Couldn't register signals");
    std::thread::spawn(move || {
        for sig in signals.forever() {
            Notification::new()
                .summary("mp_rater")
                .timeout(10000)
                .urgency(Urgency::Low)
                .icon("/usr/share/icons/Adwaita/scalable/devices/media-optical-dvd-symbolic.svg")
                .body("stopping listener")
                .show()
                .ok();
            info!("recieved a signal {:?}", sig);
            std::fs::remove_file(lock_file).expect("lock File remove failed");
            info!("Cleanup done");
            exit(0);
        }
    });
}
/// listens to mpd events sets the statistics for the song
/// use_tags: if its true then eyed3 tags will be used else mpd stickers are used to store stats
pub fn listen(client: &mut mpd::Client<ConnType>, _subc: &clap::ArgMatches, use_tags: bool) -> ! {
    let mut notif = Notification::new();
    notif
        .summary("mp_rater")
        .timeout(10000)
        .urgency(Urgency::Low)
        .icon("/usr/share/icons/Adwaita/scalable/devices/media-optical-dvd-symbolic.svg");
    let mut state = ListenerState::with_status(client.status().unwrap());
    init_listener(&mut notif);
    notif.body("Listener started").show().ok();
    loop {
        if let Ok(sub_systems) = client.wait(&[]) {
            // sub systems which caused the thread to wake up
            for system in sub_systems {
                match system {
                    Subsystem::Player => {
                        // let action = eval_player_events(client, &last_state, &start_time, &timer);
                        match state.handle_event(client.status().unwrap()) {
                            Action::WhoCares => debug!("Someone can't sleep peacefully"),
                            Action::Played(id) => {
                                let song_path = PathBuf::from(
                                    client
                                        .playlistid(id.into())
                                        .unwrap()
                                        .expect("may be consume enabled?")
                                        .file,
                                );
                                notif
                                    .body(
                                        format!(
                                            "Played: {}",
                                            &song_path
                                                .file_name()
                                                .map_or(song_path.to_str(), |pth| pth.to_str())
                                                .unwrap()
                                        )
                                        .as_ref(),
                                    )
                                    .show()
                                    .ok();
                                // TODO: optimise this in better way
                                let mut stats = if use_tags {
                                    stats::stats_from_tag(&song_path)
                                } else {
                                    stats::stats_from_sticker(client, &song_path)
                                }
                                .unwrap_or_default();
                                stats.played();
                                match if use_tags {
                                    stats::stats_to_tag(&song_path, &stats)
                                } else {
                                    stats::stats_to_sticker(client, &song_path, &stats)
                                } {
                                    Ok(_) => (),
                                    Err(_) => error!("skipped rating: Couldn't set the stats"),
                                }
                            }
                            Action::Skipped(id) => {
                                let song_path = PathBuf::from(
                                    client
                                        .playlistid(id.into())
                                        .unwrap()
                                        .expect("check if consume is enabled")
                                        .file,
                                );
                                notif
                                    .body(
                                        format!(
                                            "Skipped: {}",
                                            &song_path
                                                .file_name()
                                                .map_or(song_path.to_str(), |pth| pth.to_str())
                                                .unwrap()
                                        )
                                        .as_ref(),
                                    )
                                    .show()
                                    .ok();
                                // TODO: optimise this in better way
                                let mut stats = if use_tags {
                                    stats::stats_from_tag(&song_path)
                                } else {
                                    stats::stats_from_sticker(client, &song_path)
                                }
                                .unwrap_or_default();
                                stats.skipped();
                                match if use_tags {
                                    stats::stats_to_tag(&song_path, &stats)
                                } else {
                                    stats::stats_to_sticker(client, &song_path, &stats)
                                } {
                                    Ok(_) => (),
                                    Err(_) => error!("skipped rating: Couldn't set the stats"),
                                }
                            }
                        };
                    }
                    _ => trace!("ignoring event {}", system),
                }
            }
        }
    }
}
