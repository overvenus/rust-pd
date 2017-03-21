extern crate env_logger;
extern crate pd;

use std::thread;
use std::sync::Arc;

use pd::util::set_exit_hook;
use pd::mock::Server as MockServer;
use pd::mock::case::LeaderChange;

fn main() {
    env_logger::init().unwrap();
    // set_exit_hook();

    let lc = LeaderChange::new(vec![
        "http://127.0.0.1:42379".to_owned(),
        "http://127.0.0.1:52379".to_owned(),
        "http://127.0.0.1:62379".to_owned(),
    ]);
    let lc = Arc::new(lc);

    let _server_a = MockServer::run("127.0.0.1:42379", lc.clone());
    let _server_b = MockServer::run("127.0.0.1:52379", lc.clone());
    let _server_c = MockServer::run("127.0.0.1:62379", lc.clone());

    loop {
        thread::park()
    }
}