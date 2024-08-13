use core::time;

use chrono::Utc;
use libp2p::core::either::EitherOutbound;
use log::{error, info, warn};
use sha2::{Digest, Sha256};

pub struct App {
    pub blocks: Vec<Block>,
}

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
    hasher.finalize().as_slice().to_owned()
    //hasher.finalize() completes the hash computation and returns a fixed-size byte array (32 bytes for SHA-256).
    // .as_slice() converts this array into a byte slice (&[u8]).
    // .to_owned() creates an owned Vec<u8> (a vector of unsigned 8-bit integers), which is the return type of the function.
    // Rationale: The final hash is returned as a Vec<u8> because it is a flexible and commonly used data structure in Rust for handling byte data. This allows the hash to be easily stored, transmitted, or compared with other hashes.
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

const DIFFICULTY_PREFIX: &str = "00";

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
        if block.previous_hash != previous_block.hash {
            //checking prev hash
            warn!("block with id: {} has wrong prev hash", block.id);
            return false;
        } else if !hash_to_binary_representation(
            //checking proof of work difficulty
            &hex::decode(&block.hash).expect("can decode from hex"),
        )
        .starts_with(DIFFICULTY_PREFIX)
        {
            warn!("block with id: {} has invalid difficulty", block.id);
            return false;
        } else if block.id != previous_block.id + 1 {
            //block sequence
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

fn main() {}
