#[macro_use]
extern crate log;
extern crate env_logger;
extern crate pd;
extern crate futures;
extern crate kvproto;

use std::thread;
use std::sync::Arc;
use std::time::{Instant, Duration};

use kvproto::metapb;

use futures::Future;
use futures::future;

use pd::util::set_exit_hook;
use pd::client::RpcClient;

const EPS: [&'static str; 3] =
    ["http://127.0.0.1:43079", "http://127.0.0.1:53079", "http://127.0.0.1:63079"];

fn main() {
    use pd::client::AsyncPdClient;

    env_logger::init().unwrap();
    set_exit_hook();

    thread::spawn(setup);
    thread::sleep(Duration::from_secs(1));

    let client = RpcClient::new(EPS[0]).unwrap();

    bootstrap(&client);

    let region = client.get_region_by_id(1);
    let f = region.then(|res| {
        match res {
            Ok(resp) => println!("{:?}", resp),
            Err(err) => println!("{:?}", err),
        }
        future::ok(())
    });

    let start = Instant::now();
    client.spawn(f);
    println!("spawn {:?}", start.elapsed());

    thread::sleep(Duration::from_secs(2));
}

fn setup() {
    use pd::mock::Server as MockServer;
    use pd::mock::case::*;

    let eps: Vec<_> = EPS.iter().map(|ep| ep.to_string()).collect();

    let se = Arc::new(Service::new(eps.clone()));

    let _server_a = MockServer::run("127.0.0.1:43079", se.clone(), Some(se.clone()));
    let _server_b = MockServer::run("127.0.0.1:53079", se.clone(), Some(se.clone()));
    let _server_a = MockServer::run("127.0.0.1:63079", se.clone(), Some(se.clone()));

    loop {
        thread::park()
    }
}

fn bootstrap(client: &RpcClient) {
    use pd::client::PdClient;

    assert_ne!(client.get_cluster_id().unwrap(), 0);

    let store_id = client.alloc_id().unwrap();
    let mut store = metapb::Store::new();
    store.set_id(store_id);
    debug!("bootstrap store {:?}", store);

    let peer_id = client.alloc_id().unwrap();
    let mut peer = metapb::Peer::new();
    peer.set_id(peer_id);
    peer.set_store_id(store_id);

    let region_id = client.alloc_id().unwrap();
    let mut region = metapb::Region::new();
    region.set_id(region_id);
    region.mut_peers().push(peer.clone());
    debug!("bootstrap region {:?}", region);

    client.bootstrap_cluster(store.clone(), region.clone()).unwrap();
    assert_eq!(client.is_cluster_bootstrapped().unwrap(), true);

    let tmp_store = client.get_store(store_id).unwrap();
    assert_eq!(tmp_store.get_id(), store.get_id());

    let tmp_region = client.get_region_by_id(region_id).unwrap().unwrap();
    assert_eq!(tmp_region.get_id(), region.get_id());
}
