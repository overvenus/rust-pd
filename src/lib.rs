#![cfg_attr(feature="dev", feature(plugin))]
#![cfg_attr(feature="dev", plugin(clippy))]

extern crate kvproto;
extern crate grpc;
extern crate protobuf;
extern crate futures;
extern crate futures_cpupool;
extern crate backtrace;
#[macro_use]
extern crate log;
extern crate url;
#[macro_use]
extern crate lazy_static;
extern crate rand;
#[macro_use]
extern crate prometheus;
#[macro_use]
extern crate quick_error;

#[macro_use]
pub mod util;

pub mod mock;

pub mod client;

pub use self::client::{PdClient, RpcClient, validate_endpoints};
