use id3::{frame::Comment, Tag};
use log::{debug, error, info, trace, warn};
use mpd::{idle::Subsystem, status::State, Idle};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::exit;
use std::time::{Duration, Instant};

/// header name which will be used on either mpd's sticker database or tags for identifications
const MP_DESC: &str = "mp_rater";

#[derive(Debug)]
pub enum ConnType {
  Stream(std::os::unix::net::UnixStream),
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

#[derive(Debug)]
pub enum Action {
  Skipped,
  Played,
  WhoCares,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Statistics {
  play_cnt: u16,
  skip_cnt: u16,
}

// #[derive(Debug)]
// enum Operation {
//   Add(u16),
//   Subtract(u16),
//   Reset,
// }

/// gets the stats from mpd sticker database.
/// where spath is the path to the song relative to mpd's directory
pub fn stats_from_sticker(
  client: &mut mpd::Client<ConnType>,
  spath: &std::path::Path,
) -> Statistics {
  info!("getting stats from  mpd database for {:?}", spath);
  // get the stats from sticker, if not found then return 0,0
  client
    .sticker("song", spath.to_str().unwrap(), MP_DESC)
    .map_or(
      Statistics {
        play_cnt: 0,
        skip_cnt: 0,
      },
      |sticker| {
        serde_json::from_str(&sticker).unwrap_or_else(|err| {
          warn!("couldn't parse sticker: {:?}", err);
          client
            .delete_sticker("song", spath.to_str().unwrap(), MP_DESC)
            .unwrap_or_else(|err| warn!("failed to delete sticker {:?}", err));
          Statistics {
            play_cnt: 0,
            skip_cnt: 0,
          }
        })
      },
    )
}

/// set the stats to mpd sticker database.
/// where spath is the path to the song relative to mpd's directory
pub fn stats_to_sticker(
  client: &mut mpd::Client<ConnType>,
  spath: &std::path::Path,
  stats: &Statistics,
) {
  info!("setting stats {:?} to mpd database for {:?}", stats, spath);
  client
    .set_sticker(
      "song",
      spath.to_str().unwrap(),
      MP_DESC,
      &serde_json::to_string(stats).expect("Couldn't dump stats to json"),
    )
    .expect("Couldn't dump to mpd  database");
}

pub fn get_from_tag(spath: &std::path::Path) -> Statistics {
  let mut cmt = None;
  debug!("songs full path is {:#?}", spath);
  let tag = Tag::read_from_path(&spath).unwrap_or_else(|err: id3::Error| match err.kind {
    id3::ErrorKind::NoTag => {
      warn!("no tag found creating a new id3 tag");
      Tag::new()
    }
    _ => {
      error!(" error while opening tag {:?}", err.description);
      exit(1)
    }
  });
  for com in tag.comments() {
    debug!("available comments are {:?}", com);
    if com.description == MP_DESC {
      cmt = Some(com.clone());
      break;
    }
  }
  // if the file has ratings comment then modify it, else create fresh one with 0 0
  cmt.map_or(
    Statistics {
      play_cnt: 0,
      skip_cnt: 0,
    },
    |comment| {
      let rating: Statistics = serde_json::from_str(&comment.text).unwrap_or_else(|err| {
        warn!(
          "err {} invalid json text for rating comment {}",
          err, comment.text
        );
        Statistics {
          play_cnt: 0,
          skip_cnt: 0,
        }
      });
      rating
    },
  )
}

pub fn set_to_tag(spath: &std::path::Path, stats: &Statistics) {
  debug!("setting tag to {:#?}", spath);
  let mut tag = Tag::read_from_path(&spath).unwrap_or_else(|err: id3::Error| match err.kind {
    id3::ErrorKind::NoTag => {
      warn!("no tag found creating a new id3 tag");
      Tag::new()
    }
    _ => {
      error!(" error while opening tag {:?}", err.description);
      exit(1)
    }
  });
  let comment: Comment = Comment {
    lang: "eng".to_string(),
    description: MP_DESC.to_string(),
    text: serde_json::to_string(stats).expect("couldn't convert ratings  to json"),
  };
  info!("attaching tag comment {:?}", comment);
  tag.add_comment(comment);
  tag
    .write_to_path(&spath, id3::Version::Id3v24)
    .unwrap_or_else(|err| warn!("failed to write tag {}", err));
}

fn eval_player_events(
  client: &mut mpd::Client<ConnType>,
  last_state: &mpd::Status,
  start_time: &std::time::Duration,
  timer: &std::time::Instant,
) -> Action {
  trace!("handling player_event()");
  let curr_state = client.status().unwrap();
  // if state is paused or stopped then no need to rate. if last state is paused then its
  // just start so no need to rate either
  if curr_state.state == State::Stop
    || curr_state.state == State::Pause
    || last_state.state == State::Stop
  {
    debug!("ignoring player due to {:?}", curr_state.state);
    return Action::WhoCares;
  }
  // if its paused and resume then no need to rate. if paused and now its next song then its
  // been skipped
  if last_state.state == State::Pause {
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
pub fn listen(client: &mut mpd::Client<ConnType>, _subc: &clap::ArgMatches, use_tags: bool) -> ! {
  let timer = Instant::now();
  let root_dir = if use_tags {
    PathBuf::from(client.music_directory().unwrap())
  } else {
    std::path::PathBuf::new()
  };
  loop {
    let mut spath = root_dir.clone();
    spath.push(client.currentsong().unwrap().unwrap().file);
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
                // TODO: optimise this in better way
                let mut stats = if use_tags {
                  get_from_tag(&spath)
                } else {
                  stats_from_sticker(client, &spath)
                };
                stats.play_cnt += 1;
                if use_tags {
                  set_to_tag(&spath, &stats)
                } else {
                  stats_to_sticker(client, &spath, &stats)
                };
              }
              Action::Skipped => {
                // TODO: optimise this in better way
                let mut stats = if use_tags {
                  get_from_tag(&spath)
                } else {
                  stats_from_sticker(client, &spath)
                };
                stats.skip_cnt += 1;
                if use_tags {
                  set_to_tag(&spath, &stats)
                } else {
                  stats_to_sticker(client, &spath, &stats)
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
