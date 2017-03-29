// Copyright 2017 PingCAP, Inc.
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

use std::thread;
use std::time::Duration;
use std::collections::HashSet;

use grpc;
use grpc::futures_grpc::GrpcFutureSend;

use protobuf::RepeatedField;

use url::Url;

use rand::{self, Rng};

use futures::Future;

use kvproto::metapb;
use kvproto::pdpb::{self, GetMembersResponse};
use kvproto::pdpb_grpc::{PDAsync, PDAsyncClient};

use super::super::{Result, Error, PdClient};
use super::super::metrics::*;
use super::RpcAsyncClient;


pub fn validate_endpoints(endpoints: &[String]) -> Result<(PDAsyncClient, GetMembersResponse)> {
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

        let resp = match Future::wait(client.GetMembers(pdpb::GetMembersRequest::new())) {
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

    match members {
        Some(members) => {
            let (client, members) = try!(try_connect_leader(&members));
            info!("All PD endpoints are consistent: {:?}", endpoints);
            Ok((client, members))
        }
        _ => Err(box_err!("PD cluster failed to respond")),
    }
}

fn connect(addr: &str) -> Result<PDAsyncClient> {
    debug!("connect to PD endpoint: {:?}", addr);
    let ep = box_try!(Url::parse(addr));
    let host = match ep.host_str() {
        Some(h) => h.to_owned(),
        None => return Err(box_err!("unkown host, please specify the host")),
    };
    let port = match ep.port() {
        Some(p) => p,
        None => return Err(box_err!("unkown port, please specify the port")),
    };

    let mut conf: grpc::client::GrpcClientConf = Default::default();
    conf.http.no_delay = Some(true);

    // TODO: It seems that `new` always return an Ok(_).
    PDAsyncClient::new(&host, port, false, conf)
        .and_then(|client| {
            // try request.
            match Future::wait(client.GetMembers(pdpb::GetMembersRequest::new())) {
                Ok(_) => Ok(client),
                Err(e) => Err(e),
            }
        })
        .map_err(Error::Grpc)
}

pub fn try_connect_leader(previous: &GetMembersResponse)
                          -> Result<(PDAsyncClient, GetMembersResponse)> {
    // Try to connect other members.
    // Randomize endpoints.
    let members = previous.get_members();
    let mut indexes: Vec<usize> = (0..members.len()).collect();
    rand::thread_rng().shuffle(&mut indexes);

    let mut resp = None;
    'outer: for i in indexes {
        for ep in members[i].get_client_urls() {
            match connect(ep.as_str()) {
                Ok(c) => {
                    match Future::wait(c.GetMembers(pdpb::GetMembersRequest::new())) {
                        Ok(r) => {
                            resp = Some(r);
                            break 'outer;
                        }
                        Err(e) => {
                            error!("PD endpoint {} failed to respond: {:?}", ep, e);
                            continue;
                        }
                    };
                }
                Err(e) => {
                    error!("failed to connect to {}, {:?}", ep, e);
                    continue;
                }
            }
        }
    }

    // Then try to connect the PD cluster leader.
    if let Some(resp) = resp {
        let leader = resp.get_leader().clone();
        for ep in leader.get_client_urls() {
            if let Ok(client) = connect(ep.as_str()) {
                info!("connect to PD leader {:?}", ep);
                return Ok((client, resp));
            }
        }
    }

    Err(box_err!("failed to connect to {:?}", members))
}

const MAX_RETRY_COUNT: usize = 100;
const RETRY_INTERVAL: u64 = 1;

fn do_request<F, R>(client: &RpcAsyncClient, f: F) -> Result<R>
    where F: Fn(&PDAsyncClient) -> GrpcFutureSend<R>
{
    for _ in 0..MAX_RETRY_COUNT {
        let r = {
            let timer = PD_SEND_MSG_HISTOGRAM.start_timer();
            let r = Future::wait(f(&client.inner.get_client()));
            timer.observe_duration();
            r
        };

        match r {
            Ok(r) => {
                return Ok(r);
            }
            Err(e) => {
                error!("fail to request: {:?}", e);
                match try_connect_leader(&client.inner.clone_members()) {
                    Ok((cli, mbrs)) => {
                        client.inner.set_client(cli);
                        client.inner.set_members(mbrs);
                    }
                    Err(e) => {
                        error!("fail to connect to PD leader {:?}", e);
                        thread::sleep(Duration::from_secs(RETRY_INTERVAL));
                    }
                }
            }
        }
    }

    Err(box_err!("fail to request"))
}

fn check_resp_header(header: &pdpb::ResponseHeader) -> Result<()> {
    if !header.has_error() {
        return Ok(());
    }
    // TODO: translate more error types
    let err = header.get_error();
    Err(box_err!(err.get_message()))
}

impl PdClient for RpcAsyncClient {
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
