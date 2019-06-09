#![allow(non_snake_case)]
#![feature(proc_macro_hygiene)]
#![no_std]

extern crate pwasm_std;
extern crate pwasm_abi;
extern crate pwasm_abi_derive;
extern crate pwasm_ethereum;
extern crate parity_wasm;
extern crate validator;

use pwasm_abi::types::*;
use core::default::Default;
pub mod proc_table;

pub mod ext {
    extern "C" {
            pub fn extcodesize( address: *const u8) -> i32;
            pub fn extcodecopy( dest: *mut u8, address: *const u8);
    }
}

pub fn extcodesize(address: &Address) -> i32 {
    unsafe { ext::extcodesize(address.as_ptr()) }
}

pub fn extcodecopy(address: &Address) -> pwasm_std::Vec<u8> {
    let len = unsafe { ext::extcodesize(address.as_ptr()) };
    match len {
        0 => pwasm_std::Vec::new(),
        non_zero => {
            let mut data = pwasm_std::Vec::with_capacity(non_zero as usize);
            unsafe {
                data.set_len(non_zero as usize);
                ext::extcodecopy(data.as_mut_ptr(), address.as_ptr());
            }
            data
        }
    }
}

type ProcedureKey = [u8; 24];

pub mod token {
    use pwasm_abi::types::*;
    use validator::{Validity, Module};
    use pwasm_ethereum;
    
    use crate::proc_table;
    use crate::proc_table::cap;

    use pwasm_abi_derive::eth_abi;

    #[eth_abi(TokenEndpoint, KernelClient)]
    pub trait KernelInterface {
        /// The constructor set with Initial Entry Procedure
        fn constructor(&mut self, _entry_proc_key: String, _entry_proc_address: Address);
        /// Get Entry Procedure
        #[constant]
        fn entryProcedure(&mut self) -> [u8; 24];
        /// Get Current Executing Procedure
        #[constant]
        fn currentProcedure(&mut self) -> [u8; 24];

        /// Get Procedure Address By Key
        /// Returns 0 if Procedure Not Found
        fn getProcedureByKey(&mut self, _proc_key: String) -> Address;
        /// Check if Procedure Contract is Valid
        fn check_contract(&mut self, _to: Address) -> bool;
        /// Get the size (in bytes) of another contract
        fn get_code_size(&mut self, _to: Address) -> i32;
        /// Copy the code of another contract into memory
        fn code_copy(&mut self, _to: Address) -> pwasm_std::Vec<u8>;
    }

    pub struct KernelContract;

    impl KernelInterface for KernelContract {
        
        fn constructor(&mut self, _entry_proc_key: String, _entry_proc_address: Address) {
            let _entry_proc_key = {
                let byte_key = _entry_proc_key.as_bytes();
                let len = byte_key.len();
                let mut output = [0u8; 24];
                output[..len].copy_from_slice(byte_key);
                output
            };

            proc_table::insert_proc(_entry_proc_key, _entry_proc_address, cap::NewCapList::empty()).unwrap();
            proc_table::set_entry_proc_id(_entry_proc_key).unwrap();
        }

        fn entryProcedure(&mut self) -> [u8; 24] {
            proc_table::get_entry_proc_id()
        }

        fn currentProcedure(&mut self) -> [u8; 24] {
            proc_table::get_current_proc_id()
        }

        fn getProcedureByKey(&mut self, _proc_key: String) -> Address {
            let _proc_key = {
                let byte_key = _proc_key.as_bytes();
                let len = byte_key.len();
                let mut output = [0u8; 24];
                output[..len].copy_from_slice(byte_key);
                output
            };

            proc_table::get_proc_addr(_proc_key).unwrap_or(H160::zero())
        }

        fn check_contract(&mut self, target: Address) -> bool {
            // First we check if the target is the null address. If so we return
            // false.
            if target == H160::zero() {
                false
            } else {
                // Next we get the code of the contract, using EXTCODECOPY under
                // the hood.
                let code: pwasm_std::Vec<u8> = self.code_copy(target);
                Module::new(code.as_slice()).is_valid()
            }
        }

        fn get_code_size(&mut self, to: Address) -> i32 {
            super::extcodesize(&to)
        }

        fn code_copy(&mut self, to: Address) -> pwasm_std::Vec<u8> {
            super::extcodecopy(&to)
        }
    }
}
// Declares the dispatch and dispatch_ctor methods
use pwasm_abi::eth::EndpointInterface;

#[no_mangle]
pub fn call() {
    let mut endpoint = token::TokenEndpoint::new(token::KernelContract {});
    // Read http://solidity.readthedocs.io/en/develop/abi-spec.html#formal-specification-of-the-encoding for details
    pwasm_ethereum::ret(&endpoint.dispatch(&pwasm_ethereum::input()));
}

#[no_mangle]
pub fn deploy() {
    let mut endpoint = token::TokenEndpoint::new(token::KernelContract {});
    endpoint.dispatch_ctor(&pwasm_ethereum::input());
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    extern crate pwasm_test;
    use self::pwasm_test::{ext_get, ext_reset};
    use super::*;
    use core::str::FromStr;
    use pwasm_abi::types::*;
    use token::KernelInterface;

    #[test]
    fn should_initialize_with_entry_procedure() {
        let mut contract = token::KernelContract {};

        let owner_address = Address::from_str("ea674fdde714fd979de3edf0f56aa9716b898ec8").unwrap();
        let entry_proc_key = pwasm_abi::types::String::from("init");
        let entry_proc_address =
            Address::from_str("db6fd484cfa46eeeb73c71edee823e4812f9e2e1").unwrap();

        // Here we're creating an External context using ExternalBuilder and set the `sender` to the `owner_address`
        // so `pwasm_ethereum::sender()` in KernelInterface::constructor() will return that `owner_address`
        ext_reset(|e| e.sender(owner_address.clone()));

        contract.constructor(entry_proc_key.clone(), entry_proc_address.clone());

        assert_eq!(&contract.entryProcedure()[0..4], entry_proc_key.as_bytes());
        assert_eq!(contract.currentProcedure(), [0u8; 24]);
    }

}
