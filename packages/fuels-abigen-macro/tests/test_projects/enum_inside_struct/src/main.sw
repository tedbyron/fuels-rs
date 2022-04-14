contract;

use std::*;
use core::*;
use std::storage::*;

enum Shaker {
    Cosmopolitan:u64,
    Mojito:u64,
}

struct Cocktail {
    the_thing_you_mix_in: Shaker,
    glass: u64,
}

abi TestContract {
    fn give_and_return_enum_inside_struct(a: u64) -> Cocktail;
}


impl TestContract for Contract {
    fn give_and_return_enum_inside_struct(a: u64) -> Cocktail {
        let b = Cocktail {
            the_thing_you_mix_in: Shaker::Mojito(222),
            glass: 333
        };
        b
    }
}
