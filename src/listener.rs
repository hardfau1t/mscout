//! This module handles functions relating listening to events from mpd and setting stats to a song based on the
//! events
use crate::stats;
use log::{debug, trace, error};
use mpd::{idle::Subsystem, Idle};
use notify_rust::{Notification, Urgency};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::exit;
use std::time::{Duration, Instant};

/// header name which will be used on either mpd's sticker database or tags for identifications
pub const MP_DESC: &str = "mp_rater";

/// defines connection type for the mpd.
#[derive(Debug)]
pub enum ConnType {
  /// connects through linux socket file
  Stream(std::os::unix::net::UnixStream),
  /// connects using normal network sockets
  Socket(std::net::TcpStream),
}

impl Read for ConnType {
  fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
    match self {
      ConnType::Stream(s) => s.read(buf),
      ConnType::Socket(s) => s.read(buf),
    }
  }
}

impl Write for ConnType {
  fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
    match self {
      ConnType::Stream(s) => s.write(buf),
      ConnType::Socket(s) => s.write(buf),
    }
  }

  fn flush(&mut self) -> std::io::Result<()> {
    match self {
      ConnType::Stream(s) => s.flush(),
      ConnType::Socket(s) => s.flush(),
    }
  }
}

/// specifies last action of the mpd event. It is different from mpd events that mpd events only
/// mentions subsystems which can't be used to determine the status without some calculations
#[derive(Debug)]
enum Action {
  /// last event skipped the playing song.
  Skipped,
  /// last event successfully played complete song
  Played,
  /// doesn't matter if other type of event has occurred
  WhoCares,
}

/// by comparing last state to the current state this fn will determine whether an event skipped a
/// song or fully played based on that it returns action type.
/// Note: only skip to next song is counted, not if previous song is played or some other random
/// song in the sequence is played.
fn eval_player_events(
  client: &mut mpd::Client<ConnType>,
  last_state: &mpd::Status,
  start_time: &std::time::Duration,
  timer: &std::time::Instant,
) -> Action {
  trace!("handling player_event()");
  let curr_state = client.status().unwrap_or_else(|err|{error!("may be mpd connection failed"); exit(1)});
  // if state is paused or stopped then no need to rate. if last state is paused then its
  // just start so no need to rate either
  if curr_state.state == mpd::status::State::Stop
    || curr_state.state == mpd::status::State::Pause
    || last_state.state == mpd::status::State::Stop
  {
    debug!("ignoring player due to {:?}", curr_state.state);
    return Action::WhoCares;
  }
  // if its paused and resume then no need to rate. if paused and now its next song then its
  // been skipped
  if last_state.state == mpd::status::State::Pause {
    if last_state.song == curr_state.song {
      debug!("resumed from pause");
      Action::WhoCares
    } else if last_state.nextsong == curr_state.song {
      Action::Skipped
    } else {
      debug!("may be sequence change");
      Action::WhoCares
    }
  } else {
    // last state is playing and current is also playing and last_state next song is
    // current song then either its skipped or played completely
    if last_state.song == curr_state.song {
      debug!("probably seeked");
      Action::WhoCares
    } else if last_state.nextsong == curr_state.song {
      let elapsed = last_state.elapsed.unwrap().to_std().unwrap();
      // +1 second is kept to compunsate computation delays or some other wierdos
      let elapsed_time = elapsed + timer.elapsed() - *start_time + Duration::new(1, 0);
      debug!(
        "elapsed {:?}, timer_elapsed {:?}, start_time {:?}, duration {:?}, sum_played {:?}",
        elapsed,
        timer.elapsed(),
        start_time,
        last_state.duration,
        elapsed_time
      );
      if elapsed_time >= last_state.duration.unwrap().to_std().unwrap() {
        Action::Played
      } else {
        Action::Skipped
      }
    } else {
      debug!(
        "may be sequence changed? report if not!!!\nlast_state {:?}\ncurr_state {:?}",
        last_state, curr_state
      );
      Action::WhoCares
    }
  }
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
  let timer = Instant::now();
  // if stickers are used then only relative path provided by mpd is used so empty buf is
  // initialized
  loop {
    let spath = PathBuf::from(client.currentsong().unwrap().unwrap().file);
    let last_state = client.status().unwrap();
    let start_time = timer.elapsed();
    // TODO: remove unwrap and add a closure to wait for the state change, may be if state is stopped or no song is in queue then this will error

    if let Ok(sub_systems) = client.wait(&[]) {
      // sub systems which caused the thread to wake up
      for system in sub_systems {
        match system {
          Subsystem::Player => {
            let action = eval_player_events(client, &last_state, &start_time, &timer);
            match action {
              Action::WhoCares => debug!("Someone can't sleep peacefully"),
              Action::Played => {
                notif
                  .clone()
                  .body(
                    format!(
                      "Played: {}",
                      &spath
                        .file_name()
                        .map_or(spath.to_str(), |pth| pth.to_str())
                        .unwrap()
                    )
                    .as_ref(),
                  )
                  .show()
                  .ok();
                // TODO: optimise this in better way
                let mut stats = if use_tags {
                  stats::stats_from_tag(&spath).unwrap_or_else(|err|{
                      todo!()
                  })
                } else {
                  stats::stats_from_sticker(client, &spath)
                };
                stats.played();
                if use_tags {
                  stats::stats_to_tag(&spath, &stats)
                } else {
                  stats::stats_to_sticker(client, &spath, &stats)
                };
              }
              Action::Skipped => {
                notif
                  .clone()
                  .body(
                    format!(
                      "Skipped: {}",
                      &spath
                        .file_name()
                        .map_or(spath.to_str(), |pth| pth.to_str())
                        .unwrap()
                    )
                    .as_ref(),
                  )
                  .show()
                  .ok();
                // TODO: optimise this in better way
                let mut stats = if use_tags {
                  stats::stats_from_tag(&spath).unwrap_or_else(|err|{
                  todo!()
                  })
                } else {
                  stats::stats_from_sticker(client, &spath)
                };
                stats.skipped();
                if use_tags {
                  stats::stats_to_tag(&spath, &stats)
                } else {
                  stats::stats_to_sticker(client, &spath, &stats)
                };
              }
            };
          }
          _ => trace!("ignoring event {}", system),
        }
      }
    }
  }
}
