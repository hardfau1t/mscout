//! This module has functions related to statitics, manually setting them and displaying them.
use crate::{
  listener::{self, ConnType, MP_DESC},
  ROOT_DIR,
};
use id3::{frame::Comment, Tag};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::path;
use std::process::exit;

// #[derive(Debug)]
// enum Operation {
//   Add(u16),
//   Subtract(u16),
//   Reset,
// }

/// stores statistics in the form of played count and skipped count. using these perticular song
/// can be rated.
#[derive(Debug, Deserialize, Serialize)]
pub struct Statistics {
  /// number of times a song is played completely.
  play_cnt: u32,
  /// number of times a song is skipped.
  skip_cnt: u32,
}

impl Statistics {
  /// increments skip count
  pub fn skipped(&mut self) {
    self.skip_cnt += 1;
  }
  /// increments the play count
  pub fn played(&mut self) {
    self.play_cnt += 1;
  }
  /// returns ratings which is a number between 0-10 if there are ratings else None
  pub fn get_ratings(&self) -> f32 {
    (self.play_cnt as f32 / (1 + self.skip_cnt) as f32) * (self.play_cnt + self.skip_cnt) as f32
  }
}

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
            .delete_sticker("song", spath.to_str().unwrap(), MP_DESC) // if the sticker is invalid then remove it.
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

/// extracts the statistics from eyed3 tags(from comments).
/// rel_path : relative path to the song from mpd_directory
pub fn stats_from_tag(rel_path: &std::path::Path) -> Statistics {
  let mut cmt = None;
  let mut spath = path::PathBuf::from(ROOT_DIR.get().expect(
    "statistics to tag requires full path, try to use --socket-file or set root-dir manually",
  ));
  spath.push(rel_path);
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

/// set the statistics to the eyed3 tags(from comments).
/// spath : absolute path to the song.
pub fn stats_to_tag(spath: &std::path::Path, stats: &Statistics) {
  let mut root = path::PathBuf::from(ROOT_DIR.get().expect(
    "statistics to tag requires full path, try to use --socket-file or set root-dir manually",
  ));
  root.push(spath);
  debug!("setting tag to {:#?}", root);
  let mut tag = Tag::read_from_path(&root).unwrap_or_else(|err: id3::Error| match err.kind {
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
    .write_to_path(&root, id3::Version::Id3v24)
    .unwrap_or_else(|err| warn!("failed to write tag {}", err));
}

/// extracts song statistics from id3 metadata or mpd's database based on use-tags flags
pub fn get_stats(
  client: &mut mpd::Client<listener::ConnType>,
  args: &clap::ArgMatches,
  use_tags: bool,
) {
  let mut songs = Vec::new();
  if args.is_present("current") {
    songs.push(path::PathBuf::from(
      client
        .currentsong()
        .unwrap()
        .unwrap_or_else(|| {
          error!("failed to get current song from mpd");
          exit(1);
        })
        .file,
    ));
  } else {
    for user_path in args.values_of("path").unwrap_or_else(|| {
      error!("no song is specified!!");
      exit(1);
    }) {
      songs.push(path::PathBuf::from(user_path));
    }
  };
  for song in songs {
    let rates = if use_tags {
      stats_from_tag(&song)
    } else {
      stats_from_sticker(client, &song)
    };
    if args.is_present("stats") {
      if args.is_present("json") {
        println!("{}", serde_json::to_string(&rates).unwrap());
      } else {
        println!(
          "play count: {}\nskip count {}",
          rates.play_cnt, rates.skip_cnt
        );
      }
    } else {
      println!("ratings: {}", rates.get_ratings());
    }
  }
}

/// sets the stats of a custom user stats
pub fn set_stats(
  client: &mut mpd::Client<listener::ConnType>,
  subc: &clap::ArgMatches,
  use_tags: bool,
) {
    // get the song to set stats, if current is given then get it from mpd or else from path
    // argument
  let song_file = if subc.is_present("current") {
    path::PathBuf::from(
      client
        .currentsong()
        .unwrap()
        .unwrap_or_else(|| {
          error!("failed to get current song from mpd");
          exit(1);
        })
        .file,
    )
  } else {
    path::PathBuf::from(subc.value_of("path").unwrap_or_else(|| {
      error!("missing song path this should be checked during init. please report an issue");
      exit(1)
    }))
  };
  let stat = serde_json::from_str::<Statistics>(subc.value_of("stats").unwrap()).unwrap_or_else(|err|{
      match err.classify() {
          serde_json::error::Category::Syntax => {
              error!("invalid json syntax at {}:{}, please use -Sh for example", err.line(), err.column());
          },
          serde_json::error::Category::Data => {
              error!("invalid input data format at {}:{} , please use -Sh for example", err.line(), err.column());
          },
          _ => {
              error!("invalid input stats, please use -Sh for example");
          },
      }
      exit(1);
  });
  if use_tags{
      stats_to_tag(&song_file, &stat);
  }else{
      stats_to_sticker(client, &song_file, &stat);
  }
}
