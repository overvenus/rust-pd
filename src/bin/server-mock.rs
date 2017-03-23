extern crate env_logger;
extern crate pd;

use std::thread;
use std::sync::Arc;

use pd::util::set_exit_hook;
use pd::mock::Server as MockServer;
use pd::mock::case::*;

fn main() {
    env_logger::init().unwrap();
    set_exit_hook();

    let eps = vec![
        "http://127.0.0.1:43079".to_owned(),
        "http://127.0.0.1:53079".to_owned(),
        "http://127.0.0.1:63079".to_owned(),
    ];

    let se = Arc::new(Service::new(eps.clone()));
    let lc = Arc::new(LeaderChange::new(eps.clone()));

    let _server_a = MockServer::run("127.0.0.1:43079", se.clone(), Some(lc.clone()));
    let _server_b = MockServer::run("127.0.0.1:53079", se.clone(), Some(lc.clone()));
    let _server_a = MockServer::run("127.0.0.1:63079", se.clone(), Some(lc.clone()));

    loop {
        thread::park()
    }
}