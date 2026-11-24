#![no_std]

use soroban_sdk::{contract, contractimpl, Env};

#[contract]
pub struct ReleaseProfileSafe;

#[contractimpl]
impl ReleaseProfileSafe {
    pub fn ping(_env: Env) {}
}
