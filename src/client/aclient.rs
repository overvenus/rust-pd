// Copyright 2016 PingCAP, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// See the License for the specific language governing permissions and
// limitations under the License.

use std::fmt;
use std::result;
use std::thread;
use std::sync::RwLock;
use std::time::Duration;
use std::collections::HashSet;

use grpc;
use grpc::futures_grpc::GrpcFutureSend;

use protobuf::RepeatedField;

use url::Url;

use rand::{self, Rng};

use kvproto::{metapb, pdpb};
use kvproto::pdpb_grpc::{self, PD};
use kvproto::pdpb_grpc::{PDAsync, PDAsyncClient};

use futures;

use super::{Result, PdClient, AsyncPdClient, PdFuture};
use super::metrics::*;

struct Inner {
    members: pdpb::GetMembersResponse,
    client: PDAsyncClient,
}

pub struct RpcClient {
    cluster_id: u64,
    inner: RwLock<Inner>,
}

impl RpcClient {
    pub fn new(endpoints: &str) -> Result<RpcClient> {
        let endpoints: Vec<_> = endpoints.split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        let (client, members) = try!(validate_endpoints(&endpoints));
        Ok(RpcClient {
            cluster_id: members.get_header().get_cluster_id(),
            inner: RwLock::new(Inner {
                members: members,
                client: client,
            }),
        })
    }

    fn header(&self) -> pdpb::RequestHeader {
        let mut header = pdpb::RequestHeader::new();
        header.set_cluster_id(self.cluster_id);
        header
    }
}

fn get_members(client: &PDAsyncClient) -> Result<pdpb::GetMembersResponse> {
    let req = pdpb::GetMembersRequest::new();
    futures::Future::wait(client.GetMembers(req)).map_err(|e| box_err!(e))
}

pub fn validate_endpoints(endpoints: &[&str])
                          -> Result<(pdpb_grpc::PDAsyncClient, pdpb::GetMembersResponse)> {
    if endpoints.is_empty() {
        return Err(box_err!("empty PD endpoints"));
    }

    let len = endpoints.len();
    let mut endpoints_set = HashSet::with_capacity(len);

    let mut members = None;
    let mut cluster_id = None;
    for ep in endpoints {
        if !endpoints_set.insert(ep) {
            return Err(box_err!("duplicate PD endpoint {}", ep));
        }

        let client = match connect(ep) {
            Ok(c) => c,
            // Ignore failed PD node.
            Err(e) => {
                error!("PD endpoint {} is down: {:?}", ep, e);
                continue;
            }
        };

        let resp = match get_members(&client) {
            Ok(resp) => resp,
            // Ignore failed PD node.
            Err(e) => {
                error!("PD endpoint {} failed to respond: {:?}", ep, e);
                continue;
            }
        };

        // Check cluster ID.
        let cid = resp.get_header().get_cluster_id();
        if let Some(sample) = cluster_id {
            if sample != cid {
                return Err(box_err!("PD response cluster_id mismatch, want {}, got {}",
                                    sample,
                                    cid));
            }
        } else {
            cluster_id = Some(cid);
        }
        // TODO: check all fields later?

        if members.is_none() {
            members = Some(resp);
        }
    }

    info!("All PD endpoints are consistent, {:?}", endpoints);

    match members {
        Some(members) => {
            let (client, members) = box_try!(try_connect_leader(&members));
            Ok((client, members))
        }
        _ => Err(box_err!("PD cluster failed to respond")),
    }
}

fn connect(addr: &str) -> Result<pdpb_grpc::PDAsyncClient> {
    debug!("connect to PD endpoint: {:?}", addr);
    let (host, port) = match Url::parse(addr) {
        Ok(ep) => {
            let host = match ep.host_str() {
                Some(h) => h.to_owned(),
                None => return Err(box_err!("unkown host, please specify the host")),
            };
            let port = match ep.port() {
                Some(p) => p,
                None => return Err(box_err!("unkown port, please specify the port")),
            };
            (host, port)
        }

        Err(_) => {
            let mut parts = addr.split(':');
            (parts.next().unwrap().to_owned(), parts.next().unwrap().parse::<u16>().unwrap())
        }
    };

    let mut conf: grpc::client::GrpcClientConf = Default::default();
    conf.http.no_delay = Some(true);

    // TODO: It seems that `new` always return an Ok(_).
    match pdpb_grpc::PDAsyncClient::new(&host, port, false, conf) {
        Ok(client) => {
            // try request.
            match get_members(&client) {
                Ok(_) => Ok(client),
                Err(e) => Err(box_err!("fail to connect to {:?} {:?}", addr, e)),
            }
        }

        Err(e) => Err(box_err!(e)),
    }
}

fn try_connect_leader(members: &pdpb::GetMembersResponse)
                      -> Result<(pdpb_grpc::PDAsyncClient, pdpb::GetMembersResponse)> {
    // Try to connect the PD cluster leader.
    let leader = members.get_leader();
    for ep in leader.get_client_urls() {
        if let Ok(client) = connect(ep.as_str()) {
            info!("connect to PD leader {:?}", ep);
            return Ok((client, members.clone()));
        }
    }

    // Then try to connect other members.
    // Randomize endpoints.
    let members = members.get_members();
    let mut indexes: Vec<usize> = (0..members.len()).collect();
    rand::thread_rng().shuffle(&mut indexes);

    for i in indexes {
        for ep in members[i].get_client_urls() {
            match connect(ep.as_str()) {
                Ok(cli) => {
                    let resp = match get_members(&cli) {
                        Ok(resp) => resp,
                        Err(e) => {
                            error!("PD endpoint {} failed to respond: {:?}", ep, e);
                            continue;
                        }
                    };

                    return Ok((cli, resp));
                }
                Err(_) => {
                    error!("failed to connect to {}, try next", ep);
                    continue;
                }
            }
        }
    }

    Err(box_err!("failed to connect to {:?}", members))
}

fn check_resp_header(header: &pdpb::ResponseHeader) -> Result<()> {
    if !header.has_error() {
        return Ok(());
    }
    // TODO: translate more error types
    let err = header.get_error();
    Err(box_err!(err.get_message()))
}

impl fmt::Debug for RpcClient {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt,
               "PD gRPC Client connects to cluster {:?}",
               self.cluster_id)
    }
}

const MAX_RETRY_COUNT: usize = 100;
const RETRY_INTERVAL: u64 = 1;

// send a request in synchronized fashion.
fn do_request<F, R>(client: &RpcClient, f: F) -> Result<R>
    where F: Fn(&pdpb_grpc::PDAsyncClient) -> GrpcFutureSend<R>
{
    let mut resp = None;
    for _ in 0..MAX_RETRY_COUNT {
        let r: result::Result<_, grpc::error::GrpcError> = {
            let inner = client.inner.read().unwrap();
            let timer = PD_SEND_MSG_HISTOGRAM.start_timer();
            let future = f(&inner.client);
            let r = futures::Future::wait(future);
            timer.observe_duration();
            r
        };

        match r {
            Ok(r) => {
                resp = Some(r);
                break;
            }
            Err(e) => {
                error!("fail to request: {:?}", e);
                let mut inner = client.inner.write().unwrap();
                match try_connect_leader(&inner.members) {
                    Ok((cli, mbrs)) => {
                        inner.client = cli;
                        inner.members = mbrs;
                    }
                    Err(e) => {
                        error!("fail to connect to PD leader {:?}", e);
                        thread::sleep(Duration::from_secs(RETRY_INTERVAL));
                    }
                }
                continue;
            }
        }
    }

    resp.ok_or(box_err!("fail to request"))
}

impl PdClient for RpcClient {
    fn get_cluster_id(&self) -> Result<u64> {
        Ok(self.cluster_id)
    }

    fn bootstrap_cluster(&self, stores: metapb::Store, region: metapb::Region) -> Result<()> {
        let mut req = pdpb::BootstrapRequest::new();
        req.set_header(self.header());
        req.set_store(stores);
        req.set_region(region);

        let resp = try!(do_request(self, |client| client.Bootstrap(req.clone())));
        try!(check_resp_header(resp.get_header()));
        Ok(())
    }

    fn is_cluster_bootstrapped(&self) -> Result<bool> {
        let mut req = pdpb::IsBootstrappedRequest::new();
        req.set_header(self.header());

        let resp = try!(do_request(self, |client| client.IsBootstrapped(req.clone())));
        try!(check_resp_header(resp.get_header()));

        Ok(resp.get_bootstrapped())
    }

    fn alloc_id(&self) -> Result<u64> {
        let mut req = pdpb::AllocIDRequest::new();
        req.set_header(self.header());

        let resp = try!(do_request(self, |client| client.AllocID(req.clone())));
        try!(check_resp_header(resp.get_header()));

        Ok(resp.get_id())
    }

    fn put_store(&self, store: metapb::Store) -> Result<()> {
        let mut req = pdpb::PutStoreRequest::new();
        req.set_header(self.header());
        req.set_store(store);

        let resp = try!(do_request(self, |client| client.PutStore(req.clone())));
        try!(check_resp_header(resp.get_header()));

        Ok(())
    }

    fn get_store(&self, store_id: u64) -> Result<metapb::Store> {
        let mut req = pdpb::GetStoreRequest::new();
        req.set_header(self.header());
        req.set_store_id(store_id);

        let mut resp = try!(do_request(self, |client| client.GetStore(req.clone())));
        try!(check_resp_header(resp.get_header()));

        Ok(resp.take_store())
    }

    fn get_cluster_config(&self) -> Result<metapb::Cluster> {
        let mut req = pdpb::GetClusterConfigRequest::new();
        req.set_header(self.header());

        let mut resp = try!(do_request(self, |client| client.GetClusterConfig(req.clone())));
        try!(check_resp_header(resp.get_header()));

        Ok(resp.take_cluster())
    }

    fn get_region(&self, key: &[u8]) -> Result<metapb::Region> {
        let mut req = pdpb::GetRegionRequest::new();
        req.set_header(self.header());
        req.set_region_key(key.to_vec());

        let mut resp = try!(do_request(self, |client| client.GetRegion(req.clone())));
        try!(check_resp_header(resp.get_header()));

        Ok(resp.take_region())
    }

    fn get_region_by_id(&self, region_id: u64) -> Result<Option<metapb::Region>> {
        let mut req = pdpb::GetRegionByIDRequest::new();
        req.set_header(self.header());
        req.set_region_id(region_id);

        let mut resp = try!(do_request(self, |client| client.GetRegionByID(req.clone())));
        try!(check_resp_header(resp.get_header()));

        if resp.has_region() {
            Ok(Some(resp.take_region()))
        } else {
            Ok(None)
        }
    }

    fn region_heartbeat(&self,
                        region: metapb::Region,
                        leader: metapb::Peer,
                        down_peers: Vec<pdpb::PeerStats>,
                        pending_peers: Vec<metapb::Peer>)
                        -> Result<pdpb::RegionHeartbeatResponse> {
        let mut req = pdpb::RegionHeartbeatRequest::new();
        req.set_header(self.header());
        req.set_region(region);
        req.set_leader(leader);
        req.set_down_peers(RepeatedField::from_vec(down_peers));
        req.set_pending_peers(RepeatedField::from_vec(pending_peers));

        let resp = try!(do_request(self, |client| client.RegionHeartbeat(req.clone())));
        try!(check_resp_header(resp.get_header()));

        Ok(resp)
    }

    fn ask_split(&self, region: metapb::Region) -> Result<pdpb::AskSplitResponse> {
        let mut req = pdpb::AskSplitRequest::new();
        req.set_header(self.header());
        req.set_region(region);

        let resp = try!(do_request(self, |client| client.AskSplit(req.clone())));
        try!(check_resp_header(resp.get_header()));

        Ok(resp)
    }

    fn store_heartbeat(&self, stats: pdpb::StoreStats) -> Result<()> {
        let mut req = pdpb::StoreHeartbeatRequest::new();
        req.set_header(self.header());
        req.set_stats(stats);

        let resp = try!(do_request(self, |client| client.StoreHeartbeat(req.clone())));
        try!(check_resp_header(resp.get_header()));

        Ok(())
    }

    fn report_split(&self, left: metapb::Region, right: metapb::Region) -> Result<()> {
        let mut req = pdpb::ReportSplitRequest::new();
        req.set_header(self.header());
        req.set_left(left);
        req.set_right(right);

        let resp = try!(do_request(self, |client| client.ReportSplit(req.clone())));
        try!(check_resp_header(resp.get_header()));

        Ok(())
    }
}

// TODO: retry on failure.
// struct retry;
// impl futures::Future for retry {}
// Or https://github.com/srijs/rust-tokio-retry

impl AsyncPdClient for RpcClient {
    fn get_region_by_id(&self, region_id: u64) -> PdFuture<metapb::Region> {
        let mut req = pdpb::GetRegionByIDRequest::new();
        req.set_header(self.header());
        req.set_region_id(region_id);

        client.GetRegionByID(req).and_then(|resp| {
            try!(check_resp_header(resp.get_header()));
            if resp.has_region() {
                Ok(Some(resp.take_region()))
            } else {
                Ok(None)
            }
        })
    }

    fn region_heartbeat(&self,
                        region: metapb::Region,
                        leader: metapb::Peer,
                        down_peers: Vec<pdpb::PeerStats>,
                        pending_peers: Vec<metapb::Peer>)
                        -> PdFuture<pdpb::RegionHeartbeatResponse> {
        // let mut req = pdpb::RegionHeartbeatRequest::new();
        // req.set_header(self.header());
        // req.set_region(region);
        // req.set_leader(leader);
        // req.set_down_peers(RepeatedField::from_vec(down_peers));
        // req.set_pending_peers(RepeatedField::from_vec(pending_peers));

        // let resp = try!(do_request(self, |client| client.RegionHeartbeat(req.clone())));
        // try!(check_resp_header(resp.get_header()));

        // Ok(resp)

        unimplemented!()
    }

    fn ask_split(&self, region: metapb::Region) -> PdFuture<pdpb::AskSplitResponse> {
        // let mut req = pdpb::AskSplitRequest::new();
        // req.set_header(self.header());
        // req.set_region(region);

        // let resp = try!(do_request(self, |client| client.AskSplit(req.clone())));
        // try!(check_resp_header(resp.get_header()));

        // Ok(resp)

        unimplemented!()
    }

    fn store_heartbeat(&self, stats: pdpb::StoreStats) -> PdFuture<()> {
        // let mut req = pdpb::StoreHeartbeatRequest::new();
        // req.set_header(self.header());
        // req.set_stats(stats);

        // let resp = try!(do_request(self, |client| client.StoreHeartbeat(req.clone())));
        // try!(check_resp_header(resp.get_header()));

        // Ok(())

        unimplemented!()
    }

    fn report_split(&self, left: metapb::Region, right: metapb::Region) -> PdFuture<()> {
        // let mut req = pdpb::ReportSplitRequest::new();
        // req.set_header(self.header());
        // req.set_left(left);
        // req.set_right(right);

        // let resp = try!(do_request(self, |client| client.ReportSplit(req.clone())));
        // try!(check_resp_header(resp.get_header()));

        // Ok(())

        unimplemented!()
    }
}
