// src/constants.rs
use std::env;

pub struct Env {
    pub wss_url: String,
    pub https_url: String,
    pub factory_address: String,
    pub factory_creation_block: u64,
    pub token_in: String,
}

impl Env {
    pub fn new() -> Self {
        Self {
            wss_url: env::var("WSS_URL").expect("WSS_URL must be set"),
            https_url: env::var("HTTPS_URL").expect("HTTPS_URL must be set"),
            factory_address: env::var("FACTORY_ADDRESS").expect("FACTORY_ADDRESS must be set"),
            factory_creation_block: env::var("FACTORY_CREATION_BLOCK").expect("FACTORY_CREATION_BLOCK must be set").parse().expect("FACTORY_CREATION_BLOCK must be a number"),
            token_in: env::var("TOKEN_IN").expect("TOKEN_IN must be set"),
        }
    }
}
