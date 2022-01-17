use id3::{frame::Comment, Tag};
use log::{debug, info, trace, warn};
use mpd::{idle::Subsystem, status::State, Idle, Song, Status};
use serde::{Deserialize, Serialize};
use socket2::{Domain, SockAddr, Socket, Type};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const MP_DESC: &str = "mp_rater";

#[derive(Debug)]
pub enum ConnectionType {
  UnixSock(String),
  NetSock(String),
}

#[derive(Debug)]
enum Action {
  Skipped,
  Played,
}

#[derive(Debug, Deserialize, Serialize)]
struct Statistics {
  play_cnt: u16,
  skip_cnt: u16,
}

#[derive(Debug)]
enum Operation {
  Add(u16),
  Subtract(u16),
  Reset,
}

pub struct Listener {
  client: mpd::Client<Socket>,
  last_state: Status,
  last_song: Option<Song>,
  timer: Instant,
  start_time: Duration,
  dir: std::path::PathBuf,
}

impl Listener {
  pub fn new(con_address: ConnectionType) -> Result<Self, mpd::error::Error> {
    let mut client = match con_address {
      ConnectionType::UnixSock(address) => {
        let addr = SockAddr::unix(address).unwrap();
        let sock = socket2::Socket::new(Domain::UNIX, Type::STREAM, None).unwrap();
        sock.connect(&addr).unwrap();
        mpd::Client::new(sock)?
      }
      ConnectionType::NetSock(_) => unimplemented!(),
    };
    let status = client.status()?;
    let timer = Instant::now();
    let last_song = client.currentsong().unwrap();
    let dir: PathBuf = PathBuf::from(client.music_directory().unwrap());
    Ok(Listener {
      client,
      last_state: status,
      timer,
      start_time: timer.elapsed(),
      last_song,
      dir,
    })
  }
  fn add_sticker(&self, sticker_type: Action, op: Operation) {
    info!(
      "{:?} to sticker {:?} to {:?} ",
      op, sticker_type, self.last_state.song
    );
  }
  fn add_tag(&self, tag_type: Action, op: Operation) {
    info!(
      "{:?} to tag {:?} to {:?} ",
      op, tag_type, self.last_state.song
    );
    let mut cmt = None;
    let mut spath = self.dir.clone();
    spath.push(self.last_song.as_ref().unwrap().file.clone());
    debug!("path is {:#?}", spath);
    let mut tag = Tag::read_from_path(&spath).unwrap();
    for com in tag.comments() {
      debug!("available comments are {:?}", cmt);
      if com.description == MP_DESC {
        cmt = Some(com.clone());
        break;
      }
    }
    // if the file has ratings comment then modify it, else create fresh one with 0 0
    let mut ratings = cmt.map_or(
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
        let desc = comment.description;
        tag.remove_comment(Some(&desc), None);
        rating
      },
    );
    match op {
      Operation::Add(n) => match tag_type {
        Action::Skipped => ratings.skip_cnt += n,
        Action::Played => ratings.play_cnt += n,
      },
      Operation::Subtract(n) => match tag_type {
        Action::Skipped => ratings.skip_cnt = ratings.skip_cnt.saturating_sub(n),
        Action::Played => ratings.play_cnt = ratings.play_cnt.saturating_sub(n),
      },
      Operation::Reset => match tag_type {
        Action::Skipped => ratings.skip_cnt = 0,
        Action::Played => ratings.play_cnt = 0,
      },
    }
    let comment: Comment = Comment {
      lang: "eng".to_string(),
      description: MP_DESC.to_string(),
      text: serde_json::to_string(&ratings).expect("couldn't convert ratings  to json"),
    };
    info!("attaching tag comment {:?}", comment);
    tag.add_comment(comment);
    tag
      .write_to_path(&spath, id3::Version::Id3v24)
      .unwrap_or_else(|err| warn!("failed to write tag {}", err));
  }

  fn player_event(&mut self) {
    trace!("handling player_event()");
    let status = self.client.status().unwrap();
    // if state is paused or stopped then no need to rate. if last state is paused then its
    // just start so no need to rate either
    if status.state == State::Stop
      || status.state == State::Pause
      || self.last_state.state == State::Stop
    {
      debug!("ignoring player due to {:?}", status.state);
      return;
    }
    // if its paused and resume then no need to rate. if paused and now its next song then its
    // been skipped
    if self.last_state.state == State::Pause {
      if self.last_state.song == status.song {
        debug!("resumed from pause");
      } else if self.last_state.nextsong == status.song {
        self.add_sticker(Action::Skipped, Operation::Add(1));
      }
    } else {
      // last state is playing and current is also playing and last_state next song is
      // current song then either its skipped or played completely
      if self.last_state.nextsong == status.song {
        if let Some(time_elapsed) = self.last_state.elapsed {
          if let Ok(elapsed) = time_elapsed.to_std() {
            // +1 second is kept to compunsate
            let elapsed_time =
              elapsed + self.timer.elapsed() - self.start_time + Duration::new(1, 0);
            debug!(
              "elapsed {:?}, timer_elapsed {:?}, start_time {:?}, duration {:?}, sum_played {:?}",
              elapsed,
              self.timer.elapsed(),
              self.start_time,
              self.last_state.duration,
              elapsed_time
            );
            if elapsed_time >= self.last_state.duration.unwrap().to_std().unwrap() {
              self.add_tag(Action::Played, Operation::Add(1));
            } else {
              self.add_tag(Action::Skipped, Operation::Add(1));
            }
          }
        }
        debug!("last_state elapsed is {:?}", self.last_state.elapsed);
      } else {
        debug!("probably changed the sequence");
      }
    }
  }
  pub fn listen(&mut self) -> ! {
    loop {
      self.last_state = self.client.status().unwrap();
      self.last_song = self.client.currentsong().unwrap();
      self.start_time = self.timer.elapsed();
      if let Ok(sub_systems) = self.client.wait(&[]) {
        for system in sub_systems {
          match system {
            Subsystem::Player => self.player_event(),
            _ => trace!("ignoring event {}", system),
          }
        }
      }
    }
  }
}
