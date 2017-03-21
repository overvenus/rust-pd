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
        "http://127.0.0.1:42379".to_owned(),
        "http://127.0.0.1:52379".to_owned(),
        "http://127.0.0.1:62379".to_owned(),
    ];

    let se = Arc::new(Service::new(eps.clone()));
    let lc = Arc::new(Split::new(eps));

    let _server_a = MockServer::run("127.0.0.1:42379", se.clone(), Some(lc.clone()));
    let _server_b = MockServer::run("127.0.0.1:52379", se.clone(), Some(lc.clone()));
    let _server_c = MockServer::run("127.0.0.1:62379", se.clone(), Some(lc.clone()));

    loop {
        thread::park()
    }
}