use chrono::prelude::*;
use libp2p::{
    core::upgrade,
    futures::StreamExt,
    mplex,
    noise::{Keypair, NoiseConfig, X25519Spec},
    swarm::{Swarm, SwarmBuilder},
    tcp::TokioTcpConfig,
    Transport,
};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{clone, time::Duration};
use tokio::{
    io::{stdin, AsyncBufReadExt, BufReader},
    select, spawn,
    sync::mpsc,
    time::sleep,
};

const DIFFICULTY_PREFIX: &str = "00";

mod p2p;

pub struct App {
    pub blocks: Vec<Block>,
}

#[derive(Serialize, Deserialize,Debug, Clone)]
pub struct Block {
    pub id: u64,
    pub hash: String,
    pub previous_hash: String,
    pub timestamp: i64,
    pub data: String,
    pub nonce: u64,
}

impl Block {
    pub fn new(id: u64, previous_hash: String, data: String) -> Self {
        let now = Utc::now();
        let (nonce, hash) = mine_block(id, now.timestamp(), &previous_hash, &data);
        Self {
            id,
            hash,
            previous_hash,
            timestamp: now.timestamp(),
            data,
            nonce,
        }
    }
}

fn calculate_hash(id: u64, timestamp: i64, previous_hash: &str, data: &str, nonce: u64) -> Vec<u8> {
    let data = serde_json::json!({
        "id": id,
        "previus hash": previous_hash,
        "data": data,
        "timestamp": timestamp,
        "nonce": nonce
    });
    let mut hasher = Sha256::new();
    hasher.update(data.to_string().as_bytes()); //'.as_bytes()' converts the string into a byte slice (&[u8]), which is the format required by the hasher.
    hasher.finalize().as_slice().to_owned()    //hasher.finalize() completes the hash computation and returns a fixed-size byte array (32 bytes for SHA-256).   // .as_slice() converts this array into a byte slice (&[u8]).    // .to_owned() creates an owned Vec<u8> (a vector of unsigned 8-bit integers), which is the return type of the function.   // Rationale: The final hash is returned as a Vec<u8> because it is a flexible and commonly used data structure in Rust for handling byte data. This allows the hash to be easily stored, transmitted, or compared with other hashes.
}

fn mine_block(id:u64, timestamp:i64, previous_hash: &str, data: &str ) -> (u64, String) {
    info!("mining bock");
    let mut nonce = 0;

    loop {
        if nonce % 10000 == 0 {
            info!("nonce: {}", nonce);
        }
        let hash = calculate_hash(id, timestamp, previous_hash, data, nonce);
        let binary_hash = hash_to_binary_representation(&hash);
        if binary_hash.starts_with(DIFFICULTY_PREFIX) {
            info!(
                "mined! nonce: {}, hash: {}, binary hash: {}",
                nonce,
                hex::encode(&hash),
                binary_hash
            );
            return (nonce, hex::encode(hash));
        }
        nonce += 1;
    }
}

fn hash_to_binary_representation(hash: &[u8]) -> String {
    let mut res: String = String::default();
    for c in hash {
        res.push_str(&format!("{:b}", c));
    }
    res
}

impl App {
    fn new() -> Self {
        Self { blocks: vec![] }
    }

    fn genesis(&mut self) {
        let genesis_block = Block {
            id: 0,
            hash: "0000f816a87f806bb0073dcf026a64fb40c946b5abee2573702828694d5b4c43".to_string(),
            previous_hash: String::from("genesis"),
            timestamp: Utc::now().timestamp(),
            data: String::from("genesis"),
            nonce: 2836,
        };
        self.blocks.push(genesis_block);
    }

    fn try_add_block(&mut self, block: Block) {
        let latest_block = self.blocks.last().expect("there is atleast one block");
        if self.is_block_valid(&block, latest_block) {
            self.blocks.push(block);
        } else {
            error!("couldnt add the block - invalid");
        }
    }

    fn is_block_valid(&self, block: &Block, previous_block: &Block) -> bool {
        if block.previous_hash != previous_block.hash {      //checking prev hash
            warn!("block with id: {} has wrong prev hash", block.id);
            return false;
        } else if !hash_to_binary_representation(     //checking proof of work difficulty
            &hex::decode(&block.hash).expect("can decode from hex"),
        )
        .starts_with(DIFFICULTY_PREFIX)
        {
            warn!("block with id: {} has invalid difficulty", block.id);
            return false;
        } else if block.id != previous_block.id + 1 {     //block sequence
            warn!(
                "block with id: {} is not the next block after th latesr: {}",
                block.id, previous_block.id
            );
            return false;
        } else if hex::encode(calculate_hash(
            block.id,
            block.timestamp,
            &block.previous_hash,
            &block.data,
            block.nonce,
        )) != block.hash
        {
            warn!("block with id:{} has invalid hash", block.id);
            return false;
        }
        true
    }

    fn is_chain_valid(&self, chain: &[Block]) -> bool {
        for i in 0..chain.len() {
            if i == 0 {
                continue;
            }
            let first = chain.get(i - 1).expect("has to exist");
            let second = chain.get(i).expect("has to exist");
            if !self.is_block_valid(second, first) {
                return false;
            }
        }
        true
    }

    //longest valid chain
    fn choose_chain(&mut self, local: Vec<Block>, remote: Vec<Block>) -> Vec<Block> {
        let is_local_valid = self.is_chain_valid(&local);
        let is_remote_valid = self.is_chain_valid(&remote);

        if is_local_valid && is_remote_valid {
            if local.len() >= remote.len() {
                local
            } else {
                remote
            }
        } else if is_remote_valid && !is_local_valid {
            remote
        } else if !is_remote_valid && is_local_valid {
            local
        } else {
            panic!("local and remote chains are both invalid");
        }
    }
}

#[tokio::main]
async fn main(){
    pretty_env_logger::init();                     //helps output log in human readable form

    info!("Peer Id:{}", p2p::PEER_ID.clone());
    let(response_sender, mut response_rcv) = mpsc::unbounded_channel();          //mpsc - multi producer, single consumer, used for communication bw diff parts of application, especially in async tasks
    let (init_sender, mut init_rcv) = mpsc::unbounded_channel();                 //unbounded_channel can store as many msgs as needed

    let auth_keys = Keypair::<X25519Spec>::new()
        .into_authentic(&p2p::KEYS)
        .expect("can create auth keys");

    let transp = TokioTcpConfig::new()          //creates a transmission control protocol for async runtie
        .upgrade(upgrade::Version::V1)
        .authenticate(NoiseConfig::xx(auth_keys).into_authenticated())    //Noise protocol is a framework for building secure communication protocols using cryptographic primitives. The xx refers to a specific handshake pattern used in Noise
        .multiplex(mplex::MplexConfig::new())    //Multiplexing allows multiple streams of data to be sent over a single connection, which is useful in P2P networks where you may want to communicate about different topics or with different peers simultaneously. mplex is a multiplexing protocol used in libp2p.
        .boxed();    //This wraps the transport configuration in a Box, which is a type of smart pointer in Rust. It allows for dynamic dispatch, making it easier to work with the transport in a flexible way.

    let behaviour = p2p::AppBehaviour::new(App::new(), response_sender, init_sender.clone()).await;

    let mut swarm = SwarmBuilder::new(transp, behaviour, *p2p::PEER_ID)
        .executor(Box::new(|fut| {               //The executor is responsible for running asynchronous tasks. Here, it is given a closure (|fut| { spawn(fut); }) that spawns tasks using the spawn() function.
            spawn(fut);
        }))
        .build();

    let mut stdin = BufReader::new(stdin()).lines();

    Swarm::listen_on(
        &mut swarm,
        "/ip4/0.0.0.0/tcp/0"
            .parse()
            .expect("can get a local socket"),
    )
    .expect("swarm can be started");

    spawn(async move{
        sleep(Duration::from_secs(1)).await;
        info!("sending init event");
        init_sender.send(true).expect("can send init events");
    });

    loop {
        let evt = {
            select! {
                line = stdin.next_line() => Some(p2p::EventType::Input(line.expect("can get line"). expect("can read line from stdin"))),
                response = response_rcv.recv() => {
                    Some(p2p::EventType::LocalChainResponse(response.expect("response exists")))
                },
                _init = init_rcv.recv() => {             //The underscore _init means that the actual value is not used anywhere in the code, so it's just for triggering the event.
                    Some(p2p::EventType::Init)
                }
                event = swarm.select_next_some() => {
                    info!("unhandled Swarm Event: {:?}", event);
                    None
                },
            }
        };

        if let Some(event) = evt {
            match event {
                p2p::EventType::Init => {
                    let peers = p2p::get_list_peers(&swarm);
                    swarm.behaviour_mut().app.genesis();

                    info!("connected nodes: {}", peers.len());
                    if !peers.is_empty(){
                        let req = p2p::LocalChainRequest {
                            from_peer_id: peers
                                .iter()
                                .last()
                                .expect("at least one peer")
                                .to_string(),
                        };

                        let json = serde_json::to_string(&req).expect("can jsonify req");
                        swarm
                            .behaviour_mut()
                            .floodsub
                            .publish(p2p::CHAIN_TOPIC.clone(), json.as_bytes());
                    }
                }
                p2p::EventType::LocalChainResponse(resp) => {
                    let json = serde_json::to_string(&resp).expect("can jsonify response");
                    swarm
                        .behaviour_mut()
                        .floodsub
                        .publish(p2p::CHAIN_TOPIC.clone(), json.as_bytes());
                }
                p2p::EventType::Input(line) => match line.as_str(){
                    "ls p" => p2p::handle_print_peers(&swarm),
                    cmd if cmd.starts_with("ls c") => p2p::handle_print_chain(&swarm),
                    cmd if cmd.starts_with("create b") => p2p::handle_create_block(cmd, &mut swarm),
                    _ => error!("unknown command"),
                },   
            }
        }
    }
}