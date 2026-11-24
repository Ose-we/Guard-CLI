#![no_std]

use soroban_sdk::{contract, contractimpl, Env};

#[contract]
pub struct ReleaseProfileVulnerable;

#[contractimpl]
impl ReleaseProfileVulnerable {
    pub fn ping(_env: Env) {}
}
