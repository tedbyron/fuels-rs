contract;

use std::*;
use core::*;
use std::storage::*;

abi TestContract {
  fn initialize_storage_slot(gas: u64, coin: u64, color: b256, storage_key: b256) -> u64;
  fn get_storage_slot(gas: u64, coin: u64, color: b256, storage_key: b256) -> u64;
  fn test_storage_slot(gas: u64, coin: u64, color: b256, storage_key: b256) -> b256;
}

impl TestContract for Contract {
  fn initialize_storage_slot(gas: u64, coin: u64, color: b256, storage_key: b256) -> u64 {
    store(storage_key, 214);
    get(storage_key)
  }
  fn get_storage_slot(gas: u64, coin: u64, color: b256, storage_key: b256) -> u64 {
    get(storage_key)
  }
  fn test_storage_slot(gas: u64, coin: u64, color: b256, storage_key: b256) -> b256 {
    storage_key
  }
}

