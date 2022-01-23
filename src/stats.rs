//! This module has functions related to statitics, manually setting them and displaying them.
use crate::listener;

// #[derive(Debug)]
// enum Operation {
//   Add(u16),
//   Subtract(u16),
//   Reset,
// }

/// manually sets the user given stats to a song
pub fn get_stats(_client: &mpd::Client<listener::ConnType>, _subc: &clap::ArgMatches, _use_tags: bool) {
  todo!()
}

/// prints the stats of a custom user stats
pub fn set_stats(_client: &mpd::Client<listener::ConnType>, _subc: &clap::ArgMatches, _use_tags: bool) {
  todo!()
}
